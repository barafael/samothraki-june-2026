mod controls;
mod lifecycle;

pub use controls::*;

use crate::maplibre::bindings::Map;
use wasm_bindgen::prelude::*;

pub struct MapLibreManager {
    pub map: Option<Map>,
    pub control_manager: ControlManager,
}

impl MapLibreManager {
    pub fn new() -> Self {
        Self {
            map: None,
            control_manager: ControlManager::new(),
        }
    }

    pub fn create_map(&mut self, container_id: &str, center: [f64; 2]) -> Result<(), JsValue> {
        lifecycle::create_map(self, container_id, center)
    }

    pub fn add_map_controls(&mut self) -> Result<(), JsValue> {
        if let Some(map) = &self.map {
            self.control_manager.add_all_controls(map)
        } else {
            Err(JsValue::from_str("Map not initialized"))
        }
    }
}
