use crate::maplibre::bindings::{Map, NavigationControl, ScaleControl};
use wasm_bindgen::prelude::*;

pub struct ControlManager;

impl ControlManager {
    pub fn new() -> Self {
        Self
    }

    pub fn add_all_controls(&mut self, map: &Map) -> Result<(), JsValue> {
        map.addControl(&JsValue::from(NavigationControl::new()), Some("top-right"));

        let scale_options = {
            let obj = js_sys::Object::new();
            js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("maxWidth"),
                &JsValue::from_f64(100.0),
            )?;
            js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("unit"),
                &JsValue::from_str("metric"),
            )?;
            obj
        };
        map.addControl(
            &JsValue::from(ScaleControl::new(&scale_options)),
            Some("bottom-left"),
        );

        Ok(())
    }
}
