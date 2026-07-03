mod app;
mod data;
mod maplibre;
#[cfg(feature = "fullstack")]
mod server_fns;
mod utils;

#[cfg(target_arch = "wasm32")]
fn init_logging() {
    console_error_panic_hook::set_once();
    coerce_console_to_strings();
}

/// The dx dev devtools hook forwards `console.error`/`console.warn` arguments
/// over its websocket and rejects any non-string payload with
/// "invalid type: map, expected a string". Third-party JS (MapLibre) logs raw
/// error/event *objects*, which trip that parser and spam the dev server.
///
/// Wrap the console methods so every argument is coerced to a string before it
/// reaches the dx hook. Installed at startup, after the hook, so ours runs first.
#[cfg(target_arch = "wasm32")]
fn coerce_console_to_strings() {
    let _ = js_sys::eval(
        r#"(function(){
            var c = console;
            ['error','warn','info','log','debug'].forEach(function(name){
                var orig = c[name];
                if (typeof orig !== 'function') return;
                c[name] = function(){
                    var args = Array.prototype.map.call(arguments, function(a){
                        if (typeof a === 'string') return a;
                        if (a instanceof Error) return a.stack || (a.name + ': ' + a.message);
                        try { return JSON.stringify(a); } catch (e) { return String(a); }
                    });
                    return orig.apply(c, args);
                };
            });
        })();"#,
    );
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
