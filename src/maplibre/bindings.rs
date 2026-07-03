use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Element, HtmlElement};

#[wasm_bindgen]
extern "C" {
    // Map
    #[wasm_bindgen(js_namespace = maplibregl, js_name = Map)]
    pub type Map;

    #[wasm_bindgen(constructor, js_namespace = maplibregl, js_name = Map)]
    pub fn new(options: &JsValue) -> Map;

    #[wasm_bindgen(method, js_name = getContainer)]
    pub fn get_container(this: &Map) -> HtmlElement;

    #[wasm_bindgen(method, js_name = getCanvas)]
    pub fn get_canvas(this: &Map) -> Element;

    #[wasm_bindgen(method)]
    pub fn addControl(this: &Map, control: &JsValue, position: Option<&str>) -> Map;

    #[wasm_bindgen(method, js_name = setLayoutProperty)]
    pub fn set_layout_property(this: &Map, layer_id: &str, name: &str, value: &JsValue) -> Map;

    #[wasm_bindgen(method, js_name = getLayer)]
    pub fn get_layer_raw(this: &Map, id: &str) -> JsValue;

    #[wasm_bindgen(method, js_name = addSource)]
    pub fn add_source(this: &Map, id: &str, source: &JsValue) -> Map;

    #[wasm_bindgen(method, js_name = getSource)]
    pub fn get_source_raw(this: &Map, id: &str) -> JsValue;

    #[wasm_bindgen(method, js_name = addLayer)]
    pub fn add_layer(this: &Map, layer: &JsValue) -> Map;

    #[wasm_bindgen(method, js_name = on)]
    pub fn on(this: &Map, event: &str, handler: &JsValue) -> Map;

    #[wasm_bindgen(method, js_name = off)]
    pub fn off(this: &Map, event: &str, handler: &JsValue) -> Map;

    #[wasm_bindgen(method, js_name = isStyleLoaded)]
    pub fn is_style_loaded(this: &Map) -> bool;

    #[wasm_bindgen(method, js_name = getBounds)]
    pub fn get_bounds(this: &Map) -> JsValue;

    #[wasm_bindgen(method, js_name = setFeatureState)]
    pub fn set_feature_state_raw(this: &Map, options: &JsValue, state: &JsValue) -> Map;

    // NavigationControl
    #[wasm_bindgen(js_namespace = maplibregl, js_name = NavigationControl)]
    pub type NavigationControl;

    #[wasm_bindgen(constructor, js_namespace = maplibregl, js_name = NavigationControl)]
    pub fn new() -> NavigationControl;

    // ScaleControl
    #[wasm_bindgen(js_namespace = maplibregl, js_name = ScaleControl)]
    pub type ScaleControl;

    #[wasm_bindgen(constructor, js_namespace = maplibregl, js_name = ScaleControl)]
    pub fn new(options: &JsValue) -> ScaleControl;

    // Popup
    #[wasm_bindgen(js_namespace = maplibregl, js_name = Popup)]
    pub type Popup;

    #[wasm_bindgen(constructor, js_namespace = maplibregl, js_name = Popup)]
    pub fn new(options: &JsValue) -> Popup;

    #[wasm_bindgen(method, js_name = setLngLat)]
    pub fn set_lng_lat(this: &Popup, lnglat: &JsValue) -> Popup;

    #[wasm_bindgen(method, js_name = setHTML)]
    pub fn set_html(this: &Popup, html: &str) -> Popup;

    #[wasm_bindgen(method, js_name = addTo)]
    pub fn add_to(this: &Popup, map: &Map) -> Popup;

    #[wasm_bindgen(method, js_name = remove)]
    pub fn remove(this: &Popup);
}

impl Map {
    pub fn get_layer(&self, id: &str) -> Option<JsValue> {
        let raw = self.get_layer_raw(id);
        if raw.is_null() || raw.is_undefined() {
            None
        } else {
            Some(raw)
        }
    }

    pub fn get_source(&self, id: &str) -> Option<JsValue> {
        let raw = self.get_source_raw(id);
        if raw.is_null() || raw.is_undefined() {
            None
        } else {
            Some(raw)
        }
    }

    pub fn on_layer(&self, event: &str, layer_id: &str, handler: &JsValue) {
        let args = js_sys::Array::new();
        args.push(&JsValue::from_str(event));
        args.push(&JsValue::from_str(layer_id));
        args.push(handler);
        if let Ok(on_fn) = js_sys::Reflect::get(self, &JsValue::from_str("on")) {
            let on_function: &js_sys::Function = on_fn.as_ref().unchecked_ref();
            let _ = on_function.apply(self, &args);
        }
    }
}
