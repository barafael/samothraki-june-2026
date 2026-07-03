#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
    #[wasm_bindgen(js_namespace = console)]
    fn error(s: &str);
}

pub fn info(msg: &str) {
    #[cfg(target_arch = "wasm32")]
    log(msg);
    #[cfg(not(target_arch = "wasm32"))]
    println!("[info] {}", msg);
}

pub fn error_(msg: &str) {
    #[cfg(target_arch = "wasm32")]
    error(msg);
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("[error] {}", msg);
}
