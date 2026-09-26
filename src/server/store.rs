//! `photo_data.json`: every located photo/video.
//!
//! Two backends, picked at startup:
//! * **GitHub** (`PHOTO_DATA_GITHUB_TOKEN` set — the deployed app): the file in the repo
//!   is the source of truth. Reads use conditional requests (cheap, and they
//!   pick up edits pushed from elsewhere); each saved annotation is a commit.
//! * **Local file** (development): `$PHOTO_DATA_PATH`, default
//!   `assets/photo_data.json`, read and rewritten in place. Also used read-only
//!   as the fallback if GitHub is unreachable before the first successful read.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine as _;
use dioxus::logger::tracing;
use serde::Deserialize;

use super::{env_or, github_repo, http};
use crate::data::{media_type_for, PhotoEntry};

pub fn store() -> &'static PhotoStore {
    static STORE: OnceLock<PhotoStore> = OnceLock::new();
    STORE.get_or_init(PhotoStore::from_env)
}

pub struct PhotoStore {
    local_path: PathBuf,
    github: Option<GitHubFile>,
    /// Last version read from / written to GitHub.
    cache: tokio::sync::Mutex<Option<Cached>>,
}

struct Cached {
    entries: Vec<PhotoEntry>,
    sha: String,
    etag: Option<String>,
}

impl PhotoStore {
    fn from_env() -> Self {
        let local_path = PathBuf::from(env_or("PHOTO_DATA_PATH", "assets/photo_data.json"));
        let github = std::env::var("PHOTO_DATA_GITHUB_TOKEN")
            .ok()
            .filter(|t| !t.is_empty())
            .map(|token| GitHubFile {
                repo: github_repo(),
                branch: env_or("GITHUB_BRANCH", "main"),
                path: "assets/photo_data.json".into(),
                token,
            });
        match &github {
            Some(gh) => tracing::info!("photo data: GitHub {}@{}:{}", gh.repo, gh.branch, gh.path),
            None => tracing::info!("photo data: local file {}", local_path.display()),
        }
        Self {
            local_path,
            github,
            cache: tokio::sync::Mutex::new(None),
        }
    }

    pub async fn load(&self) -> Result<Vec<PhotoEntry>, String> {
        let Some(gh) = &self.github else {
            return read_local(&self.local_path).await;
        };
        let mut cache = self.cache.lock().await;
        match refresh(gh, &mut cache).await {
            Ok(()) => {}
            Err(e) if cache.is_some() => {
                tracing::warn!("photo data refresh failed, serving cached: {e}")
            }
            Err(e) => {
                tracing::error!("photo data from GitHub failed, serving bundled copy: {e}");
                return read_local(&self.local_path).await;
            }
        }
        Ok(cache
            .as_ref()
            .map(|c| c.entries.clone())
            .unwrap_or_default())
    }

    pub async fn save(&self, filename: &str, lat: f64, lng: f64) -> Result<PhotoEntry, String> {
        let Some(gh) = &self.github else {
            let mut entries = read_local(&self.local_path).await?;
            let entry = upsert(&mut entries, filename, lat, lng);
            write_local(&self.local_path, &entries).await?;
            write_exif_gps(filename, lat, lng);
            return Ok(entry);
        };

        let mut cache = self.cache.lock().await;
        let message = format!("Annotate {filename} at {lat:.6}, {lng:.6}");
        // Retry once if someone else committed the file in between.
        for _ in 0..2 {
            refresh(gh, &mut cache).await?;
            let current = cache.as_mut().expect("refresh fills the cache");
            let mut entries = current.entries.clone();
            let entry = upsert(&mut entries, filename, lat, lng);
            match gh.put(&entries, &current.sha, &message).await? {
                Some(sha) => {
                    *current = Cached {
                        entries,
                        sha,
                        etag: None,
                    };
                    return Ok(entry);
                }
                None => current.etag = None, // conflict: force a full re-read
            }
        }
        Err("photo data changed concurrently on GitHub; try again".into())
    }
}

/// Set the location of `filename`, adding an entry if it has none yet.
fn upsert(entries: &mut Vec<PhotoEntry>, filename: &str, lat: f64, lng: f64) -> PhotoEntry {
    if let Some(existing) = entries.iter_mut().find(|e| e.filename == filename) {
        existing.lat = lat;
        existing.lng = lng;
        return existing.clone();
    }
    let entry = PhotoEntry {
        filename: filename.to_string(),
        path: format!("photos/{filename}"),
        lat,
        lng,
        timestamp: super::time::local_timestamp_from_filename(filename)
            .unwrap_or_else(|| "unknown".into()),
        media_type: media_type_for(filename),
    };
    entries.push(entry.clone());
    entry
}

fn parse(json: &str) -> Result<Vec<PhotoEntry>, String> {
    serde_json::from_str(json).map_err(|e| format!("parse photo_data.json: {e}"))
}

fn serialize(entries: &[PhotoEntry]) -> Result<String, String> {
    serde_json::to_string_pretty(entries).map_err(|e| e.to_string())
}

async fn read_local(path: &Path) -> Result<Vec<PhotoEntry>, String> {
    match tokio::fs::read_to_string(path).await {
        Ok(json) => parse(&json),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("read {}: {e}", path.display())),
    }
}

