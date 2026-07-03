use js_sys::{Array, Object, Reflect};
use wasm_bindgen::prelude::*;
use web_sys::window;

pub fn load_script(src: &str, on_load: Option<&JsValue>) -> Result<(), JsValue> {
    let window = window().ok_or_else(|| JsValue::from_str("No global window exists"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("No document exists on window"))?;

    let script = document.create_element("script")?;
    script.set_attribute("src", src)?;

    if let Some(callback) = on_load {
        Reflect::set(&script, &JsValue::from_str("onload"), callback)?;
    }

    document
        .head()
        .ok_or_else(|| JsValue::from_str("document should have head"))?
        .append_child(&script)?;

    Ok(())
}

pub fn load_css(href: &str) -> Result<(), JsValue> {
    let window = window().ok_or_else(|| JsValue::from_str("No global window exists"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("No document exists on window"))?;

    let link = document.create_element("link")?;
    link.set_attribute("rel", "stylesheet")?;
    link.set_attribute("href", href)?;

    document
        .head()
        .ok_or_else(|| JsValue::from_str("document should have head"))?
        .append_child(&link)?;

    Ok(())
}

pub fn create_geojson_source(data: &JsValue) -> Result<JsValue, JsValue> {
    let source = Object::new();
    Reflect::set(
        &source,
        &JsValue::from_str("type"),
        &JsValue::from_str("geojson"),
    )?;
    Reflect::set(&source, &JsValue::from_str("data"), data)?;
    Ok(source.into())
}

pub fn create_map_options(container_id: &str, center: [f64; 2]) -> Result<JsValue, JsValue> {
    let style = Object::new();
    Reflect::set(&style, &"version".into(), &JsValue::from_f64(8.0))?;

    let sources = Object::new();
    let raster_source = Object::new();
    Reflect::set(&raster_source, &"type".into(), &"raster".into())?;
    let tiles = Array::new();
    tiles.push(&"https://tile.opentopomap.org/{z}/{x}/{y}.png".into());
    Reflect::set(&raster_source, &"tiles".into(), &tiles)?;
    Reflect::set(
        &raster_source,
        &"tileSize".into(),
        &JsValue::from_f64(256.0),
    )?;
    Reflect::set(
        &raster_source,
        &"attribution".into(),
        &"© OpenTopoMap contributors".into(),
    )?;
    Reflect::set(&sources, &"opentopomap".into(), &raster_source)?;
    Reflect::set(&style, &"sources".into(), &sources)?;

    let layers = Array::new();
    let layer = Object::new();
    Reflect::set(&layer, &"id".into(), &"opentopomap-layer".into())?;
    Reflect::set(&layer, &"type".into(), &"raster".into())?;
    Reflect::set(&layer, &"source".into(), &"opentopomap".into())?;
    layers.push(&layer);
    Reflect::set(&style, &"layers".into(), &layers)?;

    let options = Object::new();
    Reflect::set(&options, &"container".into(), &container_id.into())?;
    Reflect::set(&options, &"style".into(), &style)?;

    let center_arr = Array::new();
    center_arr.push(&JsValue::from_f64(center[0]));
    center_arr.push(&JsValue::from_f64(center[1]));
    Reflect::set(&options, &"center".into(), &center_arr)?;
    Reflect::set(&options, &"zoom".into(), &JsValue::from_f64(11.0))?;

    Ok(options.into())
}
