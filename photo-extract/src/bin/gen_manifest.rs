//! Build step for the static viewer.
//!
//! Reads the curated `assets/photo_data.json` (the editor's output, including
//! manual GPS annotations), generates WebP thumbnails into `thumbs/` for image
//! entries, and writes `manifest.json` — the single data file the viewer fetches
//! at runtime. Photos and videos are served from R2; only thumbnails are derived
//! here.
//!
//! Idempotent: a thumbnail is regenerated only when it's missing or older than
//! its source image, so re-runs are fast.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use image::imageops::FilterType;
use photo_extract::{PhotoEntry, PHOTOS_SRC_DIR};

/// Longest-edge size for generated thumbnails.
const THUMB_MAX_EDGE: u32 = 400;
const PHOTO_DATA_PATH: &str = "assets/photo_data.json";
const MANIFEST_PATH: &str = "manifest.json";
const THUMBS_DIR: &str = "thumbs";

fn mtime(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// Returns true if `dst` exists and is at least as new as `src`.
fn up_to_date(src: &Path, dst: &Path) -> bool {
    match (mtime(src), mtime(dst)) {
        (Some(s), Some(d)) => d >= s,
        _ => false,
    }
}

/// Generate a WebP thumbnail for `src` at `dst`, longest edge <= THUMB_MAX_EDGE.
fn make_thumbnail(src: &Path, dst: &Path) -> Result<(), image::ImageError> {
    let img = image::open(src)?;
    let thumb = img.resize(THUMB_MAX_EDGE, THUMB_MAX_EDGE, FilterType::Lanczos3);
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(image::ImageError::IoError)?;
    }
    thumb.save(dst)?;
    Ok(())
}

fn main() {
    let content = fs::read_to_string(PHOTO_DATA_PATH)
        .unwrap_or_else(|e| panic!("read {PHOTO_DATA_PATH}: {e}"));
    let mut entries: Vec<PhotoEntry> =
        serde_json::from_str(&content).expect("parse photo_data.json");

    fs::create_dir_all(THUMBS_DIR).expect("create thumbs dir");

    let mut made = 0usize;
    let mut skipped = 0usize;
    let mut no_thumb = 0usize;

    for entry in &mut entries {
        // Only images get thumbnails; videos keep an empty thumb and the viewer
        // falls back to a poster/full-res path.
        if !entry.media_type.starts_with("image/") {
            entry.thumb = String::new();
            no_thumb += 1;
            continue;
        }

        let src = PathBuf::from(PHOTOS_SRC_DIR).join(&entry.filename);
        if !src.is_file() {
            eprintln!(" MISS {} (source not found, no thumb)", entry.filename);
            entry.thumb = String::new();
            no_thumb += 1;
            continue;
        }

        let stem = Path::new(&entry.filename)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or(&entry.filename);
        let thumb_rel = format!("{THUMBS_DIR}/{stem}.webp");
        let dst = PathBuf::from(&thumb_rel);

        if up_to_date(&src, &dst) {
            skipped += 1;
        } else {
            match make_thumbnail(&src, &dst) {
                Ok(()) => {
                    made += 1;
                    eprintln!("  OK  {thumb_rel}");
                }
                Err(e) => {
                    eprintln!(" FAIL {} ({e})", entry.filename);
                    entry.thumb = String::new();
                    no_thumb += 1;
                    continue;
                }
            }
        }
        entry.thumb = thumb_rel;
    }

    let json = serde_json::to_string_pretty(&entries).expect("serialize manifest");
    fs::write(MANIFEST_PATH, &json).expect("write manifest.json");

    eprintln!(
        "\n{} entries -> {MANIFEST_PATH}  (thumbs: {made} generated, {skipped} up-to-date, {no_thumb} none)",
        entries.len()
    );
}
