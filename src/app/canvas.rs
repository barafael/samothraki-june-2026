use std::rc::Rc;

use dioxus::prelude::*;
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use crate::data::{calculate_center, photos_to_geojson, PhotoEntry};
use crate::maplibre::bindings::Map;
use crate::maplibre::helpers::{create_geojson_source, load_css, load_script};
use crate::maplibre::manager::MapLibreManager;
use crate::utils::log;

static PHOTO_PANEL_MIN_PCT: f64 = 20.0;
static PHOTO_PANEL_MAX_PCT: f64 = 80.0;

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

/// Full-resolution image URL for a photo, routed through the asset base so it
/// resolves to R2 in production and root-relative locally. Thumbnails
/// (`photo.thumb`) are generated and published but not yet consumed by the UI.
fn full_url(photo: &PhotoEntry) -> String {
    crate::config::asset_url(&photo.path)
}

/// URL for a video's browser-playable (H.264) transcode: the originals are HEVC
/// and won't play in <video>, so videos are served from `media/` (transcoded),
/// while photos stay on `photos/`. `path` is stored as `photos/<file>`. Routed
/// through the asset base so it resolves to R2 in production.
fn web_video_url(path: &str) -> String {
    let file = path.strip_prefix("photos/").unwrap_or(path);
    crate::config::asset_url(&format!("media/{}", file))
}

/// Toggle the `.picking` CSS class on the map container. The class forces a
/// crosshair cursor via `!important`, which reliably overrides MapLibre's own
/// inline cursor (grab/pointer) that it sets during interaction.
#[cfg(feature = "editor")]
fn set_map_picking_cursor(on: bool) {
    let el = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id("map-container"));
    if let Some(el) = el {
        let list = el.class_list();
        let _ = if on {
            list.add_1("picking")
        } else {
            list.remove_1("picking")
        };
    }
}

/// Index of the photo `step` positions from `cur` in chronological order.
/// `photos` isn't stored time-sorted, so we sort indices by timestamp (the
/// `YYYY:MM:DD HH:MM:SS` format sorts chronologically) and wrap around.
fn neighbor_in_time(photos: &[PhotoEntry], cur: usize, step: isize) -> Option<usize> {
    if photos.is_empty() {
        return None;
    }
    let mut order: Vec<usize> = (0..photos.len()).collect();
    order.sort_by(|&a, &b| {
        photos[a]
            .timestamp
            .cmp(&photos[b].timestamp)
            .then(photos[a].filename.cmp(&photos[b].filename))
    });
    let pos = order.iter().position(|&i| i == cur)?;
    let n = order.len() as isize;
    let next_pos = (((pos as isize + step) % n) + n) % n;
    Some(order[next_pos as usize])
}

fn update_selected_on_map(path: Option<&str>) {
    let filter = path
        .map(|p| format!("['==',['get','path'],'{}']", p.replace('\'', "\\'")))
        .unwrap_or_else(|| "['==',['get','path'],'']".into());
    // Log a string, not the raw error object: the dx devtools console hook
    // forwards console args over its websocket and rejects non-string payloads
    // ("invalid type: map, expected a string").
    let _ = js_sys::eval(&format!(
        "try{{var m=window.mapInstance;if(m)m.setFilter('holiday-photos-highlight',{})}}catch(e){{console.error('setFilter failed: '+e)}}",
        filter,
    ));
}

