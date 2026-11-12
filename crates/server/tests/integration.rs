//! Integration test: full upload → query → delete lifecycle.
//!
//! Starts a real server on a random port, uploads a ULog fixture,
//! verifies all API endpoints, then cleans up.

use serde_json::Value;
use std::net::TcpListener;
use std::sync::Arc;

/// Find a free port by binding to :0 and reading back the assigned port.
fn free_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

/// Resolve a test fixture path relative to the workspace root.
fn fixture_path(name: &str) -> std::path::PathBuf {
    let manifest = env!("CARGO_MANIFEST_DIR");

    // First: check local fixtures in the converter crate
    let local = std::path::Path::new(manifest)
        .parent()
        .unwrap() // crates/
        .parent()
        .unwrap() // workspace root
        .join("crates/converter/tests/fixtures")
        .join(name);
    if local.exists() {
        return local;
    }

    // Fallback: px4-ulog-rs repo (local dev)
    std::path::Path::new(manifest)
        .parent()
        .unwrap() // crates/
        .parent()
        .unwrap() // workspace root
        .parent()
        .unwrap() // ulog/
        .join("px4-ulog-rs/tests/fixtures")
        .join(name)
}

/// Start the server in the background and return the base URL.
async fn start_server() -> (String, tokio::task::JoinHandle<()>) {
    start_server_with(None).await
}

async fn start_server_with(
    vehicle_name_header: Option<&str>,
) -> (String, tokio::task::JoinHandle<()>) {
    let (base_url, handle, _) = start_server_with_state(vehicle_name_header).await;
    (base_url, handle)
}

