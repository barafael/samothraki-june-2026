mod app;
mod config;
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

/// dx's devtools `monkeyPatchConsole` forwards raw `console.*` args over its
/// websocket as `{Log:{level,messages:args}}`, where `messages` must deserialize
/// to `Vec<String>`. Third-party JS (MapLibre) logs error/event *objects*, so
/// `messages:[{}]` reaches the dev server and fails to parse
/// ("invalid type: map, expected a string") — spamming the log and, because the
/// bad frame breaks the websocket, restarting the app.
///
/// Fix: coerce every console arg to a string, and make our wrapper the OUTERMOST
/// one. dx patches the console asynchronously (after `launch`), so we can't just
/// wrap once at startup — dx would wrap over us and send the raw object first.
/// Instead we re-assert on an interval: whenever `console.error` isn't our marked
/// wrapper, re-wrap the current function (idempotent via the marker). This way we
/// stringify before dx's wrapper serializes the args.
#[cfg(target_arch = "wasm32")]
fn coerce_console_to_strings() {
    let _ = js_sys::eval(
        r#"(function(){
            var c = console;
            var names = ['error','warn','info','log','debug'];
            var MARK = '__coerced_to_string';
            function stringify(a){
                if (typeof a === 'string') return a;
                if (a instanceof Error) return a.stack || (a.name + ': ' + a.message);
                try { return JSON.stringify(a); } catch (e) { return String(a); }
            }
            function reassert(){
                names.forEach(function(name){
                    var cur = c[name];
                    if (typeof cur !== 'function' || cur[MARK]) return;
                    var inner = cur;
                    var wrapped = function(){
                        return inner.apply(c, Array.prototype.map.call(arguments, stringify));
                    };
                    wrapped[MARK] = true;
                    c[name] = wrapped;
                });
            }
            reassert();
            // Re-assert for a short window so we end up outermost once dx patches.
            var n = 0;
            var id = setInterval(function(){ reassert(); if (++n > 40) clearInterval(id); }, 50);
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
    // Serve the original photos at `/photos/<file>` and the H.264 transcodes of
    // the (HEVC) videos at `/media/<file>` — both directly, no copying.
    #[cfg(feature = "server")]
    dioxus::serve(|| async move {
        use tower_http::services::ServeDir;
        let router = dioxus::server::router(app::App)
            .nest_service("/photos", ServeDir::new(server_fns::PHOTOS_SRC_DIR))
            .nest_service("/media", ServeDir::new(server_fns::MEDIA_WEB_DIR));
        Ok(router)
    });

    // Everything else (plain web, or fullstack client build)
    #[cfg(not(feature = "server"))]
    dioxus::LaunchBuilder::new().launch(app::App);
}
