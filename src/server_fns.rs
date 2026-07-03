use std::collections::HashMap;

use dioxus::prelude::*;

use crate::data::PhotoEntry;

// These items are only referenced from `#[server]` fn bodies (and the server
// router), which are stripped on the wasm client — gate them to `server` so the
// client doesn't see them as dead code.
#[cfg(feature = "server")]
const TAGS_PATH: &str = "assets/photo_tags.json";
#[cfg(feature = "server")]
const PHOTO_DATA_PATH: &str = "assets/photo_data.json";

/// Original photo directory. Served directly at `/photos/<file>` by the axum
/// router in `main.rs`, so photos are never copied or duplicated into assets.
#[cfg(feature = "server")]
pub const PHOTOS_SRC_DIR: &str = "Photos-3-001";

/// Web-playable (H.264) video transcodes, served at `/media/<file>`. Produced
/// by scripts/transcode_videos.sh from the HEVC originals (browsers can't play
/// HEVC in <video>). Gitignored derived artifacts.
#[cfg(feature = "server")]
pub const MEDIA_WEB_DIR: &str = "media-web";

/// Client-facing path for a photo, matching the `/photos` static route.
#[cfg(feature = "server")]
fn photo_path(filename: &str) -> String {
    format!("photos/{}", filename)
}

#[cfg(feature = "server")]
fn default_media_type() -> String {
    "image/jpeg".into()
}

#[cfg(feature = "server")]
fn timestamp_from_filename(filename: &str) -> String {
    // PXL_YYYYMMDD_HHMMSSxxx.jpg -> "YYYY:MM:DD HH:MM:SS"
    if let Some(rest) = filename.strip_prefix("PXL_") {
        if rest.len() >= 15 {
            let date = &rest[..8];
            let time = &rest[9..15];
            return format!(
                "{}:{}:{} {}:{}:{}",
                &date[..4],
                &date[4..6],
                &date[6..8],
                &time[..2],
                &time[2..4],
                &time[4..6]
            );
        }
    }
    "unknown".to_string()
}

#[server(endpoint = "photos/load")]
pub async fn load_photo_data() -> Result<Vec<PhotoEntry>, ServerFnError> {
    let content = tokio::fs::read_to_string(PHOTO_DATA_PATH)
        .await
        .unwrap_or_else(|_| "[]".to_string());
    let entries: Vec<PhotoEntry> = serde_json::from_str(&content).unwrap_or_default();
    Ok(entries)
}

#[server(endpoint = "tags/load")]
pub async fn load_tags() -> Result<HashMap<String, Vec<String>>, ServerFnError> {
    let content = tokio::fs::read_to_string(TAGS_PATH)
        .await
        .unwrap_or_else(|_| "{}".to_string());
    let tags: HashMap<String, Vec<String>> = serde_json::from_str(&content).unwrap_or_default();
    Ok(tags)
}

#[server(endpoint = "tags/save")]
pub async fn save_tags(tags: HashMap<String, Vec<String>>) -> Result<(), ServerFnError> {
    let json = serde_json::to_string_pretty(&tags)?;
    tokio::fs::write(TAGS_PATH, json)
        .await
        .map_err(|e| ServerFnError::ServerError {
            message: e.to_string(),
            code: 500,
            details: None,
        })?;
    Ok(())
}

#[server(endpoint = "annotations/save")]
pub async fn save_annotation(
    filename: String,
    lat: f64,
    lng: f64,
) -> Result<PhotoEntry, ServerFnError> {
    // Read existing photo data
    let content = tokio::fs::read_to_string(PHOTO_DATA_PATH)
        .await
        .unwrap_or_else(|_| "[]".to_string());
    let mut entries: Vec<PhotoEntry> = serde_json::from_str(&content).unwrap_or_default();

    // Update an existing entry or create one. The photo is served directly from
    // PHOTOS_SRC_DIR, so there is nothing to copy.
    let entry = if let Some(existing) = entries.iter_mut().find(|e| e.filename == filename) {
        existing.lat = lat;
        existing.lng = lng;
        existing.clone()
    } else {
        let new_entry = PhotoEntry {
            path: photo_path(&filename),
            timestamp: timestamp_from_filename(&filename),
            filename: filename.clone(),
            lat,
            lng,
            media_type: default_media_type(),
        };
        entries.push(new_entry.clone());
        new_entry
    };

    // Write back photo_data.json
    let json = serde_json::to_string_pretty(&entries)?;
    tokio::fs::write(PHOTO_DATA_PATH, json)
        .await
        .map_err(|e| ServerFnError::ServerError {
            message: e.to_string(),
            code: 500,
            details: None,
        })?;

    // Try to write EXIF GPS back to the original file (best-effort)
    let src_path = format!("{}/{}", PHOTOS_SRC_DIR, filename);
    let _ = std::process::Command::new("python3")
        .args([
            "scripts/write_exif_gps.py",
            &src_path,
            &lat.to_string(),
            &lng.to_string(),
        ])
        .output();

    Ok(entry)
}
