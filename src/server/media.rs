//! `/photos/<file>` and `/videos/<file>`.
//!
//! Lookup order for each request:
//!   1. Local directories (development): `$MEDIA_DIR/{photos,videos}`
//!      (default `dist-media/`, the output of `scripts/publish_media.sh`),
//!      then the originals in `Photos-3-001/` / legacy transcodes in `media-web/`.
//!   2. The on-disk cache (`$MEDIA_CACHE_DIR`).
//!   3. GitHub Releases: the release tagged `photos` or `videos` in
//!      `$GITHUB_REPO`, downloaded once into the cache. (`$MEDIA_BASE_URL`
//!      overrides the `https://github.com/<repo>/releases/download` prefix.)
//!
//! Serving through the app (rather than linking the release URLs directly)
//! gives same-origin URLs with real content types, HTTP range support for
//! video seeking, and long cache lifetimes — GitHub's release downloads are
//! `application/octet-stream` behind short-lived signed redirects.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use dioxus::server::axum::{
    body::Body,
    extract::{Path as UrlPath, Request},
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use tokio::io::AsyncWriteExt;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use super::{env_or, github_repo, http};

#[derive(Clone, Copy)]
enum Kind {
    Photos,
    Videos,
}

impl Kind {
    /// URL segment, local subdirectory, and GitHub release tag — all the same.
    fn name(self) -> &'static str {
        match self {
            Kind::Photos => "photos",
            Kind::Videos => "videos",
        }
    }

    fn local_dirs(self) -> Vec<PathBuf> {
        let media = PathBuf::from(env_or("MEDIA_DIR", "dist-media")).join(self.name());
        let fallback = match self {
            Kind::Photos => "Photos-3-001",
            Kind::Videos => "media-web",
        };
        vec![media, PathBuf::from(fallback)]
    }
}

pub fn router() -> Router {
    Router::new()
        .route(
            "/photos/{file}",
            get(|UrlPath(file): UrlPath<String>, req: Request| serve(Kind::Photos, file, req)),
        )
        .route(
            "/videos/{file}",
            get(|UrlPath(file): UrlPath<String>, req: Request| serve(Kind::Videos, file, req)),
        )
}

/// Plain media filenames only: no separators, no leading dot, so a request
/// can never escape the media directories or address another release.
pub fn is_safe_filename(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 255
        && !name.starts_with('.')
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

async fn serve(kind: Kind, file: String, req: Request) -> Response {
    if !is_safe_filename(&file) {
        return StatusCode::NOT_FOUND.into_response();
    }

    if let Some(local) = kind
        .local_dirs()
        .into_iter()
        .map(|d| d.join(&file))
        .find(|p| p.is_file())
    {
        return serve_file(&local, req, "no-cache").await;
    }

    let cached = cache_dir().join(kind.name()).join(&file);
    if let Err(err) = ensure_cached(kind, &file, &cached).await {
        return err.into_response();
    }
    // Derivatives are published once and not edited in place; a week is safe.
    serve_file(&cached, req, "public, max-age=604800").await
}

async fn serve_file(path: &Path, req: Request, cache_control: &'static str) -> Response {
    match ServeFile::new(path).oneshot(req).await {
        Ok(res) => {
            let mut res = res.map(Body::new);
            res.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static(cache_control),
            );
            res
        }
        Err(err) => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()).into_response(),
    }
}

fn cache_dir() -> PathBuf {
    std::env::var("MEDIA_CACHE_DIR")
        .ok()
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("samothraki-media"))
}

enum FetchError {
    NotFound,
    Failed(String),
}

impl IntoResponse for FetchError {
    fn into_response(self) -> Response {
        match self {
            FetchError::NotFound => StatusCode::NOT_FOUND.into_response(),
            FetchError::Failed(msg) => {
                dioxus::logger::tracing::error!("media fetch failed: {msg}");
                (StatusCode::BAD_GATEWAY, msg).into_response()
            }
        }
    }
}

/// Download `file` from the GitHub release into `dest` unless it is already
/// there. Concurrent requests for the same file (a browser opens several
/// range requests for one video) wait for a single download.
async fn ensure_cached(kind: Kind, file: &str, dest: &Path) -> Result<(), FetchError> {
    if dest.is_file() {
        return Ok(());
    }
    let lock = {
        static INFLIGHT: OnceLock<Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>> =
            OnceLock::new();
        let mut map = INFLIGHT.get_or_init(Default::default).lock().unwrap();
        map.entry(dest.to_path_buf()).or_default().clone()
    };
    let _guard = lock.lock().await;
    if dest.is_file() {
        return Ok(());
    }

    let base = std::env::var("MEDIA_BASE_URL")
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| format!("https://github.com/{}/releases/download", github_repo()));
    let url = format!("{}/{}/{file}", base.trim_end_matches('/'), kind.name());
    let mut resp = http()
        .get(&url)
        .send()
        .await
        .map_err(|e| FetchError::Failed(format!("GET {url}: {e}")))?;
    if resp.status() == reqwest::StatusCode::NOT_FOUND {
        return Err(FetchError::NotFound);
    }
    if !resp.status().is_success() {
        return Err(FetchError::Failed(format!(
            "GET {url}: HTTP {}",
            resp.status()
        )));
    }

    let dir = dest.parent().expect("cache path has a parent");
    tokio::fs::create_dir_all(dir)
        .await
        .map_err(|e| FetchError::Failed(format!("create {}: {e}", dir.display())))?;
    // Write to a temp name and rename, so a half-written file is never served.
    let tmp = dest.with_file_name(format!("{file}.part"));
    let result: Result<(), String> = async {
        let mut out = tokio::fs::File::create(&tmp)
            .await
            .map_err(|e| e.to_string())?;
        while let Some(chunk) = resp.chunk().await.map_err(|e| e.to_string())? {
            out.write_all(&chunk).await.map_err(|e| e.to_string())?;
        }
        out.flush().await.map_err(|e| e.to_string())?;
        tokio::fs::rename(&tmp, dest)
            .await
            .map_err(|e| e.to_string())
    }
    .await;
    if let Err(e) = result {
        let _ = tokio::fs::remove_file(&tmp).await;
        return Err(FetchError::Failed(format!("download {url}: {e}")));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::is_safe_filename;

    #[test]
    fn accepts_media_names() {
        assert!(is_safe_filename("PXL_20260619_052531747.MP.jpg"));
        assert!(is_safe_filename("PXL_20260620_101402649.LS.mp4"));
    }

    #[test]
    fn rejects_traversal_and_odd_names() {
        for bad in [
            "", "..", ".env", "../x.jpg", "a/b.jpg", "a\\b.jpg", "a b.jpg", "a%2Fb",
        ] {
            assert!(!is_safe_filename(bad), "{bad:?} should be rejected");
        }
    }
}
