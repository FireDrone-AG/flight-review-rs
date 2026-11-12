//! FireDrone: serving the built web app (`--static-dir`) and the links to
//! other tools that its navigation shows (`--nav-link`).

use axum::{
    extract::Request,
    http::{header, HeaderValue},
    middleware::{self, Next},
    response::Response,
    routing::get,
    Json, Router,
};
use serde::Serialize;
use std::path::Path;
use tower_http::services::{ServeDir, ServeFile};

/// A link to another tool, shown in the web app's navigation.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct NavLink {
    pub name: String,
    pub url: String,
}

impl std::str::FromStr for NavLink {
    type Err = String;

    /// `Name=https://host/path`
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (name, url) = s
            .split_once('=')
            .ok_or_else(|| format!("expected NAME=URL, got {s:?}"))?;
        let (name, url) = (name.trim(), url.trim());
        if name.is_empty() {
            return Err(format!("no name before '=' in {s:?}"));
        }
        // Only web links: the URL ends up in an href.
        if !(url.starts_with("https://") || url.starts_with("http://")) {
            return Err(format!(
                "{name}: the URL must start with https:// or http://"
            ));
        }
        Ok(NavLink {
            name: name.to_string(),
            url: url.to_string(),
        })
    }
}

/// `GET /api/nav-links`, plus the web app from `static_dir` for every other
/// path that no API route takes. Merge it into [`crate::build_router`]'s router.
pub fn router(static_dir: Option<&Path>, nav_links: Vec<NavLink>) -> Router {
    let mut app = Router::new().route(
        "/api/nav-links",
        get(move || std::future::ready(Json(nav_links.clone()))),
    );
    if let Some(dir) = static_dir {
        // Hashed build output: a missing file is a 404, not index.html, so
        // that no page gets cached under an asset's name.
        let immutable = dir.join("_app").join("immutable");
        // Paths that are neither an API route nor a file get index.html, so
        // client-side routes work on reload.
        let index = ServeFile::new(dir.join("index.html"));
        app = app
            .nest_service("/_app/immutable", ServeDir::new(immutable))
            .fallback_service(ServeDir::new(dir).fallback(index));
    }
    app.layer(middleware::from_fn(cache_control))
}

/// Files under `/_app/immutable/` have the content hash in their name and can
/// be cached for good. Everything else (index.html above all) is revalidated
/// on each load, so a new version shows up without a forced reload.
async fn cache_control(req: Request, next: Next) -> Response {
    let immutable = req.uri().path().starts_with("/_app/immutable/");
    let mut res = next.run(req).await;
    let value = if immutable && res.status().is_success() {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(value));
    res
}

#[cfg(test)]
mod tests {
    use super::NavLink;

    #[test]
    fn parses_nav_links() {
        assert_eq!(
            " Fleet = https://fleet.example.com/drones ".parse::<NavLink>(),
            Ok(NavLink {
                name: "Fleet".into(),
                url: "https://fleet.example.com/drones".into()
            })
        );
        // '=' in the URL stays in the URL
        assert_eq!(
            "Docs=https://example.com/?a=b"
                .parse::<NavLink>()
                .unwrap()
                .url,
            "https://example.com/?a=b"
        );
        assert!("https://fleet.example.com".parse::<NavLink>().is_err());
        assert!("=https://fleet.example.com".parse::<NavLink>().is_err());
        assert!("Fleet=javascript:alert(1)".parse::<NavLink>().is_err());
        assert!("Fleet=fleet.example.com".parse::<NavLink>().is_err());
    }
}