fn sync_map_source(filtered: &[PhotoEntry]) {
    let gj = match photos_to_geojson(filtered) {
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

fn apply_filters(photos_all: &[PhotoEntry], date_filter: &str, selected_path: Option<&str>) {
    let filtered: Vec<PhotoEntry> = photos_all
        .iter()
        .filter(|p| {
            if !date_filter.is_empty() {
                let pd = exif_date_part(&p.timestamp);
                if pd != date_filter {
                    return false;
                }
            }
            true
        })
        .cloned()
        .collect();
    sync_map_source(&filtered);
    update_selected_on_map(selected_path);
}

#[component]
pub fn Canvas(photos: Signal<Vec<PhotoEntry>>, photos_loaded: Signal<bool>) -> Element {
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

    // Tab state. The Annotate tab (and everything it drives) is editor-only.
    let active_tab = use_signal(|| "map".to_string());

    // ---- Annotation state (editor only) ----
    #[cfg(feature = "editor")]
    let annotate = annotation::use_annotation_state(photos);

    // Double-click a photo to view it fullscreen; ESC exits.
    let fullscreen = use_signal(|| false);

    // Global ESC handler: leave fullscreen. Installed once.
    {
        let mut fs = fullscreen;
        use_effect(move || {
            let handler = Closure::wrap(Box::new(move |e: web_sys::KeyboardEvent| {
                if e.key() == "Escape" {
                    fs.set(false);
                }
            }) as Box<dyn FnMut(web_sys::KeyboardEvent)>);
            if let Some(doc) = web_sys::window().and_then(|w| w.document()) {
                let _ = doc
                    .add_event_listener_with_callback("keydown", handler.as_ref().unchecked_ref());
            }
            handler.forget();
        });
    }

    // Filter state (date only)
    let filter_date = use_signal(String::new);

    // Shared rebuild function using Rc
    let rebuild: Rc<dyn Fn()> = {
        let photos_sig = photos;
        let fd = filter_date;
        let sp = selected_photo;
        Rc::new(move || {
            let photos_all = photos_sig.read().clone();
            let date = fd.read().clone();
            let sel_path = sp.read().as_ref().map(|p| p.path.clone());
            apply_filters(&photos_all, &date, sel_path.as_deref());
        })
    };

    let rebuild_init = rebuild.clone();
    let rebuild_filters = rebuild.clone();

    // Map initialization (use_effect)
    let photos_init = photos;
    let photos_clone_init = photos;
    let sel_photo_init = selected_photo;
    let sel_idx_init = selected_idx;
    let prev_id_init = prev_feature_id;
    let mg_init = manager;
    // Captured by the marker-click handler so clicking a marker in annotate mode
    // loads that photo into the form for re-positioning (editor only).
    #[cfg(feature = "editor")]
    let edit_tab_init = active_tab;
    #[cfg(feature = "editor")]
    let annotate_init = annotate;

    use_effect(move || {
        // Read the signal so the effect re-runs once photos finish loading
        // (the data is fetched async, so it isn't ready on first render).
        if !*photos_loaded.read() || *initialized.read() {
            return;
        }
        initialized.set(true);
        log::info("Initializing map");

        let _ = load_css("https://unpkg.com/maplibre-gl@3.6.2/dist/maplibre-gl.css");

        let center = calculate_center(&photos_init.read());
        let geojson = match photos_to_geojson(&photos_init.read()) {
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
                        #[cfg(feature = "editor")]
                        let edit_tab = edit_tab_init;
                        #[cfg(feature = "editor")]
                        let ann = annotate_init;

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
                                    #[cfg(feature = "editor")]
                                    let et = edit_tab;
                                    #[cfg(feature = "editor")]
                                    let ann_click = ann;

                                    let ch = Closure::wrap(Box::new(move |event: JsValue| {
                                        // In pick mode, a marker click must pick coordinates, not
                                        // select the marker; let the general click handler take it.
                                        #[cfg(feature = "editor")]
                                        if *ann_click.picking.read() {
                                            return;
                                        }
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
                                                        // In annotate mode, load the clicked photo
                                                        // into the form so its location can be edited.
                                                        #[cfg(feature = "editor")]
                                                        if *et.read() == "annotate" {
                                                            let mut a = ann_click;
                                                            a.filename.set(photo.filename.clone());
                                                            a.lat.set(format!("{:.6}", photo.lat));
                                                            a.lng.set(format!("{:.6}", photo.lng));
                                                            a.preview_url.set(
                                                                crate::config::asset_url(&format!(
                                                                    "photos/{}",
                                                                    photo.filename
                                                                )),
                                                            );
                                                        }
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

                                    // "Pick a location" mode: a general map click fills the
                                    // lat/lng fields, but only while picking is active (editor).
                                    #[cfg(feature = "editor")]
                                    {
                                        let pick_handler = {
                                            let mut a = ann;
                                            Closure::wrap(Box::new(move |e: JsValue| {
                                                if !*a.picking.read() {
                                                    return;
                                                }
                                                let lnglat = match js_sys::Reflect::get(
                                                    &e,
                                                    &"lngLat".into(),
                                                ) {
                                                    Ok(v) => v,
                                                    Err(_) => return,
                                                };
                                                let lng =
                                                    js_sys::Reflect::get(&lnglat, &"lng".into())
                                                        .ok()
                                                        .and_then(|v| v.as_f64());
                                                let lat =
                                                    js_sys::Reflect::get(&lnglat, &"lat".into())
                                                        .ok()
                                                        .and_then(|v| v.as_f64());
                                                if let (Some(lat), Some(lng)) = (lat, lng) {
                                                    a.lat.set(format!("{:.6}", lat));
                                                    a.lng.set(format!("{:.6}", lng));
                                                    a.status.set("Location picked".to_string());
                                                    a.picking.set(false);
                                                    set_map_picking_cursor(false);
                                                }
                                            })
                                                as Box<dyn FnMut(JsValue)>)
                                        };
                                        map_ref.on("click", pick_handler.as_ref());
                                        pick_handler.forget();
                                    }

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
    // While dragging, drop the transform transition so panning is real-time
    // (not animated); otherwise keep it so zoom stays smooth.
    let img_transition = if *img_dragging.read() {
        "none"
    } else {
        "transform 0.15s"
    };
    let (date_min, date_max) = photo_date_range(&photos.read());

    // Nav button photos
    let photos_p = photos;
    let photos_n = photos;

    // Pre-compute tab styles (editor only — the viewer has no tab bar).
    #[cfg(feature = "editor")]
    let is_map_tab = active_tab() == "map";
    #[cfg(feature = "editor")]
    let is_annotate_tab = active_tab() == "annotate";
    #[cfg(feature = "editor")]
    let map_tab_style = format!("padding:8px 16px; border:none; background:{}; color:{}; font-size:0.85rem; cursor:pointer; border-bottom:{};",
        if is_map_tab { "#16213e" } else { "transparent" },
        if is_map_tab { "#fff" } else { "#888" },
        if is_map_tab { "2px solid #ff6b35" } else { "2px solid transparent" },
    );
    #[cfg(feature = "editor")]
    let annotate_tab_style = format!("padding:8px 16px; border:none; background:{}; color:{}; font-size:0.85rem; cursor:pointer; border-bottom:{};",
        if is_annotate_tab { "#16213e" } else { "transparent" },
        if is_annotate_tab { "#fff" } else { "#888" },
        if is_annotate_tab { "2px solid #ff6b35" } else { "2px solid transparent" },
    );

    rsx! {
        div {
            id: "split-container",
            class: "split-container",
            style: "display:flex; flex-direction:column; flex:1; overflow:hidden;",

            // Tab bar. In the viewer there's only the map, so the tab bar is
            // editor-only (the Annotate tab pulls in all editing UI).
            {
                #[cfg(feature = "editor")]
                { rsx! {
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
                } }
                #[cfg(not(feature = "editor"))]
                { rsx! {} }
            }

            // Filter bar (map mode only) — date filter.
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
                    button {
                        style: "padding:1px 10px; border-radius:3px; border:1px solid #555; background:#222; color:#aaa; font-size:0.75rem; cursor:pointer;",
                        onclick: {
                            let mut fd = filter_date;
                            let r = rebuild_filters.clone();
                            move |_| {
                                fd.set(String::new());
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
                                // translate() is applied in the parent (unscaled) space, so add
                                // the raw cursor delta for 1:1 drag tracking.
                                let dx = cx - pcx();
                                let dy = cy - pcy();
                                px.set(px() + dx);
                                py.set(py() + dy);
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

                    {
                        // Annotate tab (editor only). When active, render the form.
                        #[cfg(feature = "editor")]
                        if active_tab() == "annotate" {
                            annotation::annotate_panel(
                                photos,
                                annotate,
                                zoom_level,
                                img_dragging,
                                pan_x,
                                pan_y,
                                pan_client_x,
                                pan_client_y,
                                cursor,
                                img_transition,
                            )
                        } else {
                            detail_panel(
                                selected_photo,
                                selected_idx,
                                prev_feature_id,
                                photos_p,
                                photos_n,
                                zoom_level,
                                pan_x,
                                pan_y,
                                pan_anchor_x,
                                pan_anchor_y,
                                pan_client_x,
                                pan_client_y,
                                img_dragging,
                                fullscreen,
                                cursor,
                                img_transition,
                            )
                        }

                        #[cfg(not(feature = "editor"))]
                        detail_panel(
                            selected_photo,
                            selected_idx,
                            prev_feature_id,
                            photos_p,
                            photos_n,
                            zoom_level,
                            pan_x,
                            pan_y,
                            pan_anchor_x,
                            pan_anchor_y,
                            pan_client_x,
                            pan_client_y,
                            img_dragging,
                            fullscreen,
                            cursor,
                            img_transition,
                        )
                    }
                }
            }
        }
    }
}

/// The photo detail/lightbox panel: selected photo with zoom/pan, time-ordered
/// prev/next navigation, or an empty-state prompt. Shared by editor and viewer.
#[allow(clippy::too_many_arguments)]
fn detail_panel(
    selected_photo: Signal<Option<PhotoEntry>>,
    selected_idx: Signal<Option<usize>>,
    prev_feature_id: Signal<Option<String>>,
    photos_p: Signal<Vec<PhotoEntry>>,
    photos_n: Signal<Vec<PhotoEntry>>,
    zoom_level: Signal<f64>,
    pan_x: Signal<f64>,
    pan_y: Signal<f64>,
    pan_anchor_x: Signal<f64>,
    pan_anchor_y: Signal<f64>,
    pan_client_x: Signal<f64>,
    pan_client_y: Signal<f64>,
    img_dragging: Signal<bool>,
    fullscreen: Signal<bool>,
    cursor: &'static str,
    img_transition: &'static str,
) -> Element {
    if let Some(photo) = selected_photo() {
        rsx! {
            div {
                style: if fullscreen() {
                    "position:fixed; inset:0; z-index:9999; background:#000; display:flex; flex-direction:column; overflow:hidden;"
                } else {
                    "flex:1; display:flex; flex-direction:column; overflow:hidden; position:relative;"
                },
                div {
                    style: "flex:1; display:flex; align-items:center; justify-content:center; overflow:hidden; padding:8px; cursor:{cursor};",
                    ondoubleclick: {
                        let mut fs = fullscreen;
                        move |_| { let now = !fs(); fs.set(now); }
                    },
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
                            src: web_video_url(&photo.path),
                            controls: "true",
                            autoplay: "true",
                            r#loop: "true",
                        }
                    } else {
                        img {
                            style: "max-width:100%; max-height:100%; object-fit:contain; border-radius:4px; transition:{img_transition}; transform: translate({pan_x()}px, {pan_y()}px) scale({zoom_level()});",
                            src: full_url(&photo),
                        }
                    }
                }
                div {
                    style: "position:absolute; bottom:8px; right:8px; display:flex; gap:4px; z-index:1;",
                    if fullscreen() {
                        button {
                            style: "width:32px; height:32px; border:none; border-radius:4px; background:rgba(255,255,255,0.15); color:#eee; font-size:0.9rem; cursor:pointer; display:flex; align-items:center; justify-content:center;",
                            title: "Exit fullscreen (Esc)",
                            onclick: {
                                let mut fs = fullscreen;
                                move |_| fs.set(false)
                            },
                            "✕"
                        }
                    }
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
                                    let ni = neighbor_in_time(&photos_p.read(), idx, -1);
                                    if let Some(ni) = ni {
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
                                    let ni = neighbor_in_time(&photos_n.read(), idx, 1);
                                    if let Some(ni) = ni {
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
                            }
                        },
                        "▶"
                    }
                }
            }
        }
    } else {
        rsx! {
            div {
                style: "flex:1; display:flex; flex-direction:column; align-items:center; justify-content:center; color:#555; font-size:0.9rem; gap:8px;",
                span { "Click a photo marker on the map" }
                span { style: "font-size:0.8rem; color:#444;", "{photos_n.read().len()} photos" }
            }
        }
    }
}

/// Editor-only annotation workflow: file picker, lat/lng entry, pick-on-map,
/// save-back to disk. Entirely compiled out of the viewer build.
#[cfg(feature = "editor")]
mod annotation {
    use super::set_map_picking_cursor;
    use crate::data::PhotoEntry;
    use crate::server_fns;
    use dioxus::prelude::*;

    /// Grouped annotation signals, so the map click handlers and the panel share
    /// one small Copy struct instead of a dozen loose signals.
    #[derive(Clone, Copy)]
    pub struct AnnotationState {
        pub filename: Signal<String>,
        pub lat: Signal<String>,
        pub lng: Signal<String>,
        pub saving: Signal<bool>,
        pub status: Signal<String>,
        pub preview_url: Signal<String>,
        pub picking: Signal<bool>,
        /// Files without GPS, minus any already annotated this session.
        pub no_gps: Signal<Vec<String>>,
    }

    pub fn use_annotation_state(photos: Signal<Vec<PhotoEntry>>) -> AnnotationState {
        // Files with no GPS come from a build-time scan; this list is only needed
        // for the editor's annotation dropdown, so it's compiled in here only.
        static NO_GPS_JSON: &str = include_str!("../../assets/photos_no_gps.json");
        let all_no_gps: Vec<String> = serde_json::from_str::<serde_json::Value>(NO_GPS_JSON)
            .ok()
            .and_then(|v| {
                let files = v.get("files_no_gps")?;
                let images = files.get("images")?.as_array()?;
                let videos = files.get("videos")?.as_array()?;
                Some(
                    images
                        .iter()
                        .chain(videos.iter())
                        .filter_map(|f| f.as_str().map(String::from))
                        .collect(),
                )
            })
            .unwrap_or_default();

        let no_gps = use_signal(move || all_no_gps.clone());

        // Reactive: drop files that have since been annotated (now present in
        // `photos`), so the dropdown shrinks as locations are assigned.
        {
            let mut no_gps = no_gps;
            let base = no_gps.read().clone();
            use_effect(move || {
                let annotated: std::collections::HashSet<String> =
                    photos.read().iter().map(|p| p.filename.clone()).collect();
                let filtered: Vec<String> = base
                    .iter()
                    .filter(|f| !annotated.contains(*f))
                    .cloned()
                    .collect();
                if *no_gps.read() != filtered {
                    no_gps.set(filtered);
                }
            });
        }

        AnnotationState {
            filename: use_signal(String::new),
            lat: use_signal(String::new),
            lng: use_signal(String::new),
            saving: use_signal(|| false),
            status: use_signal(String::new),
            preview_url: use_signal(String::new),
            picking: use_signal(|| false),
            no_gps,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn annotate_panel(
        photos: Signal<Vec<PhotoEntry>>,
        ann: AnnotationState,
        zoom_level: Signal<f64>,
        img_dragging: Signal<bool>,
        pan_x: Signal<f64>,
        pan_y: Signal<f64>,
        pan_client_x: Signal<f64>,
        pan_client_y: Signal<f64>,
        cursor: &'static str,
        img_transition: &'static str,
    ) -> Element {
        let no_gps_filenames = ann.no_gps.read().clone();
        let save_btn_style = format!("margin-top:8px; padding:8px 16px; border:none; border-radius:4px; background:#ff6b35; color:#fff; font-size:0.85rem; cursor:pointer;{}",
            if (ann.saving)() { " opacity:0.6;" } else { "" },
        );

        rsx! {
            div {
                style: "flex:1; display:flex; flex-direction:column; overflow:hidden; padding:12px; gap:8px; font-size:0.8rem;",

                h3 { style: "margin:0 0 4px; font-size:0.95rem;", "Location Annotation" }
                p { style: "margin:0; color:#999; font-size:0.75rem;", "Assign GPS coordinates to files without location data." }

                label { style: "color:#888; margin-top:4px;", "File" }
                select {
                    style: "width:100%; background:#222; color:#ccc; border:1px solid #444; border-radius:3px; padding:4px 6px; font-size:0.75rem;",
                    value: "{(ann.filename)()}",
                    onchange: {
                        let mut a = ann;
                        move |e| {
                            let fname = e.value().to_string();
                            a.filename.set(fname.clone());
                            a.lat.set(String::new());
                            a.lng.set(String::new());
                            if fname.is_empty() {
                                a.status.set(String::new());
                                a.preview_url.set(String::new());
                            } else {
                                a.status.set("Photo loaded".to_string());
                                a.preview_url.set(crate::config::asset_url(&format!("photos/{}", fname)));
                            }
                        }
                    },
                    option { value: "", "Select a file…" }
                    // An already-annotated photo (clicked on the map) isn't in the
                    // no-GPS list; surface it so the select reflects what's loaded.
                    if !(ann.filename)().is_empty() && !no_gps_filenames.contains(&(ann.filename)()) {
                        option { value: "{(ann.filename)()}", "{(ann.filename)()} (editing)" }
                    }
                    for fname in no_gps_filenames.iter() {
                        option { value: "{fname}", "{fname}" }
                    }
                }

                label { style: "color:#888; margin-top:4px;", "Latitude" }
                input {
                    r#type: "number",
                    step: "any",
                    style: "width:100%; background:#222; color:#ccc; border:1px solid #444; border-radius:3px; padding:4px 6px; font-size:0.75rem; box-sizing:border-box;",
                    value: "{(ann.lat)()}",
                    placeholder: "e.g. 40.4831",
                    oninput: {
                        let mut a = ann;
                        move |e| a.lat.set(e.value().to_string())
                    },
                }

                label { style: "color:#888; margin-top:4px;", "Longitude" }
                input {
                    r#type: "number",
                    step: "any",
                    style: "width:100%; background:#222; color:#ccc; border:1px solid #444; border-radius:3px; padding:4px 6px; font-size:0.75rem; box-sizing:border-box;",
                    value: "{(ann.lng)()}",
                    placeholder: "e.g. 25.6455",
                    oninput: {
                        let mut a = ann;
                        move |e| a.lng.set(e.value().to_string())
                    },
                }

                // Pick-a-location button: toggles map click-to-pick mode
                button {
                    style: if (ann.picking)() {
                        "margin-top:6px; width:100%; padding:8px; border:none; border-radius:4px; background:#3a7bd5; color:#fff; font-size:0.8rem; cursor:pointer;"
                    } else {
                        "margin-top:6px; width:100%; padding:8px; border:1px solid #3a7bd5; border-radius:4px; background:transparent; color:#8ab4f0; font-size:0.8rem; cursor:pointer;"
                    },
                    onclick: {
                        let mut a = ann;
                        move |_| {
                            let now = !(a.picking)();
                            a.picking.set(now);
                            set_map_picking_cursor(now);
                            a.status.set(if now {
                                "Click the map to pick a location".to_string()
                            } else {
                                String::new()
                            });
                        }
                    },
                    if (ann.picking)() { "Click map to pick… (cancel)" } else { "📍 Pick a location on map" }
                }

                // Save button
                button {
                    style: "{save_btn_style}",
                    disabled: (ann.saving)(),
                    onclick: {
                        let mut a = ann;
                        move |_| {
                            let fname = a.filename.read().clone();
                            if fname.is_empty() { a.status.set("Select a file".to_string()); return; }
                            {
                                let _: f64 = match a.lat.read().parse() { Ok(v) => v, Err(_) => { a.status.set("Invalid latitude".to_string()); return; } };
                                let _: f64 = match a.lng.read().parse() { Ok(v) => v, Err(_) => { a.status.set("Invalid longitude".to_string()); return; } };
                            }
                            a.saving.set(true);
                            a.status.set("Saving...".to_string());

                            // The file to load once this save succeeds: the next one
                            // in the no-GPS list, or empty if this was the last.
                            let no_gps = a.no_gps.read().clone();
                            let next_file = no_gps
                                .iter()
                                .position(|f| *f == fname)
                                .and_then(|i| no_gps.get(i + 1))
                                .cloned()
                                .unwrap_or_default();

                            let lat = a.lat.read().parse::<f64>().unwrap_or(0.0);
                            let lng = a.lng.read().parse::<f64>().unwrap_or(0.0);
                            let mut ps = photos;
                            spawn(async move {
                                match server_fns::save_annotation(fname.clone(), lat, lng).await {
                                    Ok(entry) => {
                                        let mut list = ps.read().clone();
                                        if let Some(pos) = list.iter().position(|e| e.filename == fname) {
                                            list[pos] = entry;
                                        } else {
                                            list.push(entry);
                                        }
                                        ps.set(list);
                                        a.saving.set(false);
                                        a.lat.set(String::new());
                                        a.lng.set(String::new());
                                        a.filename.set(next_file.clone());
                                        if next_file.is_empty() {
                                            a.status.set("Saved! No more files.".to_string());
                                            a.preview_url.set(String::new());
                                        } else {
                                            a.status.set(format!("Saved! Next: {}", next_file));
                                            a.preview_url.set(crate::config::asset_url(&format!("photos/{}", next_file)));
                                        }
                                    }
                                    Err(e) => {
                                        a.status.set(format!("Error: {:?}", e));
                                        a.saving.set(false);
                                    }
                                }
                            });
                        }
                    },
                    if (ann.saving)() { "Saving…" } else { "Save Location" }
                }

                // Status message
                if !(ann.status)().is_empty() {
                    p { style: "margin:4px 0 0; font-size:0.75rem; color:#ff6b35;", "{(ann.status)()}" }
                }

                // Photo preview — pan/zoom like the main gallery view.
                if !(ann.preview_url)().is_empty() {
                    div {
                        style: "flex:1; position:relative; overflow:hidden; min-height:0; background:#000; border-radius:4px; margin-top:4px;",
                        div {
                            style: "width:100%; height:100%; display:flex; align-items:center; justify-content:center; overflow:hidden; cursor:{cursor};",
                            onmousedown: {
                                let mut id = img_dragging;
                                let mut pcx = pan_client_x;
                                let mut pcy = pan_client_y;
                                move |e| {
                                    if zoom_level() > 1.05 {
                                        pcx.set(e.data.coordinates().client().x);
                                        pcy.set(e.data.coordinates().client().y);
                                        id.set(true);
                                        e.prevent_default();
                                    }
                                }
                            },
                            img {
                                style: "max-width:100%; max-height:100%; object-fit:contain; border-radius:4px; transition:{img_transition}; transform: translate({pan_x()}px, {pan_y()}px) scale({zoom_level()});",
                                src: "{(ann.preview_url)()}",
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
                                    move |_| { zl.set(1.0); px.set(0.0); py.set(0.0); }
                                },
                                "1:1"
                            }
                        }
                    }
                }

                // Tips
                div { style: "border-top:1px solid #333; padding-top:8px; font-size:0.7rem; color:#666;",
                    p { style: "margin:0 0 2px;", "Use the map to find coordinates, then enter them above." }
                    p { style: "margin:0;", "Saved photos will appear on the Map tab with the rest." }
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