/// Like `start_server_with`, plus the server's state (database, storage).
async fn start_server_with_state(
    vehicle_name_header: Option<&str>,
) -> (
    String,
    tokio::task::JoinHandle<()>,
    Arc<flight_review_server::AppState>,
) {
    let port = free_port();
    let base_url = format!("http://127.0.0.1:{}", port);

    let tmp_dir = tempfile::tempdir().unwrap();
    let db_path = tmp_dir.path().join("test.db");
    let storage_path = tmp_dir.path().join("files");
    std::fs::create_dir_all(&storage_path).unwrap();

    let db_url = format!("sqlite://{}", db_path.display());
    let storage_url = format!("file://{}", storage_path.display());

    let db = flight_review_server::db::create_db(&db_url)
        .await
        .expect("failed to create test db");
    let storage = Arc::new(
        flight_review_server::storage::FileStorage::from_url(&storage_url)
            .expect("failed to create test storage"),
    );

    let state = Arc::new(flight_review_server::AppState {
        db,
        storage,
        v1_ulg_prefix: None,
        mapbox_token: None,
        vehicle_name_header: vehicle_name_header.map(String::from),
        http_client: reqwest::Client::new(),
    });

    // Use the same router the binary serves, so the test never drifts from
    // the real route table.
    let app = flight_review_server::build_router(state.clone());

    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{}", port))
        .await
        .unwrap();

    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Wait for server to be ready
    for _ in 0..50 {
        if reqwest::get(format!("{}/health", base_url)).await.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    // Keep tmp_dir alive by leaking it (cleaned up on process exit)
    std::mem::forget(tmp_dir);

    (base_url, handle, state)
}

#[tokio::test]
async fn test_full_lifecycle() {
    let (base_url, _handle) = start_server().await;
    let client = reqwest::Client::new();

    // 1. Health check
    let resp = client
        .get(format!("{}/health", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "ok");

    // 1b. Version info — all five fields present and non-empty.
    let resp = client
        .get(format!("{}/api/version", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let body: Value = resp.json().await.unwrap();
    for field in ["server", "converter", "px4_ulog", "git_sha", "build_time"] {
        let v = body[field].as_str();
        assert!(
            v.is_some_and(|s| !s.is_empty()),
            "version field '{field}' missing or empty: {body:?}"
        );
    }
    assert_eq!(
        body["px4_ulog"], "0.6.1",
        "px4-ulog version should match Cargo.lock"
    );

    // 2. Upload a log
    let fixture = fixture_path("sample.ulg");
    assert!(fixture.exists(), "Fixture not found: {:?}", fixture);

    let file_bytes = std::fs::read(&fixture).unwrap();
    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(file_bytes)
                .file_name("sample.ulg")
                .mime_str("application/octet-stream")
                .unwrap(),
        )
        .text("is_public", "true")
        .text("description", "Integration test upload")
        .text("pilot_name", "CI Bot")
        .text("tags", "test,ci");

    let resp = client
        .post(format!("{}/api/upload", base_url))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200, "Upload failed");
    let upload: Value = resp.json().await.unwrap();

    let log_id = upload["id"].as_str().expect("no id in upload response");
    let delete_token = upload["delete_token"]
        .as_str()
        .expect("no delete_token in upload response");
    assert_eq!(upload["sys_name"], "PX4");
    assert_eq!(upload["topic_count"], 15);
    assert!(upload["is_public"].as_bool().unwrap());
    assert!(upload["flight_duration_s"].as_f64().unwrap() > 0.0);

    // 3. List logs — should appear
    let resp = client
        .get(format!("{}/api/logs", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let list: Value = resp.json().await.unwrap();
    assert_eq!(list["total"], 1);
    assert_eq!(list["logs"][0]["id"], log_id);
    assert_eq!(list["logs"][0]["description"], "Integration test upload");
    assert_eq!(list["logs"][0]["pilot_name"], "CI Bot");

    // 4. Get single log
    let resp = client
        .get(format!("{}/api/logs/{}", base_url, log_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let log: Value = resp.json().await.unwrap();
    assert_eq!(log["sys_name"], "PX4");
    assert_eq!(log["ver_hw"], "AUAV_X21");
    assert_eq!(log["is_public"], true);
    assert!(log["topic_count"].as_i64().unwrap() >= 15);

    // 5. Get metadata.json
    let resp = client
        .get(format!(
            "{}/api/logs/{}/data/metadata.json",
            base_url, log_id
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let metadata: Value = resp.json().await.unwrap();
    assert_eq!(metadata["sys_name"], "PX4");
    assert!(metadata["parameters"].as_object().unwrap().len() > 400);
    assert!(metadata["topics"].as_object().unwrap().len() >= 15);
    // Verify analysis is present
    let analysis = &metadata["analysis"];
    assert!(!analysis.is_null(), "analysis should be present");
    assert!(
        analysis["flight_modes"].as_array().unwrap().len() > 0,
        "should have flight modes"
    );

    // 6. Get Parquet file with Range request
    let resp = client
        .get(format!(
            "{}/api/logs/{}/data/vehicle_attitude.parquet",
            base_url, log_id
        ))
        .header("Range", "bytes=0-1023")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 206, "Should return 206 Partial Content");
    assert_eq!(resp.bytes().await.unwrap().len(), 1024);

    // 7. Get raw .ulg file
    let resp = client
        .get(format!("{}/api/logs/{}/data/sample.ulg", base_url, log_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let ulg_size = resp.bytes().await.unwrap().len();
    assert!(ulg_size > 1_000_000, "ULG should be > 1MB");

    // 8. Stats endpoint
    let resp = client
        .get(format!("{}/api/stats?group_by=ver_hw&period=all", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let stats: Value = resp.json().await.unwrap();
    assert_eq!(stats["group_by"], "ver_hw");
    assert!(stats["data"].as_array().unwrap().len() > 0);
    assert_eq!(stats["data"][0]["group"], "AUAV_X21");

    // 9. Search filters
    let resp = client
        .get(format!("{}/api/logs?sys_name=PX4&has_gps=false", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let filtered: Value = resp.json().await.unwrap();
    // sample.ulg may or may not have GPS — just verify the filter doesn't error
    assert!(filtered["total"].as_i64().is_some());

    // 10. Delete with wrong token — should fail
    let resp = client
        .delete(format!("{}/api/logs/{}?token=wrongtoken", base_url, log_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 403, "Wrong token should be forbidden");

    // 11. Delete with correct token
    let resp = client
        .delete(format!(
            "{}/api/logs/{}?token={}",
            base_url, log_id, delete_token
        ))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204, "Correct token should succeed");

    // 12. Verify deleted
    let resp = client
        .get(format!("{}/api/logs/{}", base_url, log_id))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "Deleted log should return 404");

    // 13. List should be empty
    let resp = client
        .get(format!("{}/api/logs", base_url))
        .send()
        .await
        .unwrap();
    let list: Value = resp.json().await.unwrap();
    assert_eq!(list["total"], 0);
}

/// Behind an authenticating proxy, the vehicle name comes from the proxy's
/// header, not from the form, and logs can be listed per vehicle.
#[tokio::test]
async fn test_vehicle_name_header_and_filter() {
    let (base_url, _handle) = start_server_with(Some("X-Drone-Id")).await;
    let client = reqwest::Client::new();
    let bytes = std::fs::read(fixture_path("sample.ulg")).unwrap();

    let upload = |header: Option<&'static str>, form_name: &'static str| {
        let form = reqwest::multipart::Form::new()
            .part(
                "file",
                reqwest::multipart::Part::bytes(bytes.clone()).file_name("sample.ulg"),
            )
            .text("vehicle_name", form_name);
        let mut req = client
            .post(format!("{}/api/upload", base_url))
            .multipart(form);
        if let Some(h) = header {
            req = req.header("X-Drone-Id", h);
        }
        req.send()
    };

    // The header wins over what the form claims.
    let resp = upload(Some("fd-0001"), "fd-0002").await.unwrap();
    assert_eq!(resp.status(), 200);
    // Without the header, the form field is used.
    let resp = upload(None, "fd-0002").await.unwrap();
    assert_eq!(resp.status(), 200);

    let list = |name: &str| {
        client
            .get(format!(
                "{}/api/logs?include_private=true&vehicle_name={}",
                base_url, name
            ))
            .send()
    };
    let body: Value = list("fd-0001").await.unwrap().json().await.unwrap();
    assert_eq!(body["total"], 1, "{body:?}");
    assert_eq!(body["logs"][0]["vehicle_name"], "fd-0001");
    let body: Value = list("fd-0002").await.unwrap().json().await.unwrap();
    assert_eq!(body["total"], 1, "{body:?}");
    // Exact match: a prefix finds nothing.
    let body: Value = list("fd-000").await.unwrap().json().await.unwrap();
    assert_eq!(body["total"], 0, "{body:?}");
}

/// FireDrone: stats per drone (group_by=vehicle_name, vehicle_name filter) and
/// time per flight mode, filled at upload and by the startup backfill.
#[tokio::test]
async fn test_stats_per_drone_and_flight_modes() {
    let (base_url, _handle, state) = start_server_with_state(Some("X-Drone-Id")).await;
    let client = reqwest::Client::new();
    let bytes = std::fs::read(fixture_path("sample.ulg")).unwrap();

    let mut ids = Vec::new();
    for drone in ["fd-0001", "fd-0001", "fd-0002"] {
        let form = reqwest::multipart::Form::new()
            .part(
                "file",
                reqwest::multipart::Part::bytes(bytes.clone()).file_name("sample.ulg"),
            )
            .text("is_public", "true");
        let resp = client
            .post(format!("{}/api/upload", base_url))
            .header("X-Drone-Id", drone)
            .multipart(form)
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body: Value = resp.json().await.unwrap();
        ids.push(body["id"].as_str().unwrap().parse::<uuid::Uuid>().unwrap());
    }
    let get = |path: String| {
        let client = client.clone();
        let url = format!("{}{}", base_url, path);
        async move {
            client
                .get(url)
                .send()
                .await
                .unwrap()
                .json::<Value>()
                .await
                .unwrap()
        }
    };

    let drones = get("/api/stats?group_by=vehicle_name&period=all".into()).await;
    let groups: Vec<(String, i64)> = drones["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|g| {
            (
                g["group"].as_str().unwrap().into(),
                g["count"].as_i64().unwrap(),
            )
        })
        .collect();
    assert_eq!(groups, vec![("fd-0001".into(), 2), ("fd-0002".into(), 1)]);
    let one = get("/api/stats?group_by=ver_hw&period=all&vehicle_name=fd-0002".into()).await;
    assert_eq!(one["data"][0]["count"], 1, "{one:?}");

    let modes = |drone: &str| {
        get(format!(
            "/api/stats/flight-modes?period=all&vehicle_name={drone}"
        ))
    };
    let single = modes("fd-0002").await;
    let single = single["data"].as_array().unwrap().clone();
    assert!(!single.is_empty(), "sample.ulg has flight modes");
    assert!(single
        .iter()
        .all(|m| m["logs"] == 1 && m["total_s"].as_f64().unwrap() > 0.0));
    let double = modes("fd-0001").await;
    let double = double["data"].as_array().unwrap().clone();
    assert_eq!(double.len(), single.len());
    for (d, s) in double.iter().zip(&single) {
        assert_eq!(d["mode"], s["mode"]);
        assert_eq!(d["logs"], 2);
        let (d, s) = (
            d["total_s"].as_f64().unwrap(),
            s["total_s"].as_f64().unwrap(),
        );
        assert!((d - 2.0 * s).abs() < 1e-6, "{d} vs 2 x {s}");
    }

    // Logs from before log_flight_modes: the backfill reads metadata.json.
    state.db.insert_flight_modes(ids[2], &[]).await.unwrap();
    assert_eq!(modes("fd-0002").await["data"], serde_json::json!([]));
    assert_eq!(
        state.db.logs_without_flight_modes().await.unwrap(),
        vec![ids[2]]
    );
    flight_review_server::flight_modes::backfill(state.db.clone(), state.storage.clone()).await;
    assert_eq!(modes("fd-0002").await["data"], Value::Array(single));
}
