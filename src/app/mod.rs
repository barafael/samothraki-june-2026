use dioxus::prelude::*;

mod canvas;

use crate::data::PhotoEntry;
use canvas::Canvas;

#[cfg(feature = "fullstack")]
use crate::server_fns;

#[component]
pub fn App() -> Element {
    let mut photos = use_signal(Vec::<PhotoEntry>::new);
    let mut photos_loaded = use_signal(|| false);

    // Load photo data at runtime (not `include_str!`), so annotations saved by
    // the server show up without a rebuild and writing the file doesn't retrigger
    // one (it's no longer a compile-time dependency).
    use_future(move || async move {
        if *photos_loaded.read() {
            return;
        }
        #[cfg(feature = "fullstack")]
        {
            if let Ok(data) = server_fns::load_photo_data().await {
                photos.set(data);
            }
            photos_loaded.set(true);
        }
    });

    rsx! {
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

            Canvas { photos, photos_loaded: *photos_loaded.read() }
        }
    }
}
