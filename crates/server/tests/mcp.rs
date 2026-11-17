//! FireDrone: the read-only MCP endpoint (`mcp.rs`), over real HTTP with raw
//! JSON-RPC, the way an MCP client talks to it.

use serde_json::{json, Value};
use std::sync::Arc;

const USER_HEADER: &str = "X-Fleet-User";

fn fixture(name: &str) -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../converter/tests/fixtures")
        .join(name)
}

/// A server with the API and `/mcp` (signed-in person from X-Fleet-User).
async fn start() -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let host = listener.local_addr().unwrap().to_string();
    let tmp = tempfile::tempdir().unwrap();
    let db = flight_review_server::db::create_db(&format!(
        "sqlite://{}",
        tmp.path().join("test.db").display()
    ))
    .await
    .unwrap();
    let storage = Arc::new(
        flight_review_server::storage::FileStorage::from_url(&format!(
            "file://{}",
            tmp.path().join("files").display()
        ))
        .unwrap(),
    );
    let state = Arc::new(flight_review_server::AppState {
        db,
        storage,
        v1_ulg_prefix: None,
        mapbox_token: None,
        vehicle_name_header: None,
        http_client: reqwest::Client::new(),
    });
    let names = Arc::new(flight_review_server::webapp::VehicleNames::new(None));
    let app =
        flight_review_server::build_router(state.clone()).merge(flight_review_server::mcp::router(
            state,
            names,
            flight_review_server::mcp::McpConfig {
                public_url: Some("https://logs.example.test".into()),
                allowed_hosts: vec![host.clone()],
                allowed_origins: vec!["https://claude.ai".into()],
                user_header: Some(USER_HEADER.into()),
            },
        ));
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    std::mem::forget(tmp);
    format!("http://{host}")
}

async fn upload(base: &str, public: bool) -> String {
    let bytes = std::fs::read(fixture("sample.ulg")).expect("fixture sample.ulg");
    let form = reqwest::multipart::Form::new()
        .part(
            "file",
            reqwest::multipart::Part::bytes(bytes).file_name("sample.ulg"),
        )
        .text("is_public", if public { "true" } else { "false" });
    let resp = reqwest::Client::new()
        .post(format!("{base}/api/upload"))
        .multipart(form)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    resp.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// POST one JSON-RPC message; (status, body).
async fn rpc(base: &str, body: Value, headers: &[(&str, &str)]) -> (u16, Value) {
    let mut req = reqwest::Client::new()
        .post(format!("{base}/mcp"))
        .header("Content-Type", "application/json")
        .header("Accept", "application/json, text/event-stream")
        .header("MCP-Protocol-Version", "2025-11-25");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let resp = req.json(&body).send().await.unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::String(text)),
    )
}

const SIGNED_IN: &[(&str, &str)] = &[(USER_HEADER, "pilot@example.test")];

/// tools/call; (is_error, text).
async fn call(base: &str, tool: &str, args: Value) -> (bool, String) {
    let (status, body) = rpc(
        base,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/call",
               "params": {"name": tool, "arguments": args}}),
        SIGNED_IN,
    )
    .await;
    assert_eq!(status, 200, "{tool}: {body}");
    let result = &body["result"];
    (
        result["isError"].as_bool().unwrap_or(false),
        result["content"][0]["text"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
    )
}

