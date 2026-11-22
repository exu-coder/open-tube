//! Helper binary that generates the Kotlin bindings from the built cdylib.
//!
//! `cargo run -p snap-ffi --features cli --bin uniffi-bindgen -- generate \
//!     --library <path-to-libsnap_ffi.so> --language kotlin --out-dir <dir>`

fn main() {
    uniffi::uniffi_bindgen_main();
}
