//! UniFFI surface of the Open tube download engine.
//!
//! Kotlin passes `MediaInfo`/`Stream` as JSON (see `prompt.md`-era design:
//! those types are serde records, not UniFFI records), receives task ids and
//! progress events as JSON, and drives everything from a background thread.

use std::sync::Arc;
use std::sync::OnceLock;

use snap_core::{DirectUrlProvider, MediaInfo, ProviderChain, SnapError, Stream};
use snap_engine::Engine;

// Pin the namespace so generated Kotlin lives in `uniffi.snap_ffi`.
uniffi::setup_scaffolding!("snap_ffi");

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiError {
    #[error("{0}")]
    #[uniffi(flat_error)]
    Message(String),
}

impl From<SnapError> for FfiError {
    fn from(value: SnapError) -> Self {
        Self::Message(value.to_string())
    }
}

impl From<serde_json::Error> for FfiError {
    fn from(value: serde_json::Error) -> Self {
        Self::Message(value.to_string())
    }
}

impl From<std::io::Error> for FfiError {
    fn from(value: std::io::Error) -> Self {
        Self::Message(value.to_string())
    }
}

/// Reports download progress (JSON-encoded `Progress`) back to Kotlin.
#[uniffi::export(callback_interface)]
pub trait ProgressCallback: Send + Sync {
    fn on_progress(&self, event_json: String);
}

static ENGINE: OnceLock<Engine> = OnceLock::new();

fn engine() -> Result<&'static Engine, FfiError> {
    ENGINE
        .get()
        .ok_or_else(|| FfiError::Message("engine is not initialized".into()))
}

/// Call once at app startup with writable paths (e.g. `filesDir`).
#[uniffi::export]
pub fn init_engine(db_path: String, output_dir: String) -> Result<(), FfiError> {
    let engine = Engine::new(
        std::path::Path::new(&db_path),
        std::path::Path::new(&output_dir),
    )?;
    ENGINE
        .set(engine)
        .map_err(|_| FfiError::Message("engine already initialized".into()))
}

/// Resolve a direct media URL (mp4/m3u8/mp3/...) into `MediaInfo` JSON.
#[uniffi::export]
pub fn resolve_url(url: String) -> Result<String, FfiError> {
    let mut chain = ProviderChain::new();
    chain.register(Arc::new(DirectUrlProvider::new()));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let info: MediaInfo = runtime.block_on(chain.resolve(&url))?;
    Ok(serde_json::to_string(&info)?)
}

/// Persist a task; returns its id. Pass `[]` for streams to use all streams
/// from the media info.
#[uniffi::export]
pub fn enqueue(media_json: String, streams_json: String) -> Result<String, FfiError> {
    let media: MediaInfo = serde_json::from_str(&media_json)?;
    let streams: Vec<Stream> = serde_json::from_str(&streams_json)?;
    Ok(engine()?.enqueue(media, streams)?.id)
}

/// Download every queued task. BLOCKING: call from a background thread.
#[uniffi::export]
pub fn run_pending(callback: Box<dyn ProgressCallback>) -> Result<(), FfiError> {
    let engine = engine()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(engine.run_pending(move |progress| {
        if let Ok(json) = serde_json::to_string(&progress) {
            callback.on_progress(json);
        }
    }))?;
    Ok(())
}

#[uniffi::export]
pub fn cancel_task(id: String) -> Result<(), FfiError> {
    engine()?.cancel(&id);
    Ok(())
}

/// All persisted tasks as JSON.
#[uniffi::export]
pub fn list_tasks() -> Result<String, FfiError> {
    Ok(serde_json::to_string(&engine()?.list()?)?)
}

/// Kept from the smoke-test build; harmless and useful to verify bindings.
#[uniffi::export]
pub fn hello() -> String {
    "hello from Open tube".to_string()
}
