//! The single seam through which the app loads photo data.
//!
//! * Editor (`fullstack`): delegates to the `#[server]` fn that reads
//!   `photo_data.json` from local disk and can write annotations back.
//! * Viewer (`not(fullstack)`): fetches `manifest.json` from the asset base
//!   (R2 in production, root-relative locally) — no server code in the bundle.

use crate::data::PhotoEntry;

/// Load all photo entries. Returns an empty vec on any failure so the UI can
/// render (and, in the viewer, so a missing manifest doesn't crash the app).
pub async fn load_photos() -> Vec<PhotoEntry> {
    #[cfg(feature = "fullstack")]
    {
        match crate::server_fns::load_photo_data().await {
            Ok(entries) => entries,
            Err(e) => {
                crate::utils::log::error_(&format!("load_photo_data failed: {:?}", e));
                Vec::new()
            }
        }
    }

    #[cfg(not(feature = "fullstack"))]
    {
        fetch_manifest().await.unwrap_or_else(|e| {
            crate::utils::log::error_(&format!("manifest fetch failed: {}", e));
            Vec::new()
        })
    }
}

/// Fetch and parse the viewer manifest. Only compiled into the viewer build.
#[cfg(not(feature = "fullstack"))]
async fn fetch_manifest() -> Result<Vec<PhotoEntry>, String> {
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;

    let url = crate::config::manifest_url();
    let window = web_sys::window().ok_or("no window")?;

    let resp_value = JsFuture::from(window.fetch_with_str(&url))
        .await
        .map_err(|e| format!("fetch {url}: {:?}", e))?;
    let resp: web_sys::Response = resp_value
        .dyn_into()
        .map_err(|_| "response cast failed".to_string())?;
    if !resp.ok() {
        return Err(format!("{url} -> HTTP {}", resp.status()));
    }

    let text = JsFuture::from(resp.text().map_err(|e| format!("text(): {:?}", e))?)
        .await
        .map_err(|e| format!("read body: {:?}", e))?;
    let text = text.as_string().ok_or("body not a string")?;

    serde_json::from_str(&text).map_err(|e| format!("parse manifest: {}", e))
}
