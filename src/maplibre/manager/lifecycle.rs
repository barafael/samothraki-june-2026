use crate::maplibre::bindings::Map;
use crate::maplibre::helpers::create_map_options;
use wasm_bindgen::prelude::*;
use web_sys::window;

use super::MapLibreManager;

pub fn create_map(
    manager: &mut MapLibreManager,
    container_id: &str,
    center: [f64; 2],
) -> Result<(), JsValue> {
    let options = create_map_options(container_id, center)?;
    let map = Map::new(&options);
    manager.map = Some(map);

    if let Some(window) = window() {
        js_sys::Reflect::set(
            &window,
            &JsValue::from_str("mapInstance"),
            &JsValue::from(manager.map.as_ref().unwrap()),
        )?;
    }

    Ok(())
}
