# Open Tube

An ad-free Android video/music downloader: a Rust core (`snap-core`, `snap-hls`,
`snap-ffi`) with a Kotlin UI. The Rust library is exposed to Kotlin through
UniFFI; the native libraries and generated bindings are produced during the
build, not committed.

Not affiliated with SnapTube.

[![CI](https://github.com/exu-coder/open-tube/actions/workflows/ci.yml/badge.svg)](https://github.com/exu-coder/open-tube/actions/workflows/ci.yml)

## Build

Prerequisites: Rust (see `rust-toolchain.toml`), JDK 21, Android SDK platform
36 + build-tools 36.0.0 + NDK 28.2.13676358, Gradle 8.14.3, and `cargo-ndk`.

```bash
# Rust checks and tests
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace

# Kotlin bindings + native libs
cargo build -p snap-ffi
cargo run -p snap-ffi --features cli --bin uniffi-bindgen -- generate \
  --library target/debug/libsnap_ffi.so \
  --language kotlin --out-dir app/build/generated/uniffi --no-format

export ANDROID_NDK_HOME="$ANDROID_HOME/ndk/28.2.13676358"
export CARGO_NDK_PLATFORM=26
cargo ndk -t arm64-v8a -t armeabi-v7a -t x86_64 \
  -o app/src/main/jniLibs build --release -p snap-ffi

# APK (app/build/outputs/apk/debug/)
gradle assembleDebug
```

The same steps run automatically in GitHub Actions on every push and PR; the
debug APK is attached to each successful run as an artifact.

## Status

The queue, SQLite persistence, HLS playlist parsing and AES-128 decryption are
implemented. The HLS segment download/merge pipeline and real provider
extractors are still under construction; `hello()` is the current FFI smoke
test.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
