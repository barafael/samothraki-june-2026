mod app;
mod data;
mod maplibre;
#[cfg(feature = "fullstack")]
mod server_fns;
mod utils;

#[cfg(target_arch = "wasm32")]
fn init_logging() {
    console_error_panic_hook::set_once();
}

#[cfg(not(target_arch = "wasm32"))]
fn init_logging() {
    // Server-side logging - no console required
}

fn main() {
    init_logging();
    utils::log::info("My Holiday app starting...");

    // dx serve --fullstack passes `server` for the native server build.
    // We serve the original photo directory directly at `/photos/<file>` via a
    // static file service, so photos are never copied or duplicated into assets.
    #[cfg(feature = "server")]
    dioxus::serve(|| async move {
        use tower_http::services::ServeDir;
        let router = dioxus::server::router(app::App)
            .nest_service("/photos", ServeDir::new(server_fns::PHOTOS_SRC_DIR));
        Ok(router)
    });

    // Everything else (plain web, or fullstack client build)
    #[cfg(not(feature = "server"))]
    dioxus::LaunchBuilder::new().launch(app::App);
}