async fn write_local(path: &Path, entries: &[PhotoEntry]) -> Result<(), String> {
    tokio::fs::write(path, serialize(entries)?)
        .await
        .map_err(|e| format!("write {}: {e}", path.display()))
}

/// Best effort: also stamp the GPS into the original file's EXIF, when the
/// originals are on this machine (local editing only).
fn write_exif_gps(filename: &str, lat: f64, lng: f64) {
    let original = Path::new("Photos-3-001").join(filename);
    if !original.is_file() {
        return;
    }
    let _ = std::process::Command::new("python3")
        .arg("scripts/write_exif_gps.py")
        .arg(&original)
        .arg(lat.to_string())
        .arg(lng.to_string())
        .output();
}

/// Bring `cache` up to date with GitHub (no-op when the ETag still matches).
async fn refresh(gh: &GitHubFile, cache: &mut Option<Cached>) -> Result<(), String> {
    let etag = cache.as_ref().and_then(|c| c.etag.as_deref());
    if let Some(fresh) = gh.get(etag).await? {
        *cache = Some(fresh);
    }
    Ok(())
}

/// One file in a GitHub repo, via the REST contents API.
struct GitHubFile {
    repo: String,
    branch: String,
    path: String,
    token: String,
}

#[derive(Deserialize)]
struct ContentsResponse {
    sha: String,
    content: String,
    encoding: String,
}

#[derive(Deserialize)]
struct PutResponse {
    content: PutContent,
}

#[derive(Deserialize)]
struct PutContent {
    sha: String,
}

impl GitHubFile {
    fn url(&self) -> String {
        format!(
            "https://api.github.com/repos/{}/contents/{}",
            self.repo, self.path
        )
    }

    fn request(&self, method: reqwest::Method) -> reqwest::RequestBuilder {
        http()
            .request(method, self.url())
            .bearer_auth(&self.token)
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
    }

    /// Fetch the file; `None` if it is unchanged since `etag`.
    async fn get(&self, etag: Option<&str>) -> Result<Option<Cached>, String> {
        let mut req = self
            .request(reqwest::Method::GET)
            .query(&[("ref", &self.branch)]);
        if let Some(etag) = etag {
            req = req.header("If-None-Match", etag);
        }
        let resp = req
            .send()
            .await
            .map_err(|e| format!("GET {}: {e}", self.url()))?;
        if resp.status() == reqwest::StatusCode::NOT_MODIFIED {
            return Ok(None);
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("GET {}: HTTP {status}: {body}", self.url()));
        }
        let etag = resp
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(String::from);
        let body: ContentsResponse = resp.json().await.map_err(|e| e.to_string())?;
        if body.encoding != "base64" {
            return Err(format!("unexpected contents encoding {:?}", body.encoding));
        }
        let packed: String = body.content.split_whitespace().collect();
        let bytes = BASE64.decode(packed).map_err(|e| e.to_string())?;
        let json = String::from_utf8(bytes).map_err(|e| e.to_string())?;
        Ok(Some(Cached {
            entries: parse(&json)?,
            sha: body.sha,
            etag,
        }))
    }

    /// Commit `entries` on top of blob `sha`. Returns the new blob sha, or
    /// `None` if the file changed on GitHub since `sha` was read.
    async fn put(
        &self,
        entries: &[PhotoEntry],
        sha: &str,
        message: &str,
    ) -> Result<Option<String>, String> {
        let body = serde_json::json!({
            "message": message,
            "content": BASE64.encode(serialize(entries)?),
            "sha": sha,
            "branch": self.branch,
        });
        let resp = self
            .request(reqwest::Method::PUT)
            .json(&body)
            .send()
            .await
            .map_err(|e| format!("PUT {}: {e}", self.url()))?;
        match resp.status() {
            s if s.is_success() => {
                let body: PutResponse = resp.json().await.map_err(|e| e.to_string())?;
                Ok(Some(body.content.sha))
            }
            reqwest::StatusCode::CONFLICT => Ok(None),
            status => {
                let body = resp.text().await.unwrap_or_default();
                Err(format!("PUT {}: HTTP {status}: {body}", self.url()))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_moves_existing_entry() {
        let mut entries = Vec::new();
        let first = upsert(&mut entries, "PXL_20260619_114518677.jpg", 1.0, 2.0);
        assert_eq!(first.timestamp, "2026:06:19 14:45:18");
        assert_eq!(first.media_type, "image/jpeg");
        upsert(&mut entries, "PXL_20260619_114518677.jpg", 3.0, 4.0);
        assert_eq!(entries.len(), 1);
        assert_eq!((entries[0].lat, entries[0].lng), (3.0, 4.0));
    }

    #[test]
    fn upsert_marks_videos() {
        let mut entries = Vec::new();
        let v = upsert(&mut entries, "PXL_20260620_101402649.LS.mp4", 1.0, 2.0);
        assert_eq!(v.media_type, "video/mp4");
        assert_eq!(v.path, "photos/PXL_20260620_101402649.LS.mp4");
    }

    #[test]
    fn bundled_photo_data_parses_and_round_trips() {
        let json = std::fs::read_to_string("assets/photo_data.json").unwrap();
        let entries = parse(&json).unwrap();
        assert!(!entries.is_empty());
        assert_eq!(serialize(&entries).unwrap(), json);
    }
}
