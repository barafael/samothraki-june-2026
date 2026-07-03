use serde::Serialize;
use wasm_bindgen::JsValue;

#[derive(Debug, Serialize)]
pub struct GeoJsonSource {
    #[serde(rename = "type")]
    pub source_type: &'static str,
    pub data: FeatureCollection,
}

#[derive(Debug, Serialize)]
pub struct FeatureCollection {
    #[serde(rename = "type")]
    pub collection_type: &'static str,
    pub features: Vec<Feature>,
}

#[derive(Debug, Serialize)]
pub struct Feature {
    #[serde(rename = "type")]
    pub feature_type: &'static str,
    pub geometry: Geometry,
    pub properties: serde_json::Value,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type")]
pub enum Geometry {
    #[serde(rename = "Point")]
    Point { coordinates: [f64; 2] },
}

pub fn new_geojson_source(features: Vec<Feature>) -> GeoJsonSource {
    GeoJsonSource {
        source_type: "geojson",
        data: FeatureCollection {
            collection_type: "FeatureCollection",
            features,
        },
    }
}

pub fn new_point_feature(lon: f64, lat: f64, properties: serde_json::Value) -> Feature {
    Feature {
        feature_type: "Feature",
        geometry: Geometry::Point {
            coordinates: [lon, lat],
        },
        properties,
    }
}

pub fn to_js_value<T: Serialize>(value: &T) -> Result<JsValue, JsValue> {
    serde_wasm_bindgen::to_value(value)
        .map_err(|e| JsValue::from_str(&format!("Failed to serialize GeoJSON: {:?}", e)))
}
