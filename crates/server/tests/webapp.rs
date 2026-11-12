//! FireDrone: the web app router (`webapp::router`): nav links, vehicle
//! names, the static files with their fallback to index.html, and the cache
//! headers.

use flight_review_server::webapp::{self, NavLink};

async fn serve(app: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    base
}

async fn start(static_dir: &std::path::Path, links: Vec<NavLink>) -> String {
    serve(webapp::router(Some(static_dir), links, None)).await
}

fn cache_control(res: &reqwest::Response) -> &str {
    res.headers()
        .get("cache-control")
        .map(|v| v.to_str().unwrap())
        .unwrap_or("")
}

#[tokio::test]
async fn test_web_app_files_and_nav_links() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), "<!doctype html>app").unwrap();
    std::fs::write(dir.path().join("favicon.svg"), "<svg/>").unwrap();
    let immutable = dir.path().join("_app/immutable");
    std::fs::create_dir_all(&immutable).unwrap();
    std::fs::write(immutable.join("start.abc123.js"), "export {}").unwrap();

    let links = vec!["Fleet=https://fleet.example.com".parse().unwrap()];
    let base = start(dir.path(), links).await;
    let get = |path: &str| reqwest::get(format!("{base}{path}"));

    let res = get("/api/nav-links").await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(cache_control(&res), "no-cache");
    let body: serde_json::Value = res.json().await.unwrap();
    assert_eq!(
        body,
        serde_json::json!([{"name": "Fleet", "url": "https://fleet.example.com"}])
    );

    // Client-side route: index.html, revalidated on every load.
    let res = get("/browse").await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(cache_control(&res), "no-cache");
    assert_eq!(res.text().await.unwrap(), "<!doctype html>app");

    let res = get("/favicon.svg").await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(cache_control(&res), "no-cache");

    // Hashed build output: cached for good.
    let res = get("/_app/immutable/start.abc123.js").await.unwrap();
    assert_eq!(res.status(), 200);
    assert_eq!(cache_control(&res), "public, max-age=31536000, immutable");
    assert_eq!(res.text().await.unwrap(), "export {}");

    // A missing asset (e.g. from an older version) is a 404, not a cached index.html.
    let res = get("/_app/immutable/start.old456.js").await.unwrap();
    assert_eq!(res.status(), 404);
    assert_eq!(cache_control(&res), "no-cache");
}

#[tokio::test]
async fn test_no_nav_links() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("index.html"), "app").unwrap();
    let base = start(dir.path(), vec![]).await;
    let body: serde_json::Value = reqwest::get(format!("{base}/api/nav-links"))
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(body, serde_json::json!([]));
}

#[tokio::test]
async fn test_vehicle_names() {
    // stands in for FireDrone Fleet's /api/drone-names
    let fleet = serve(axum::Router::new().route(
        "/api/drone-names",
        axum::routing::get(|| async {
            axum::Json(serde_json::json!({"fd-0003": "Dev Drone 1", "fd-0004": 7}))
        }),
    ))
    .await;
    let names = |app| async {
        let base = serve(app).await;
        reqwest::get(format!("{base}/api/vehicle-names"))
            .await
            .unwrap()
            .json::<serde_json::Value>()
            .await
            .unwrap()
    };

    let app = webapp::router(None, vec![], Some(format!("{fleet}/api/drone-names")));
    // text names only
    assert_eq!(
        names(app).await,
        serde_json::json!({"fd-0003": "Dev Drone 1"})
    );
    // not configured, or fleet not answering: no names, the page shows IDs
    assert_eq!(
        names(webapp::router(None, vec![], None)).await,
        serde_json::json!({})
    );
    let gone = webapp::router(None, vec![], Some(format!("{fleet}/nothing-here")));
    assert_eq!(names(gone).await, serde_json::json!({}));
}
