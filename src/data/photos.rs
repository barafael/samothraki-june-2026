use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PhotoEntry {
    pub filename: String,
    pub path: String,
    pub lat: f64,
    pub lng: f64,
    pub timestamp: String,
    #[serde(default = "default_media_type")]
    pub media_type: String,
}

fn default_media_type() -> String {
    "image/jpeg".into()
}

fn compute_title(filename: &str) -> String {
    // Strip the extension (last .ext)
    if let Some(dot) = filename.rfind('.') {
        filename[..dot].to_string()
    } else {
        filename.to_string()
    }
}

pub fn photos_to_geojson(
    photos: &[PhotoEntry],
    tags: &std::collections::HashMap<String, Vec<String>>,
) -> Result<JsValue, JsValue> {
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

        let tags_arr = js_sys::Array::new();
        if let Some(photo_tags) = tags.get(&photo.path) {
            for t in photo_tags {
                tags_arr.push(&JsValue::from_str(t));
            }
        }
        js_sys::Reflect::set(&props, &"tags".into(), &tags_arr)?;
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