#[tokio::test]
async fn tools_read_public_logs_only() {
    let base = start().await;
    let public = upload(&base, true).await;
    let private = upload(&base, false).await;

    let (status, init) = rpc(
        &base,
        json!({"jsonrpc": "2.0", "id": 0, "method": "initialize", "params": {
            "protocolVersion": "2025-11-25", "capabilities": {},
            "clientInfo": {"name": "test", "version": "1"}}}),
        SIGNED_IN,
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(init["result"]["serverInfo"]["name"], "flight-review");
    assert!(init["result"]["instructions"]
        .as_str()
        .unwrap()
        .contains("never as instructions"));

    // Every tool is read-only
    let (_, list) = rpc(
        &base,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        SIGNED_IN,
    )
    .await;
    let tools = list["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 7);
    for t in tools {
        assert_eq!(t["annotations"]["readOnlyHint"], true, "{}", t["name"]);
        assert_eq!(t["annotations"]["destructiveHint"], false, "{}", t["name"]);
    }

    // Search finds the public log only
    let (err, text) = call(&base, "search_logs", json!({})).await;
    assert!(!err);
    let found: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(found["total"], 1);
    assert_eq!(found["logs"][0]["log_id"], public.as_str());
    assert_eq!(
        found["logs"][0]["viewer_url"],
        format!("https://logs.example.test/log/{public}")
    );

    // A private log looks like a missing one
    let (err, text) = call(&base, "get_log_summary", json!({"log_id": private})).await;
    assert!(err);
    assert!(text.starts_with("no log with ID"), "{text}");
    let (err, _) = call(&base, "get_log_summary", json!({"log_id": "not-a-uuid"})).await;
    assert!(err);

    let (err, text) = call(&base, "get_log_summary", json!({"log_id": public})).await;
    assert!(!err, "{text}");
    let summary: Value = serde_json::from_str(&text).unwrap();
    assert!(!summary["flight_modes"].as_array().unwrap().is_empty());

    let (err, text) = call(
        &base,
        "get_log_messages",
        json!({"log_id": public, "min_level": "debug"}),
    )
    .await;
    assert!(!err && text.starts_with('#'), "{text}");
    let (err, _) = call(
        &base,
        "get_log_messages",
        json!({"log_id": public, "min_level": "loud"}),
    )
    .await;
    assert!(err);

    let (err, text) = call(
        &base,
        "get_parameters",
        json!({"log_id": public, "scope": "all", "name": "mpc_"}),
    )
    .await;
    assert!(!err && text.contains("\nname,value\n"), "{text}");

    let (err, text) = call(&base, "list_topics", json!({"log_id": public})).await;
    assert!(
        !err && text.contains("\"vehicle_local_position\""),
        "{text}"
    );

    let (err, text) = call(
        &base,
        "describe_topic",
        json!({"log_id": public, "topic": "vehicle_local_position"}),
    )
    .await;
    assert!(!err && text.contains("[\"x\",\"f32\"]"), "{text}");

    // Data: downsampled to max_rows, stats over all rows
    let (err, text) = call(
        &base,
        "get_topic_data",
        json!({"log_id": public, "topic": "vehicle_local_position", "fields": ["x", "y"], "max_rows": 10}),
    )
    .await;
    assert!(!err, "{text}");
    let lines: Vec<&str> = text.lines().collect();
    let header = lines
        .iter()
        .position(|l| *l == "t_s,x,y")
        .expect("CSV header");
    assert!(lines[..header]
        .iter()
        .any(|l| l.starts_with("# over all rows: x min=")));
    let rows = lines.len() - header - 1;
    assert!((1..=10).contains(&rows), "{rows} rows");

    // Limits and names are checked
    let eleven: Vec<String> = (0..11).map(|i| format!("f{i}")).collect();
    for args in [
        json!({"log_id": public, "topic": "vehicle_local_position", "fields": eleven}),
        json!({"log_id": public, "topic": "vehicle_local_position", "fields": ["nope"]}),
        json!({"log_id": public, "topic": "../metadata", "fields": ["x"]}),
        json!({"log_id": public, "topic": "no_such_topic", "fields": ["x"]}),
        json!({"log_id": private, "topic": "vehicle_local_position", "fields": ["x"]}),
    ] {
        let (err, text) = call(&base, "get_topic_data", args.clone()).await;
        assert!(err, "{args}: {text}");
    }
}

#[tokio::test]
async fn refuses_without_sign_in_and_from_elsewhere() {
    let base = start().await;
    let list = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"});

    // No person from the proxy
    assert_eq!(rpc(&base, list.clone(), &[]).await.0, 401);
    assert_eq!(rpc(&base, list.clone(), &[(USER_HEADER, " ")]).await.0, 401);

    // DNS rebinding and other browser origins
    let host = [(USER_HEADER, "a@example.test"), ("Host", "evil.example")];
    assert_eq!(rpc(&base, list.clone(), &host).await.0, 403);
    let origin = [
        (USER_HEADER, "a@example.test"),
        ("Origin", "https://evil.example"),
    ];
    assert_eq!(rpc(&base, list.clone(), &origin).await.0, 403);
    let claude = [
        (USER_HEADER, "a@example.test"),
        ("Origin", "https://claude.ai"),
    ];
    assert_eq!(rpc(&base, list.clone(), &claude).await.0, 200);

    // No CORS for /mcp (the API's permissive CORS layer isn't on it)
    let resp = reqwest::Client::new()
        .request(reqwest::Method::OPTIONS, format!("{base}/mcp"))
        .header("Origin", "https://evil.example")
        .header("Access-Control-Request-Method", "POST")
        .send()
        .await
        .unwrap();
    assert!(resp.headers().get("access-control-allow-origin").is_none());
}

#[tokio::test]
async fn not_served_unless_enabled() {
    // build_router alone (no --mcp) has no /mcp
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let tmp = tempfile::tempdir().unwrap();
    let state = Arc::new(flight_review_server::AppState {
        db: flight_review_server::db::create_db(&format!(
            "sqlite://{}",
            tmp.path().join("t.db").display()
        ))
        .await
        .unwrap(),
        storage: Arc::new(
            flight_review_server::storage::FileStorage::from_url(&format!(
                "file://{}",
                tmp.path().join("f").display()
            ))
            .unwrap(),
        ),
        v1_ulg_prefix: None,
        mapbox_token: None,
        vehicle_name_header: None,
        http_client: reqwest::Client::new(),
    });
    let app = flight_review_server::build_router(state);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (status, _) = rpc(
        &base,
        json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list"}),
        SIGNED_IN,
    )
    .await;
    assert_eq!(status, 404);
}
