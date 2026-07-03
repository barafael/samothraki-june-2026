use std::collections::HashMap;
use std::rc::Rc;

use dioxus::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::data::{calculate_center, photos_to_geojson, PhotoEntry};
use crate::maplibre::bindings::Map;
use crate::maplibre::helpers::{create_geojson_source, load_css, load_script};
use crate::maplibre::manager::MapLibreManager;
use crate::utils::log;

#[cfg(feature = "fullstack")]
use crate::server_fns;

#[derive(Debug, Clone, PartialEq)]
enum Tok {
    LParen,
    RParen,
    And,
    Or,
    Name(String),
}

/// Recursive-descent tag expression evaluator.
/// A photo passes if its tags satisfy the expression.
fn tag_expr_matches(expr: &str, photo_tags: &[String]) -> bool {
    let tokens = tokenize(expr);
    if tokens.is_empty() {
        return true;
    }
    let mut pos = 0;
    
    eval_or(&tokens, &mut pos, photo_tags)
}

fn tag_color(name: &str) -> String {
    let palette = [
        "#e6194b", "#3cb44b", "#ffe119", "#4363d8", "#f58231", "#911eb4", "#42d4f4", "#f032e6",
        "#bfef45", "#fabed4", "#469990", "#dcbeff", "#9A6324", "#fffac8", "#800000", "#aaffc3",
        "#808000", "#ffd8b1", "#000075", "#a9a9a9",
    ];
    let mut h = 0u64;
    for b in name.bytes() {
        h = h.wrapping_mul(31).wrapping_add(b as u64);
    }
    palette[(h as usize) % palette.len()].to_string()
}

/// Returns (text, color) segments for syntax-highlighted display.
fn colorize_expr(expr: &str, known_tags: &[String]) -> Vec<(String, String)> {
    let mut segs: Vec<(String, String)> = Vec::new();
    let mut i = 0;
    let bytes = expr.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b' ' || bytes[i] == b'\t' {
            let s = i;
            while i < bytes.len() && (bytes[i] == b' ' || bytes[i] == b'\t') {
                i += 1;
            }
            segs.push((expr[s..i].to_string(), "#555".into()));
        } else if bytes[i] == b'(' || bytes[i] == b')' {
            segs.push((expr[i..i + 1].to_string(), "#000".into()));
            i += 1;
        } else {
            let s = i;
            while i < bytes.len() && !matches!(bytes[i], b' ' | b'\t' | b'(' | b')') {
                i += 1;
            }
            let word = &expr[s..i];
            let lc = word.to_lowercase();
            let color = match lc.as_str() {
                "and" | "or" => "#888".into(),
                _ if known_tags.iter().any(|t| t.eq_ignore_ascii_case(word)) => tag_color(&lc),
                _ => "#ccc".into(),
            };
            segs.push((word.to_string(), color));
        }
    }
    segs
}

fn tokenize(s: &str) -> Vec<Tok> {
    let s = s.to_lowercase();
    let mut out = Vec::new();
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b' ' || bytes[i] == b'\t' {
            i += 1;
            continue;
        }
        match bytes[i] {
            b'(' => {
                out.push(Tok::LParen);
                i += 1;
            }
            b')' => {
                out.push(Tok::RParen);
                i += 1;
            }
            _ => {
                let start = i;
                while i < bytes.len() && !matches!(bytes[i], b' ' | b'\t' | b'(' | b')') {
                    i += 1;
                }
                let word: String = s[start..i].to_string();
                match word.as_str() {
                    "and" => out.push(Tok::And),
                    "or" => out.push(Tok::Or),
                    _ => out.push(Tok::Name(word)),
                }
            }
        }
    }
    out
}

fn eval_or(tokens: &[Tok], pos: &mut usize, photo_tags: &[String]) -> bool {
    let mut r = eval_and(tokens, pos, photo_tags);
    while *pos < tokens.len() && tokens[*pos] == Tok::Or {
        *pos += 1;
        r = r || eval_and(tokens, pos, photo_tags);
    }
    r
}

fn eval_and(tokens: &[Tok], pos: &mut usize, photo_tags: &[String]) -> bool {
    let mut r = eval_primary(tokens, pos, photo_tags);
    while *pos < tokens.len() && tokens[*pos] == Tok::And {
        *pos += 1;
        r = r && eval_primary(tokens, pos, photo_tags);
    }
    r
}

fn eval_primary(tokens: &[Tok], pos: &mut usize, photo_tags: &[String]) -> bool {
    if *pos >= tokens.len() {
        return true;
    }
    match &tokens[*pos] {
        Tok::LParen => {
            *pos += 1;
            let r = eval_or(tokens, pos, photo_tags);
            if *pos < tokens.len() && tokens[*pos] == Tok::RParen {
                *pos += 1;
            }
            r
        }
        Tok::Name(name) => {
            *pos += 1;
            photo_tags.iter().any(|t| t.eq_ignore_ascii_case(name))
        }
        _ => true,
    }
}

static PHOTO_PANEL_MIN_PCT: f64 = 20.0;
static PHOTO_PANEL_MAX_PCT: f64 = 80.0;

#[cfg(not(feature = "fullstack"))]
const LS_KEY: &str = "my_holiday_photo_tags";

#[cfg(not(feature = "fullstack"))]
fn load_tags_from_store() -> HashMap<String, Vec<String>> {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return HashMap::new(),
    };
    let ls = match window.local_storage() {
        Ok(Some(s)) => s,
        _ => return HashMap::new(),
    };
    match ls.get_item(LS_KEY) {
        Ok(Some(val)) => serde_json::from_str(&val).unwrap_or_default(),
        _ => HashMap::new(),
    }
}

