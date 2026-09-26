//! Native server half of the app (`server` feature): runs on fly.io in
//! production and under `dx serve` locally.
//!
//! * [`media`] serves `/photos/<file>` and `/videos/<file>` from local disk when
//!   present, otherwise proxies (and caches) them from GitHub Releases.
//! * [`store`] holds `photo_data.json`: a local file in development, or the
//!   copy in the GitHub repo (read and committed via the contents API) when
//!   `PHOTO_DATA_GITHUB_TOKEN` is set.
//! * [`auth`] gates annotation saves behind `ANNOTATE_PASSWORD`.
//!
//! Configuration is read from the environment; see `deploy.md`.

pub mod auth;
pub mod media;
pub mod store;
pub mod time;

use std::sync::OnceLock;
use std::time::Duration;

pub use store::store;

/// Repository holding both the media releases and `photo_data.json`.
pub fn github_repo() -> String {
    env_or("GITHUB_REPO", "barafael/samothraki-june-2026")
}

pub fn env_or(key: &str, default: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_string())
}

/// Shared HTTP client for GitHub (API and release downloads).
pub fn http() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("samothraki-holiday")
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(60))
            .build()
            .expect("build HTTP client")
    })
}
