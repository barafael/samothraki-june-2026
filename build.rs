//! Bake ASSET_BASE_URL into the build by generating a Rust source file that
//! src/config.rs `include!`s.
//!
//! Why not `option_env!`/`cargo:rustc-env`: `dx build`'s multi-phase compile +
//! wasm-bindgen pipeline does not reliably surface a shell env var (or a build
//! script's rustc-env) to `option_env!` in the final wasm. Writing a generated
//! source file that the crate `include!`s is compile-input, so it is always
//! picked up under both `cargo` and `dx`.
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-env-changed=ASSET_BASE_URL");
    let url = std::env::var("ASSET_BASE_URL").unwrap_or_default();
    // Escape via {:?} so quotes/backslashes in the URL can't break the source.
    let generated = format!("pub const ASSET_BASE_URL: &str = {url:?};\n");
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("asset_base_url.rs");
    std::fs::write(&out, generated).expect("write asset_base_url.rs");
}