#[cfg(not(feature = "fullstack"))]
fn save_tags_to_store(tags: &HashMap<String, Vec<String>>) {
    let window = match web_sys::window() {
        Some(w) => w,
        None => return,
    };
    let ls = match window.local_storage() {
        Ok(Some(s)) => s,
        _ => return,
    };
    if let Ok(json) = serde_json::to_string(tags) {
        let _ = ls.set_item(LS_KEY, &json);
    }
}

fn collect_all_tags(tags_map: &HashMap<String, Vec<String>>) -> Vec<String> {
    let mut set = std::collections::BTreeSet::new();
    for tags in tags_map.values() {
        for t in tags {
            set.insert(t.clone());
        }
    }
    set.into_iter().collect()
}

fn exif_date_part(exif: &str) -> String {
    exif.split(' ').next().unwrap_or(exif).replace(':', "-")
}

fn photo_date_range(photos: &[PhotoEntry]) -> (String, String) {
    let dates: Vec<String> = photos
        .iter()
        .map(|p| exif_date_part(&p.timestamp))
        .collect();
    let empty = String::new();
    let min = dates.iter().min().unwrap_or(&empty);
    let max = dates.iter().max().unwrap_or(&empty);
    (min.clone(), max.clone())
}

fn update_selected_on_map(path: Option<&str>) {
    let filter = path
        .map(|p| format!("['==',['get','path'],'{}']", p.replace('\'', "\\'")))
        .unwrap_or_else(|| "['==',['get','path'],'']".into());
    let _ = js_sys::eval(&format!(
        "try{{var m=window.mapInstance;if(m)m.setFilter('holiday-photos-highlight',{})}}catch(e){{console.error(e)}}",
        filter,
    ));
}

fn sync_map_source(filtered: &[PhotoEntry], tags: &HashMap<String, Vec<String>>) {
    let gj = match photos_to_geojson(filtered, tags) {
        Ok(g) => g,
        Err(_) => return,
    };
    let win = match web_sys::window() {
        Some(w) => w,
        None => return,
    };
    js_sys::Reflect::set(&win, &"__fgj".into(), &gj).ok();
    let _ = js_sys::eval(
        "try{var m=window.mapInstance;var s=m?m.getSource('holiday-photos'):null;if(s)s.setData(window.__fgj)}catch(e){}",
    );
}
fn apply_filters(
    photos_all: &[PhotoEntry],
    tags_map: &HashMap<String, Vec<String>>,
    date_filter: &str,
    tag_expr: &str,
    selected_path: Option<&str>,
) {
    let filtered: Vec<&PhotoEntry> = photos_all
        .iter()
        .filter(|p| {
            if !date_filter.is_empty() {
                let pd = exif_date_part(&p.timestamp);
                if pd != date_filter {
                    return false;
                }
            }
            if !tag_expr.is_empty() {
                let pts = tags_map.get(&p.path).cloned().unwrap_or_default();
                if !tag_expr_matches(tag_expr, &pts) {
                    return false;
                }
            }
            true
        })
        .collect();
    let owned: Vec<PhotoEntry> = filtered.into_iter().cloned().collect();
    sync_map_source(&owned, tags_map);
    update_selected_on_map(selected_path);
}

