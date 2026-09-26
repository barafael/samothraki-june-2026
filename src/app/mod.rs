use dioxus::prelude::*;

mod canvas;

use crate::data::PhotoEntry;
use canvas::Canvas;

// Bundle the stylesheet through the asset pipeline so it's actually served
// (the legacy `[web.resource] style` entry in Dioxus.toml is not copied in 0.7).
const MAIN_CSS: Asset = asset!("/assets/main.css");

#[component]
pub fn App() -> Element {
    let mut photos = use_signal(Vec::<PhotoEntry>::new);
    let mut photos_loaded = use_signal(|| false);

    // Photo data comes from the server at runtime, so annotations show up
    // without a rebuild. On failure, render an empty map rather than nothing.
    use_future(move || async move {
        if *photos_loaded.read() {
            return;
        }
        match crate::api::load_photo_data().await {
            Ok(entries) => photos.set(entries),
            Err(e) => crate::utils::log::error_(&format!("load_photo_data failed: {e}")),
        }
        photos_loaded.set(true);
    });

    rsx! {
        document::Stylesheet { href: MAIN_CSS }
        div {
            id: "app",
            style: "width:100%; height:100vh; display:flex; flex-direction:column;",

            header {
                style: "padding:8px 16px; background:#1a1a2e; color:#eee; font-family:sans-serif; display:flex; align-items:center; gap:12px;",
                h1 { style: "margin:0; font-size:1.2rem;", "Samothraki Holiday" }
                span { style: "font-size:0.85rem; color:#999;",
                    if *photos_loaded.read() {
                        "{photos.read().len()} photos"
                    } else {
                        "Loading photos..."
                    }
                }
            }

            Canvas { photos, photos_loaded }
        }
    }
}
