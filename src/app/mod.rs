use dioxus::prelude::*;

mod canvas;

use crate::data::PhotoEntry;
use canvas::Canvas;

static PHOTO_DATA: &str = include_str!("../../assets/photo_data.json");

#[component]
pub fn App() -> Element {
    let mut photos = use_signal(Vec::<PhotoEntry>::new);
    let mut photos_loaded = use_signal(|| false);

    use_future(move || async move {
        if *photos_loaded.read() {
            return;
        }
        if let Ok(data) = serde_json::from_str::<Vec<PhotoEntry>>(PHOTO_DATA) {
            photos.set(data);
            photos_loaded.set(true);
        }
    });

    rsx! {
        div {
            id: "app",
            style: "width:100%; height:100vh; display:flex; flex-direction:column;",

            header {
                style: "padding:8px 16px; background:#1a1a2e; color:#eee; font-family:sans-serif; display:flex; align-items:center; gap:12px;",
                h1 { style: "margin:0; font-size:1.2rem;", "Samothrace Holiday" }
                span { style: "font-size:0.85rem; color:#999;",
                    if *photos_loaded.read() {
                        "{photos.read().len()} photos"
                    } else {
                        "Loading photos..."
                    }
                }
            }

            Canvas { photos, photos_loaded: *photos_loaded.read() }
        }
    }
}