#[component]
pub fn Canvas(photos: Signal<Vec<PhotoEntry>>, photos_loaded: bool) -> Element {
    let manager = use_signal(MapLibreManager::new);
    let mut initialized = use_signal(|| false);
    let selected_photo = use_signal(|| None::<PhotoEntry>);
    let selected_idx = use_signal(|| None::<usize>);
    let prev_feature_id = use_signal(|| None::<String>);
    let zoom_level = use_signal(|| 1.0);
    let split_pos = use_signal(|| 30.0);
    let dragging = use_signal(|| false);
    let img_dragging = use_signal(|| false);
    let pan_x = use_signal(|| 0.0);
    let pan_y = use_signal(|| 0.0);
    let pan_anchor_x = use_signal(|| 0.0);
    let pan_anchor_y = use_signal(|| 0.0);
    let pan_client_x = use_signal(|| 0.0);
    let pan_client_y = use_signal(|| 0.0);

    // Tag state
    let photo_tags = use_signal(HashMap::<String, Vec<String>>::new);
    let all_tags = use_signal(|| collect_all_tags(&HashMap::new()));
    let new_tag_input = use_signal(String::new);
    let tags_loaded = use_signal(|| false);

    // Tab state
    let active_tab = use_signal(|| "map".to_string());

    // Annotation state
    static NO_GPS_JSON: &str = include_str!("../../assets/photos_no_gps.json");
    let no_gps_filenames: Vec<String> = {
        serde_json::from_str::<serde_json::Value>(NO_GPS_JSON)
            .ok()
            .and_then(|v| {
                let files = v.get("files_no_gps")?;
                let images = files.get("images")?.as_array()?;
                let videos = files.get("videos")?.as_array()?;
                let all: Vec<String> = images
                    .iter()
                    .chain(videos.iter())
                    .filter_map(|f| f.as_str().map(String::from))
                    .collect();
                Some(all)
            })
            .unwrap_or_default()
    };
    let annotate_filename = use_signal(String::new);
    let annotate_lat = use_signal(String::new);
    let annotate_lng = use_signal(String::new);
    let annotate_saving = use_signal(|| false);
    let annotate_status = use_signal(String::new);
    let preview_url = use_signal(String::new);

    // Load tags from server
    {
        let mut pt = photo_tags;
        let mut at = all_tags;
        let mut tl = tags_loaded;
        use_future(move || async move {
            if *tl.read() {
                return;
            }
            #[cfg(feature = "fullstack")]
            match server_fns::load_tags().await {
                Ok(tags) => {
                    at.set(collect_all_tags(&tags));
                    pt.set(tags);
                    tl.set(true);
                }
                Err(e) => log::error_(&format!("Failed to load tags: {:?}", e)),
            }
            #[cfg(not(feature = "fullstack"))]
            {
                let tags = load_tags_from_store();
                at.set(collect_all_tags(&tags));
                pt.set(tags);
                tl.set(true);
            }
        });
    }

    // Filter state
    let filter_date = use_signal(String::new);
    let filter_expr = use_signal(String::new);

    // Shared rebuild function using Rc
    let rebuild: Rc<dyn Fn()> = {
        let photos_sig = photos;
        let pt = photo_tags;
        let fd = filter_date;
        let fe = filter_expr;
        let sp = selected_photo;
        Rc::new(move || {
            let photos_all = photos_sig.read().clone();
            let tags = pt.read().clone();
            let date = fd.read().clone();
            let expr = fe.read().clone();
            let sel_path = sp.read().as_ref().map(|p| p.path.clone());
            apply_filters(&photos_all, &tags, &date, &expr, sel_path.as_deref());
        })
    };

    // Clone rebuild Rc's for all callback groups
    let rebuild_init = rebuild.clone();
    let rebuild_filters = rebuild.clone();
    let rebuild_tags = rebuild.clone();
    let rebuild_pills = rebuild.clone();

    // Map initialization (use_effect)
    let photos_init = photos;
    let photos_clone_init = photos;
    let sel_photo_init = selected_photo;
    let sel_idx_init = selected_idx;
    let prev_id_init = prev_feature_id;
    let mg_init = manager;
    let pt_init = photo_tags;
    let _pt_click = photo_tags;

    use_effect(move || {
        if !photos_loaded || *initialized.read() {
            return;
        }
        initialized.set(true);
        log::info("Initializing map");

        let _ = load_css("https://unpkg.com/maplibre-gl@3.6.2/dist/maplibre-gl.css");

        let center = calculate_center(&photos_init.read());
        let tags_map = pt_init.read().clone();
        let geojson = match photos_to_geojson(&photos_init.read(), &tags_map) {
            Ok(gj) => gj,
            Err(e) => {
                log::error_(&format!("Failed build GeoJSON: {:?}", e));
                return;
            }
        };

        let photos_click = photos_clone_init;
        let spc = sel_photo_init;
        let sic = sel_idx_init;
        let pic = prev_id_init;

        let on_load = {
            let mut mg = mg_init;
            let center_clone = center;
            let geojson_clone = geojson;
            let r_init = rebuild_init.clone();

            Closure::wrap(Box::new(move || {
                log::info("MapLibre script loaded");
                {
                    let mg_ref = &mut *mg.write();
                    if let Err(err) = mg_ref.create_map("maplibre-canvas", center_clone) {
                        log::error_(&format!("Failed create map: {:?}", err));
                        return;
                    }
                    if let Err(err) = mg_ref.add_map_controls() {
                        log::error_(&format!("Failed add controls: {:?}", err));
                        return;
                    }
                }
                if let Some(map_ref) = get_map_ref() {
                    let gj = geojson_clone.clone();
                    let load_handler = {
                        let plist = photos_click;
                        let r2 = r_init.clone();
                        let spc2 = spc;
                        let sic2 = sic;
                        let pic2 = pic;

                        Closure::wrap(Box::new(move || {
                            log::info("Style loaded, adding photo markers");
                            let map_ref = match get_map_ref() {
                                Some(m) => m,
                                None => {
                                    log::error_("No map");
                                    return;
                                }
                            };
                            let src = create_geojson_source(&gj);
                            match src {
                                Ok(s) => {
                                    map_ref.add_source("holiday-photos", &s);
                                    log::info("Added source");

                                    let base_layer = js_sys::Object::new();
                                    js_sys::Reflect::set(
                                        &base_layer,
                                        &"id".into(),
                                        &"holiday-photos-layer".into(),
                                    )
                                    .ok();
                                    js_sys::Reflect::set(
                                        &base_layer,
                                        &"type".into(),
                                        &"circle".into(),
                                    )
                                    .ok();
                                    js_sys::Reflect::set(
                                        &base_layer,
                                        &"source".into(),
                                        &"holiday-photos".into(),
                                    )
                                    .ok();
                                    let base_paint = js_sys::Object::new();
                                    js_sys::Reflect::set(
                                        &base_paint,
                                        &"circle-radius".into(),
                                        &JsValue::from_f64(4.0),
                                    )
                                    .ok();
                                    js_sys::Reflect::set(
                                        &base_paint,
                                        &"circle-color".into(),
                                        &JsValue::from_str("#777777"),
                                    )
                                    .ok();
                                    js_sys::Reflect::set(
                                        &base_paint,
                                        &"circle-stroke-color".into(),
                                        &JsValue::from_str("#444444"),
                                    )
                                    .ok();
                                    js_sys::Reflect::set(
                                        &base_paint,
                                        &"circle-stroke-width".into(),
                                        &JsValue::from_f64(2.0),
                                    )
                                    .ok();
                                    js_sys::Reflect::set(&base_layer, &"paint".into(), &base_paint)
                                        .ok();
                                    map_ref.add_layer(&base_layer.into());

                                    // Highlight layer (orange, shown only for selected feature)
                                    let hl_layer = js_sys::Object::new();
                                    js_sys::Reflect::set(
                                        &hl_layer,
                                        &"id".into(),
                                        &"holiday-photos-highlight".into(),
                                    )
                                    .ok();
                                    js_sys::Reflect::set(
                                        &hl_layer,
                                        &"type".into(),
                                        &"circle".into(),
                                    )
                                    .ok();
                                    js_sys::Reflect::set(
                                        &hl_layer,
                                        &"source".into(),
                                        &"holiday-photos".into(),
                                    )
                                    .ok();
                                    let hl_filter = js_sys::Array::new();
                                    hl_filter.push(&"==".into());
                                    let get_path = js_sys::Array::new();
                                    get_path.push(&"get".into());
                                    get_path.push(&"path".into());
                                    hl_filter.push(&get_path);
                                    hl_filter.push(&JsValue::from_str(""));
                                    js_sys::Reflect::set(&hl_layer, &"filter".into(), &hl_filter)
                                        .ok();
                                    let hl_paint = js_sys::Object::new();
                                    js_sys::Reflect::set(
                                        &hl_paint,
                                        &"circle-radius".into(),
                                        &JsValue::from_f64(6.0),
                                    )
                                    .ok();
                                    js_sys::Reflect::set(
                                        &hl_paint,
                                        &"circle-color".into(),
                                        &JsValue::from_str("#ff6b35"),
                                    )
                                    .ok();
                                    js_sys::Reflect::set(
                                        &hl_paint,
                                        &"circle-stroke-color".into(),
                                        &JsValue::from_str("#ffffff"),
                                    )
                                    .ok();
                                    js_sys::Reflect::set(
                                        &hl_paint,
                                        &"circle-stroke-width".into(),
                                        &JsValue::from_f64(2.0),
                                    )
                                    .ok();
                                    js_sys::Reflect::set(&hl_layer, &"paint".into(), &hl_paint)
                                        .ok();
                                    map_ref.add_layer(&hl_layer.into());
                                    log::info("Added layers");

                                    let mut spc3 = spc2;
                                    let mut sic3 = sic2;
                                    let mut pic3 = pic2;
                                    let plist3 = plist;
                                    let r3 = r2.clone();

                                    let ch = Closure::wrap(Box::new(move |event: JsValue| {
                                        let feats =
                                            js_sys::Reflect::get(&event, &"features".into())
                                                .ok()
                                                .and_then(|f| {
                                                    if f.is_array() {
                                                        let a = js_sys::Array::from(&f);
                                                        if a.length() > 0 {
                                                            Some(a)
                                                        } else {
                                                            None
                                                        }
                                                    } else {
                                                        None
                                                    }
                                                });
                                        if let Some(feats) = feats {
                                            if let Some(feat) =
                                                feats.get(0).dyn_ref::<js_sys::Object>()
                                            {
                                                let pv = js_sys::Reflect::get(
                                                    feat,
                                                    &"properties".into(),
                                                )
                                                .ok()
                                                .unwrap_or(JsValue::UNDEFINED);
                                                let path =
                                                    js_sys::Reflect::get(&pv, &"path".into())
                                                        .ok()
                                                        .and_then(|v| v.as_string());
                                                    if let Some(ref ph) = path {
                                                        if let Some((idx, photo)) = plist3
                                                            .read()
                                                            .iter()
                                                            .enumerate()
                                                            .find(|(_, p)| p.path == *ph)
                                                    {
                                                        update_selected_on_map(Some(ph));
                                                        pic3.set(Some(ph.clone()));
                                                        spc3.set(Some(photo.clone()));
                                                        sic3.set(Some(idx));
                                                    }
                                                }
                                            }
                                        }
                                    })
                                        as Box<dyn FnMut(JsValue)>);
                                    let jc = ch.as_ref().clone();
                                    map_ref.on_layer("click", "holiday-photos-layer", &jc);
                                    ch.forget();

                                    let _ = js_sys::eval("(function(){var m=window.mapInstance;if(!m)return;m.on('mouseenter','holiday-photos-layer',function(){m.getCanvas().style.cursor='pointer'});m.on('mouseleave','holiday-photos-layer',function(){m.getCanvas().style.cursor=''})})();");

                                    log::info("Click handler set");
                                    r3();
                                }
                                Err(e) => log::error_(&format!("Source err: {:?}", e)),
                            }
                        }) as Box<dyn FnMut()>)
                    };
                    let jh = load_handler.as_ref().clone();
                    map_ref.on("load", &jh);
                    load_handler.forget();
                }
            }) as Box<dyn FnMut()>)
        };
        let jv = on_load.as_ref().clone();
        if let Err(err) = load_script(
            "https://unpkg.com/maplibre-gl@3.6.2/dist/maplibre-gl.js",
            Some(&jv),
        ) {
            log::error_(&format!("Failed load MapLibre: {:?}", err));
        }
        on_load.forget();
    });

    let cursor = if *zoom_level.read() > 1.05 {
        "grab"
    } else {
        "default"
    };
    let (date_min, date_max) = photo_date_range(&photos.read());
    let colored_tokens = colorize_expr(&filter_expr(), &all_tags());

    // Pre-compute tag pill data for the selected photo
    let selected_tag_data: Vec<(String, String)> = match selected_photo() {
        Some(ref photo) => {
            let cur_tags = photo_tags
                .read()
                .get(&photo.path)
                .cloned()
                .unwrap_or_default();
            cur_tags
                .into_iter()
                .map(|t| (t, photo.path.clone()))
                .collect()
        }
        None => Vec::new(),
    };

    // Nav button photos
    let photos_p = photos;
    let photos_n = photos;

    // Pre-compute tab styles
    let is_map_tab = active_tab() == "map";
    let is_annotate_tab = active_tab() == "annotate";
    let map_tab_style = format!("padding:8px 16px; border:none; background:{}; color:{}; font-size:0.85rem; cursor:pointer; border-bottom:{};",
        if is_map_tab { "#16213e" } else { "transparent" },
        if is_map_tab { "#fff" } else { "#888" },
        if is_map_tab { "2px solid #ff6b35" } else { "2px solid transparent" },
    );
    let annotate_tab_style = format!("padding:8px 16px; border:none; background:{}; color:{}; font-size:0.85rem; cursor:pointer; border-bottom:{};",
        if is_annotate_tab { "#16213e" } else { "transparent" },
        if is_annotate_tab { "#fff" } else { "#888" },
        if is_annotate_tab { "2px solid #ff6b35" } else { "2px solid transparent" },
    );
    let save_btn_style = format!("margin-top:8px; padding:8px 16px; border:none; border-radius:4px; background:#ff6b35; color:#fff; font-size:0.85rem; cursor:pointer;{}",
        if annotate_saving() { " opacity:0.6;" } else { "" },
    );

    rsx! {
        div {
            id: "split-container",
            class: "split-container",
            style: "display:flex; flex-direction:column; flex:1; overflow:hidden;",

            // Tab bar
            div {
                style: "display:flex; align-items:center; gap:0; padding:0; background:#1a1a2e; border-bottom:1px solid #333; flex-shrink:0;",
                button {
                    style: "{map_tab_style}",
                    onclick: {
                        let mut at = active_tab;
                        move |_| at.set("map".to_string())
                    },
                    "🗺 Map"
                }
                button {
                    style: "{annotate_tab_style}",
                    onclick: {
                        let mut at = active_tab;
                        move |_| at.set("annotate".to_string())
                    },
                    "📍 Annotate"
                }
            }

            // Filter bar (map mode only)
            if active_tab() == "map" {
                div {
                    style: "display:flex; align-items:center; gap:10px; padding:6px 12px; background:#1a1a2e; color:#ccc; font-size:0.8rem; font-family:sans-serif; border-bottom:1px solid #333; flex-shrink:0;",
                    span { style: "color:#888; font-weight:bold; margin-right:2px;", "Filter" }
                    span { style: "color:#666;", "Date" }
                    input {
                        r#type: "date",
                        min: "{date_min}",
                        max: "{date_max}",
                        style: "background:#222; color:#ccc; border:1px solid #444; border-radius:3px; padding:2px 6px; font-size:0.75rem; width:140px;",
                        value: "{filter_date()}",
                        oninput: {
                            let mut fd = filter_date;
                            let r = rebuild_filters.clone();
                            move |e| {
                                fd.set(e.value().to_string());
                                r();
                            }
                        },
                    }
                    span { style: "color:#666;", "Tags" }
                    div {
                        style: "position:relative; flex:1; min-width:180px; height:1.4rem; font-family:monospace; font-size:0.75rem;",
                        div {
                            style: "position:absolute; inset:0; pointer-events:none; white-space:pre; overflow:hidden; padding:2px 6px; border:1px solid transparent; line-height:1.4rem;",
                            for (txt, clr) in &colored_tokens {
                                span { style: "color:{clr};", "{txt}" }
                            }
                        }
                        input {
                            r#type: "text",
                            placeholder: "beach and (sunset or family)",
                            style: "position:relative; width:100%; height:100%; background:transparent; color:transparent; caret-color:#ccc; border:1px solid #444; border-radius:3px; padding:2px 6px; font:inherit; outline:none; box-sizing:border-box;",
                            value: "{filter_expr()}",
                            oninput: {
                                let mut fe = filter_expr;
                                let r = rebuild_filters.clone();
                                move |e| {
                                    fe.set(e.value().to_string());
                                    r();
                                }
                            },
                        }
                    }
                    button {
                        style: "padding:1px 10px; border-radius:3px; border:1px solid #555; background:#222; color:#aaa; font-size:0.75rem; cursor:pointer;",
                        onclick: {
                            let mut fd = filter_date;
                            let mut fe = filter_expr;
                            let r = rebuild_filters.clone();
                            move |_| {
                                fd.set(String::new());
                                fe.set(String::new());
                                r();
                            }
                        },
                        "Clear"
                    }
                }
            }

            // Main split area
            div {
                style: "display:flex; flex:1; overflow:hidden;",
                onmousemove: {
                    let dr = dragging;
                    let mut sp = split_pos;
                    let idr = img_dragging;
                    let mut px = pan_x;
                    let mut py = pan_y;
                    let _pax = pan_anchor_x;
                    let _pay = pan_anchor_y;
                    let mut pcx = pan_client_x;
                    let mut pcy = pan_client_y;
                    move |e| {
                        if *dr.read() {
                            let el = web_sys::window().and_then(|w| w.document()).and_then(|d| d.get_element_by_id("split-container"));
                            if let Some(c) = el {
                                let r = c.get_bounding_client_rect();
                                let x = e.data.coordinates().client().x - r.left();
                                let dp = (x / r.width() * 100.0).max(PHOTO_PANEL_MIN_PCT).min(PHOTO_PANEL_MAX_PCT);
                                sp.set(100.0 - dp);
                            }
                        }
                        if *idr.read() {
                            let z = zoom_level();
                            if z > 1.05 {
                                let cx = e.data.coordinates().client().x;
                                let cy = e.data.coordinates().client().y;
                                let dx = cx - pcx();
                                let dy = cy - pcy();
                                px.set(px() + dx / z);
                                py.set(py() + dy / z);
                                pcx.set(cx);
                                pcy.set(cy);
                            }
                        }
                    }
                },
                onmouseup: {
                    let mut dr = dragging;
                    let mut idr = img_dragging;
                    move |_| {
                        dr.set(false);
                        idr.set(false);
                    }
                },
                onmouseleave: {
                    let mut dr = dragging;
                    let mut idr = img_dragging;
                    move |_| {
                        dr.set(false);
                        idr.set(false);
                    }
                },

                div {
                    id: "map-container",
                    style: "position:relative; width: {100.0 - split_pos()}%;",
                    div {
                        id: "maplibre-canvas",
                        style: "position:absolute; top:0; bottom:0; left:0; right:0;"
                    }
                }

                div {
                    class: "divider",
                    style: "width:6px; cursor:col-resize; flex-shrink:0; background:#555; position:relative;",
                    onmousedown: {
                        let mut dr = dragging;
                        move |e| { e.prevent_default(); dr.set(true); }
                    },
                    div { style: "position:absolute; top:50%; left:50%; transform:translate(-50%,-50%); width:3px; height:30px; background:#888; border-radius:2px;" }
                }

                div {
                    class: "photo-panel",
                    style: "width: {split_pos()}%; overflow:hidden; display:flex; flex-direction:column; background:#111; color:#eee; font-family:sans-serif;",

                    if active_tab() == "annotate" {
                        div {
                            style: "flex:1; display:flex; flex-direction:column; overflow:hidden; padding:12px; gap:8px; font-size:0.8rem;",

                            h3 { style: "margin:0 0 4px; font-size:0.95rem;", "Location Annotation" }
                            p { style: "margin:0; color:#999; font-size:0.75rem;", "Assign GPS coordinates to files without location data." }

                            label { style: "color:#888; margin-top:4px;", "File" }
                            select {
                                style: "width:100%; background:#222; color:#ccc; border:1px solid #444; border-radius:3px; padding:4px 6px; font-size:0.75rem;",
                                value: "{annotate_filename()}",
                                onchange: {
                                    let mut af = annotate_filename;
                                    let mut al = annotate_lat;
                                    let mut alng = annotate_lng;
                                    let mut st = annotate_status;
                                    let mut pu = preview_url;
                                    move |e| {
                                        let fname = e.value().to_string();
                                        af.set(fname.clone());
                                        al.set(String::new());
                                        alng.set(String::new());
                                        st.set(String::new());
                                        pu.set(String::new());
                                        if !fname.is_empty() {
                                            #[cfg(feature = "fullstack")]
                                            spawn({
                                                let mut st2 = st.clone();
                                                let mut pu2 = pu.clone();
                                                let fn2 = fname.clone();
                                                async move {
                                                    match server_fns::ensure_photo_copied(fn2).await {
                                                        Ok(dst) => {
                                                            st2.set("Photo loaded".to_string());
                                                            pu2.set(format!("/{}", dst));
                                                        }
                                                        Err(e) => st2.set(format!("Copy error: {:?}", e)),
                                                    }
                                                }
                                            });
                                        }
                                    }
                                },
                                option { value: "", "Select a file…" }
                                for fname in &no_gps_filenames {
                                    option { value: "{fname}", "{fname}" }
                                }
                            }

                            label { style: "color:#888; margin-top:4px;", "Latitude" }
                            input {
                                r#type: "number",
                                step: "any",
                                style: "width:100%; background:#222; color:#ccc; border:1px solid #444; border-radius:3px; padding:4px 6px; font-size:0.75rem; box-sizing:border-box;",
                                value: "{annotate_lat()}",
                                placeholder: "e.g. 40.4831",
                                oninput: {
                                    let mut al = annotate_lat;
                                    move |e| al.set(e.value().to_string())
                                },
                            }

                            label { style: "color:#888; margin-top:4px;", "Longitude" }
                            input {
                                r#type: "number",
                                step: "any",
                                style: "width:100%; background:#222; color:#ccc; border:1px solid #444; border-radius:3px; padding:4px 6px; font-size:0.75rem; box-sizing:border-box;",
                                value: "{annotate_lng()}",
                                placeholder: "e.g. 25.6455",
                                oninput: {
                                    let mut al = annotate_lng;
                                    move |e| al.set(e.value().to_string())
                                },
                            }

                            // Save button
                            button {
                                style: "{save_btn_style}",
                                disabled: annotate_saving(),
                                onclick: {
                                    let mut af = annotate_filename;
                                    let mut al = annotate_lat;
                                    let mut alng = annotate_lng;
                                    let mut saving = annotate_saving;
                                    let mut status = annotate_status;
                                    move |_| {
                                        let fname = af.read().clone();
                                        if fname.is_empty() { status.set("Select a file".to_string()); return; }
                                        {
                                            let _: f64 = match al.read().parse() { Ok(v) => v, Err(_) => { status.set("Invalid latitude".to_string()); return; } };
                                            let _: f64 = match alng.read().parse() { Ok(v) => v, Err(_) => { status.set("Invalid longitude".to_string()); return; } };
                                        }
                                        saving.set(true);
                                        status.set("Saving...".to_string());

                                        #[cfg(feature = "fullstack")]
                                        {
                                            let lat = al.read().parse::<f64>().unwrap_or(0.0);
                                            let lng = alng.read().parse::<f64>().unwrap_or(0.0);
                                            let mut ps = photos;
                                            let mut pu2 = preview_url;
                                            spawn({
                                                let r2 = rebuild_tags.clone();
                                                async move {
                                                    match server_fns::save_annotation(fname.clone(), lat, lng).await {
                                                        Ok(entry) => {
                                                            let mut list = ps.read().clone();
                                                            if let Some(pos) = list.iter().position(|e| e.filename == fname) {
                                                                list[pos] = entry;
                                                            } else {
                                                                list.push(entry);
                                                            }
                                                            ps.set(list);
                                                            status.set("Saved!".to_string());
                                                            saving.set(false);
                                                            al.set(String::new());
                                                            alng.set(String::new());
                                                            af.set(String::new());
                                                            pu2.set(String::new());
                                                            r2();
                                                        }
                                                        Err(e) => {
                                                            status.set(format!("Error: {:?}", e));
                                                            saving.set(false);
                                                        }
                                                    }
                                                }
                                            });
                                        }

                                        #[cfg(not(feature = "fullstack"))]
                                        {
                                            status.set("Saved (in-memory, server required for persistence)".to_string());
                                            saving.set(false);
                                        }
                                    }
                                },
                                if annotate_saving() { "Saving…" } else { "Save Location" }
                            }

                            // Status message
                            if !annotate_status().is_empty() {
                                p { style: "margin:4px 0 0; font-size:0.75rem; color:#ff6b35;", "{annotate_status()}" }
                            }

                            // Photo preview
                            if !preview_url().is_empty() {
                                div {
                                    style: "flex:1; display:flex; align-items:center; justify-content:center; overflow:hidden; min-height:0; background:#000; border-radius:4px; margin-top:4px;",
                                    img {
                                        style: "max-width:100%; max-height:100%; object-fit:contain;",
                                        src: "{preview_url()}",
                                    }
                                }
                            }

                            // Tips
                            div { style: "border-top:1px solid #333; padding-top:8px; font-size:0.7rem; color:#666;",
                                p { style: "margin:0 0 2px;", "Use the map to find coordinates, then enter them above." }
                                p { style: "margin:0;", "Saved photos will appear on the Map tab with the rest." }
                            }
                        }
                    } else if let Some(photo) = selected_photo() {
                        div {
                            style: "flex:1; display:flex; flex-direction:column; overflow:hidden; position:relative;",
                            div {
                                style: "flex:1; display:flex; align-items:center; justify-content:center; overflow:hidden; padding:8px; cursor:{cursor};",
                                onmousedown: {
                                    let mut id = img_dragging;
                                    let mut pax = pan_anchor_x;
                                    let mut pay = pan_anchor_y;
                                    let mut pcx = pan_client_x;
                                    let mut pcy = pan_client_y;
                                    let px = pan_x;
                                    let py = pan_y;
                                    move |e| {
                                        if zoom_level() > 1.05 {
                                            pax.set(px());
                                            pay.set(py());
                                            pcx.set(e.data.coordinates().client().x);
                                            pcy.set(e.data.coordinates().client().y);
                                            id.set(true);
                                            e.prevent_default();
                                        }
                                    }
                                },
                                if photo.media_type.starts_with("video/") {
                                    video {
                                        style: "max-width:100%; max-height:100%; border-radius:4px;",
                                        src: "/{photo.path}",
                                        controls: "true",
                                        autoplay: "true",
                                        r#loop: "true",
                                    }
                                } else {
                                    img {
                                        style: "max-width:100%; max-height:100%; object-fit:contain; border-radius:4px; transition:transform 0.15s; transform: translate({pan_x()}px, {pan_y()}px) scale({zoom_level()});",
                                        src: "/{photo.path}",
                                    }
                                }
                            }
                            div {
                                style: "position:absolute; bottom:8px; right:8px; display:flex; gap:4px;",
                                button {
                                    style: "width:32px; height:32px; border:none; border-radius:4px; background:rgba(255,255,255,0.15); color:#eee; font-size:1.2rem; cursor:pointer; display:flex; align-items:center; justify-content:center;",
                                    onclick: {
                                        let mut zl = zoom_level;
                                        move |_| zl.set((zl() * 1.5_f64).min(10.0))
                                    },
                                    "+"
                                }
                                button {
                                    style: "width:32px; height:32px; border:none; border-radius:4px; background:rgba(255,255,255,0.15); color:#eee; font-size:1.2rem; cursor:pointer; display:flex; align-items:center; justify-content:center;",
                                    onclick: {
                                        let mut zl = zoom_level;
                                        move |_| zl.set((zl() / 1.5_f64).max(0.25))
                                    },
                                    "−"
                                }
                                button {
                                    style: "width:32px; height:32px; border:none; border-radius:4px; background:rgba(255,255,255,0.15); color:#eee; font-size:0.8rem; cursor:pointer; display:flex; align-items:center; justify-content:center;",
                                    onclick: {
                                        let mut zl = zoom_level;
                                        let mut px = pan_x;
                                        let mut py = pan_y;
                                        move |_| {
                                            zl.set(1.0);
                                            px.set(0.0);
                                            py.set(0.0);
                                        }
                                    },
                                    "1:1"
                                }
                            }
                        }
                        div {
                            style: "display:flex; flex-direction:column; border-top:1px solid #333;",
                            div {
                                style: "display:flex; align-items:center; gap:8px; padding:8px 12px;",
                                button {
                                    style: "width:36px; height:36px; border:none; border-radius:4px; background:rgba(255,255,255,0.1); color:#eee; font-size:1rem; cursor:pointer;",
                                    onclick: {
                                        let mut si = selected_idx;
                                        let mut sp = selected_photo;
                                        let mut pi = prev_feature_id;
                                        let mut zl = zoom_level;
                                        let mut px = pan_x;
                                        let mut py = pan_y;
                                        move |_| {
                                            if let Some(idx) = si() {
                                                let ni = if idx > 0 { idx - 1 } else { photos_p.read().len() - 1 };
                                                        if let Some(p) = photos_p.read().get(ni) {
                                                    update_selected_on_map(Some(&p.path));
                                                    sp.set(Some(p.clone()));
                                                    si.set(Some(ni));
                                                    pi.set(Some(p.path.clone()));
                                                    zl.set(1.0);
                                                    px.set(0.0);
                                                    py.set(0.0);
                                                }
                                            }
                                        }
                                    },
                                    "◀"
                                }
                                div {
                                    style: "flex:1; text-align:center;",
                                    if let Some(idx) = selected_idx() {
                                        p { style: "margin:0; font-weight:bold; font-size:0.85rem; line-height:1.2;", "{idx + 1} / {photos_p.read().len()}" }
                                    }
                                    p { style: "margin:0; font-size:0.75rem; color:#999; line-height:1.2;", "{photo.filename}" }
                                }
                                button {
                                    style: "width:36px; height:36px; border:none; border-radius:4px; background:rgba(255,255,255,0.1); color:#eee; font-size:1rem; cursor:pointer;",
                                    onclick: {
                                        let mut si = selected_idx;
                                        let mut sp = selected_photo;
                                        let mut pi = prev_feature_id;
                                        let mut zl = zoom_level;
                                        let mut px = pan_x;
                                        let mut py = pan_y;
                                        move |_| {
                                            if let Some(idx) = si() {
                                                let ni = (idx + 1) % photos_n.read().len();
                                                if let Some(p) = photos_n.read().get(ni) {
                                                    update_selected_on_map(Some(&p.path));
                                                    sp.set(Some(p.clone()));
                                                    si.set(Some(ni));
                                                    pi.set(Some(p.path.clone()));
                                                    zl.set(1.0);
                                                    px.set(0.0);
                                                    py.set(0.0);
                                                }
                                            }
                                        }
                                    },
                                    "▶"
                                }
                            }
                            div {
                                style: "padding:6px 12px 10px; border-top:1px solid #333;",
                                div {
                                    style: "display:flex; flex-wrap:wrap; gap:4px; margin-bottom:6px;",
                                    for (t, path_c) in selected_tag_data {
                                        span {
                                            style: "display:inline-flex; align-items:center; gap:3px; padding:2px 8px; background:#ff6b3522; border:1px solid #ff6b3544; border-radius:12px; font-size:0.75rem; color:#ddd;",
                                            "{t}",
                                            button {
                                                style: "background:none; border:none; color:#ff6b35; cursor:pointer; font-size:0.8rem; padding:0; margin:0; line-height:1;",
                                                onclick: {
                                                    let t_c = t.clone();
                                                    let p_c = path_c.clone();
                                                    let mut pt = photo_tags;
                                                    let mut at = all_tags;
                                                    let r = rebuild_pills.clone();
                                                    move |_| {
                                                        let mut map = pt.write();
                                                        let entry = map.entry(p_c.clone()).or_default();
                                                        entry.retain(|x| x != &t_c);
                                                        if entry.is_empty() { map.remove(&p_c); }
                                                        let saved = map.clone();
                                                        drop(map);
                                    spawn(async move {
                                        #[cfg(feature = "fullstack")]
                                        { let _ = server_fns::save_tags(saved).await; }
                                        #[cfg(not(feature = "fullstack"))]
                                        save_tags_to_store(&saved);
                                    });
                                                        at.set(collect_all_tags(&pt.read()));
                                                        r();
                                                    }
                                                },
                                                "✕"
                                            }
                                        }
                                    }
                                }
                                div {
                                    style: "display:flex; gap:4px; align-items:center;",
                                    select {
                                        style: "flex:1; background:#222; color:#ccc; border:1px solid #444; border-radius:3px; padding:3px 6px; font-size:0.75rem;",
                                        onchange: {
                                            let path = photo.path.clone();
                                            let mut pt = photo_tags;
                                            let mut at = all_tags;
                                            let r = rebuild_tags.clone();
                                            move |e| {
                                                let val = e.value().to_string();
                                                if val.is_empty() { return; }
                                                let mut map = pt.write();
                                                map.entry(path.clone()).or_default().push(val);
                                                let saved = map.clone();
                                                drop(map);
                                                spawn(async move {
                                        #[cfg(feature = "fullstack")]
                                        { let _ = server_fns::save_tags(saved).await; }
                                        #[cfg(not(feature = "fullstack"))]
                                        save_tags_to_store(&saved);
                                    });
                                                at.set(collect_all_tags(&pt.read()));
                                                r();
                                            }
                                        },
                                        option { value: "", "Add tag…" }
                                        for tag in all_tags() {
                                            option { value: "{tag}", "{tag}" }
                                        }
                                    }
                                    input {
                                        style: "flex:1; background:#222; color:#ccc; border:1px solid #444; border-radius:3px; padding:3px 6px; font-size:0.75rem;",
                                        placeholder: "new tag",
                                        value: "{new_tag_input()}",
                                        oninput: {
                                            let mut nti = new_tag_input;
                                            move |e| nti.set(e.value().to_string())
                                        },
                                    }
                                    button {
                                        style: "padding:3px 10px; border:none; border-radius:3px; background:#ff6b35; color:#fff; font-size:0.75rem; cursor:pointer;",
                                        onclick: {
                                            let path = photo.path.clone();
                                            let mut pt = photo_tags;
                                            let mut at = all_tags;
                                            let mut nti = new_tag_input;
                                            let r = rebuild_tags.clone();
                                            move |_| {
                                                let val = nti.read().trim().to_string();
                                                if val.is_empty() { return; }
                                                let mut map = pt.write();
                                                map.entry(path.clone()).or_default().push(val);
                                                let saved = map.clone();
                                                drop(map);
                                                spawn(async move {
                                        #[cfg(feature = "fullstack")]
                                        { let _ = server_fns::save_tags(saved).await; }
                                        #[cfg(not(feature = "fullstack"))]
                                        save_tags_to_store(&saved);
                                    });
                                                at.set(collect_all_tags(&pt.read()));
                                                nti.set(String::new());
                                                r();
                                            }
                                        },
                                        "Add"
                                    }
                                }
                            }
                        }
                    } else {
                        div {
                            style: "flex:1; display:flex; flex-direction:column; align-items:center; justify-content:center; color:#555; font-size:0.9rem; gap:8px;",
                            span { "Click a photo marker on the map" }
                            span { style: "font-size:0.8rem; color:#444;", "{photos_n.read().len()} photos" }
                        }
                    }
                }
            }
        }
    }
}

fn get_map_ref() -> Option<Map> {
    let window = web_sys::window()?;
    let val = js_sys::Reflect::get(&window, &JsValue::from_str("mapInstance")).ok()?;
    if val.is_null() || val.is_undefined() {
        None
    } else {
        Some(val.into())
    }
}
