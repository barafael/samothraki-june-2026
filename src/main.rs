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

    // dx serve --fullstack passes `server` for the native server build
    #[cfg(feature = "server")]
    dioxus::LaunchBuilder::server().launch(app::App);

    // Everything else (plain web, or fullstack client build)
    #[cfg(not(feature = "server"))]
    dioxus::LaunchBuilder::new().launch(app::App);
}
