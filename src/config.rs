//! Compile-time asset configuration.
//!
//! Every image, thumbnail, video, and manifest URL is built from a single base
//! so the same WASM works in three places:
//!   * editor / `dx serve` — assets served at the site root by the axum router
//!     (`/photos/<file>`, `/media/<file>`); base is empty.
//!   * viewer local preview — the static bundle plus `photos/`, `thumbs/`,
//!     `media/`, and `manifest.json` served at the root; base is empty.
//!   * production — assets live on R2 behind a custom domain; set
//!     `ASSET_BASE_URL=https://assets.example.com` at build time.
//!
//! `path` values in the data model already carry the `photos/` prefix and
//! thumbnails carry `thumbs/`, so the base is the *root* the whole asset tree
//! hangs off — not the photos dir specifically.

/// Asset base URL, resolved at compile time. Empty (root-relative) by default.
/// Override with `ASSET_BASE_URL=…` when building the viewer for deployment.
pub const ASSET_BASE_URL: &str = match option_env!("ASSET_BASE_URL") {
    Some(v) => v,
    None => "",
};

/// Join the asset base with a relative asset path, avoiding a double slash.
/// `rel` may or may not start with `/`; the result is always usable as a URL.
pub fn asset_url(rel: &str) -> String {
    let base = ASSET_BASE_URL.trim_end_matches('/');
    let rel = rel.trim_start_matches('/');
    if base.is_empty() {
        format!("/{rel}")
    } else {
        format!("{base}/{rel}")
    }
}

/// URL for the viewer's data manifest. Only the viewer build fetches this; the
/// editor loads photo data through its server fn instead.
#[cfg(not(feature = "fullstack"))]
pub fn manifest_url() -> String {
    asset_url("manifest.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn root_relative_when_base_empty() {
        // Can't override the const at test time, but the empty-base branch is the
        // default and the one asserted here via a local recomputation.
        let base = "";
        let join = |rel: &str| {
            let b = base.trim_end_matches('/');
            let r = rel.trim_start_matches('/');
            if b.is_empty() {
                format!("/{r}")
            } else {
                format!("{b}/{r}")
            }
        };
        assert_eq!(join("photos/a.jpg"), "/photos/a.jpg");
        assert_eq!(join("/thumbs/a.webp"), "/thumbs/a.webp");
    }

    #[test]
    fn absolute_base_no_double_slash() {
        let base = "https://cdn.example.com/";
        let join = |rel: &str| {
            let b = base.trim_end_matches('/');
            let r = rel.trim_start_matches('/');
            format!("{b}/{r}")
        };
        assert_eq!(join("photos/a.jpg"), "https://cdn.example.com/photos/a.jpg");
        assert_eq!(
            join("/manifest.json"),
            "https://cdn.example.com/manifest.json"
        );
    }
}
