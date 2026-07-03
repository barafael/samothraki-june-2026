use std::collections::HashMap;

use dioxus::prelude::*;

use crate::data::PhotoEntry;

const TAGS_PATH: &str = "assets/photo_tags.json";
const PHOTO_DATA_PATH: &str = "assets/photo_data.json";
const PHOTOS_SRC_DIR: &str = "Photos-3-001";
const PHOTOS_DST_DIR: &str = "assets/photos";
const PHOTOS_OUTPUT_DIR: &str = "target/dx/my-holiday/debug/web/public/assets/photos";

fn default_media_type() -> String {
    "image/jpeg".into()
}

fn timestamp_from_filename(filename: &str) -> String {
    // PXL_YYYYMMDD_HHMMSSxxx.jpg -> "YYYY:MM:DD HH:MM:SS"
    if let Some(rest) = filename.strip_prefix("PXL_") {
        if rest.len() >= 15 {
            let date = &rest[..8];
            let time = &rest[9..15];
            return format!("{}:{}:{} {}:{}:{}",
                &date[..4], &date[4..6], &date[6..8],
                &time[..2], &time[2..4], &time[4..6]);
        }
    }
    "unknown".to_string()
}

#[server(endpoint = "/api/tags/load")]
pub async fn load_tags() -> Result<HashMap<String, Vec<String>>, ServerFnError> {
    let content = tokio::fs::read_to_string(TAGS_PATH)
        .await
        .unwrap_or_else(|_| "{}".to_string());
    let tags: HashMap<String, Vec<String>> = serde_json::from_str(&content).unwrap_or_default();
    Ok(tags)
}

#[server(endpoint = "/api/tags/save")]
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

#[server(endpoint = "/api/annotations/save")]
pub async fn save_annotation(
    filename: String,
    lat: f64,
    lng: f64,
) -> Result<PhotoEntry, ServerFnError> {
    // Copy file from source to assets/photos if not already there
    let src_path = format!("{}/{}", PHOTOS_SRC_DIR, filename);
    let dst_path = format!("{}/{}", PHOTOS_DST_DIR, filename);

    if !std::path::Path::new(&dst_path).exists() {
        tokio::fs::copy(&src_path, &dst_path)
            .await
            .map_err(|e| ServerFnError::ServerError {
                message: format!("Failed to copy {}: {}", filename, e),
                code: 500,
                details: None,
            })?;
    }

    // Also copy to dev server's output directory
    let output_photo = format!("{}/{}", PHOTOS_OUTPUT_DIR, filename);
    if !std::path::Path::new(&output_photo).exists() {
        let _ = tokio::fs::copy(&src_path, &output_photo).await;
    }

    // Read existing photo data
    let content = tokio::fs::read_to_string(PHOTO_DATA_PATH)
        .await
        .unwrap_or_else(|_| "[]".to_string());
    let mut entries: Vec<PhotoEntry> = serde_json::from_str(&content).unwrap_or_default();

    // Check if entry exists
    let entry = if let Some(existing) = entries.iter_mut().find(|e| e.filename == filename) {
        existing.lat = lat;
        existing.lng = lng;
        existing.clone()
    } else {
        let ts = timestamp_from_filename(&filename);
        let new_entry = PhotoEntry {
            filename: filename.clone(),
            path: dst_path.clone(),
            lat,
            lng,
            timestamp: ts,
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

    // Try to write EXIF GPS (best-effort)
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

#[server(endpoint = "/api/annotations/ensure-copied")]
pub async fn ensure_photo_copied(filename: String) -> Result<String, ServerFnError> {
    let src_path = format!("{}/{}", PHOTOS_SRC_DIR, filename);
    let dst_path = format!("{}/{}", PHOTOS_DST_DIR, filename);

    // Copy to project's assets/photos
    if !std::path::Path::new(&dst_path).exists() {
        tokio::fs::copy(&src_path, &dst_path)
            .await
            .map_err(|e| ServerFnError::ServerError {
                message: format!("Failed to copy {}: {}", filename, e),
                code: 500,
                details: None,
            })?;
    }

    // Also copy to dev server's output directory
    let output_path = format!("{}/{}", PHOTOS_OUTPUT_DIR, filename);
    if !std::path::Path::new(&output_path).exists() {
        let _ = tokio::fs::copy(&src_path, &output_path).await;
    }

    Ok(dst_path)
}
