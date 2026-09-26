use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

/// One photo/video on the map: the record stored in `assets/photo_data.json`
/// and returned by the `load_photo_data` server function.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhotoEntry {
    pub filename: String,
    /// Stable id for the map feature (`photos/<filename>`). Not a URL: media
    /// URLs are derived from `filename` by [`photo_url`] / [`video_url`].
    pub path: String,
    pub lat: f64,
    pub lng: f64,
    /// Trip-local capture time, `YYYY:MM:DD HH:MM:SS` (EXIF style, so it sorts
    /// chronologically as a string).
    pub timestamp: String,
    #[serde(default = "default_media_type")]
    pub media_type: String,
}

impl PhotoEntry {
    pub fn is_video(&self) -> bool {
        self.media_type.starts_with("video/")
    }

    /// URL of the browser-displayable asset for this entry.
    pub fn media_url(&self) -> String {
        if self.is_video() {
            video_url(&self.filename)
        } else {
            photo_url(&self.filename)
        }
    }
}

/// Whether a media filename is a video (by extension).
pub fn is_video_file(filename: &str) -> bool {
    let lower = filename.to_ascii_lowercase();
    [".mp4", ".mov", ".webm", ".avi"]
        .iter()
        .any(|ext| lower.ends_with(ext))
}

/// Web-sized JPEG for a photo. Served by the app's own server (see
/// `server::media`), which reads it from local disk or GitHub Releases.
pub fn photo_url(filename: &str) -> String {
    format!("/photos/{filename}")
}

/// Browser-playable H.264 transcode of a video (the originals are HEVC).
/// Transcodes are always `.mp4`, whatever the original's extension.
pub fn video_url(filename: &str) -> String {
    let stem = filename.rsplit_once('.').map_or(filename, |(stem, _)| stem);
    format!("/videos/{stem}.mp4")
}

fn default_media_type() -> String {
    "image/jpeg".into()
}

/// MIME type for a media file, by extension (used when the server creates an
/// entry for a newly annotated file).
#[cfg_attr(not(feature = "server"), allow(dead_code))]
pub fn media_type_for(filename: &str) -> String {
    if is_video_file(filename) {
        "video/mp4".into()
    } else {
        default_media_type()
    }
}

fn compute_title(filename: &str) -> String {
    // Strip the extension (last .ext)
    if let Some(dot) = filename.rfind('.') {
        filename[..dot].to_string()
    } else {
        filename.to_string()
    }
}

pub fn photos_to_geojson(photos: &[PhotoEntry]) -> Result<JsValue, JsValue> {
    let features = js_sys::Array::new();
    for photo in photos {
        let feature = js_sys::Object::new();
        js_sys::Reflect::set(&feature, &"type".into(), &"Feature".into())?;

        let geometry = js_sys::Object::new();
        js_sys::Reflect::set(&geometry, &"type".into(), &"Point".into())?;
        let coords = js_sys::Array::new();
        coords.push(&JsValue::from_f64(photo.lng));
        coords.push(&JsValue::from_f64(photo.lat));
        js_sys::Reflect::set(&geometry, &"coordinates".into(), &coords)?;
        js_sys::Reflect::set(&feature, &"geometry".into(), &geometry)?;

        js_sys::Reflect::set(&feature, &"id".into(), &JsValue::from_str(&photo.path))?;

        let props = js_sys::Object::new();
        js_sys::Reflect::set(
            &props,
            &"filename".into(),
            &JsValue::from_str(&photo.filename),
        )?;
        js_sys::Reflect::set(&props, &"path".into(), &JsValue::from_str(&photo.path))?;
        js_sys::Reflect::set(
            &props,
            &"timestamp".into(),
            &JsValue::from_str(&photo.timestamp),
        )?;
        let title = compute_title(&photo.filename);
        js_sys::Reflect::set(&props, &"title".into(), &JsValue::from_str(&title))?;

        js_sys::Reflect::set(&feature, &"properties".into(), &props)?;

        features.push(&feature);
    }

    let collection = js_sys::Object::new();
    js_sys::Reflect::set(&collection, &"type".into(), &"FeatureCollection".into())?;
    js_sys::Reflect::set(&collection, &"features".into(), &features)?;

    Ok(collection.into())
}

pub fn calculate_center(photos: &[PhotoEntry]) -> [f64; 2] {
    if photos.is_empty() {
        return [25.513, 40.485];
    }
    let lat_sum: f64 = photos.iter().map(|p| p.lat).sum();
    let lng_sum: f64 = photos.iter().map(|p| p.lng).sum();
    let n = photos.len() as f64;
    [lng_sum / n, lat_sum / n]
}
