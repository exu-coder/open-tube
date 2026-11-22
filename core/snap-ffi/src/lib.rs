//! Open tube: UniFFI interface, callbacks, runtime setup.
//!
//! Kotlin bindings are generated from the built cdylib into
//! `app/build/generated/uniffi` (package `com.opentube.ffi`); the generated
//! code loads `libsnap_ffi.so` through JNA.

// Pin the namespace so the generated object is always `SnapFfi`.
uniffi::setup_scaffolding!("snap_ffi");

/// Step 1 smoke test: hello callable from Kotlin.
#[uniffi::export]
pub fn hello() -> String {
    tracing::info!("snap-ffi hello");
    "hello from Open tube".to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn hello_is_stable() {
        assert_eq!(super::hello(), "hello from Open tube");
    }
}
