//! Server functions: the only way the browser reads or changes photo data.
//! Bodies run on the server (`server` feature); the client gets RPC stubs.

use dioxus::prelude::*;

use crate::data::PhotoEntry;

/// All photos/videos that have a location.
#[server]
pub async fn load_photo_data() -> Result<Vec<PhotoEntry>, ServerFnError> {
    crate::server::store()
        .load()
        .await
        .map_err(ServerFnError::new)
}

/// Set (or move) the location of `filename`. `password` must match the
/// server's `ANNOTATE_PASSWORD` (see `server::auth`).
#[server]
pub async fn save_annotation(
    filename: String,
    lat: f64,
    lng: f64,
    password: String,
) -> Result<PhotoEntry, ServerFnError> {
    if let Err(message) = crate::server::auth::check_annotate_password(&password) {
        return Err(ServerFnError::ServerError {
            message,
            code: 401,
            details: None,
        });
    }
    if !crate::server::media::is_safe_filename(&filename) {
        return Err(ServerFnError::ServerError {
            message: format!("invalid filename {filename:?}"),
            code: 400,
            details: None,
        });
    }
    if !(-90.0..=90.0).contains(&lat) || !(-180.0..=180.0).contains(&lng) {
        return Err(ServerFnError::ServerError {
            message: format!("coordinates out of range: {lat}, {lng}"),
            code: 400,
            details: None,
        });
    }
    crate::server::store()
        .save(&filename, lat, lng)
        .await
        .map_err(ServerFnError::new)
}
