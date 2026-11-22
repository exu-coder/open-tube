//! Open tube: Rust core of the Android video/music downloader.
//!
//! - `models`: MediaInfo, Stream, DownloadTask (UniFFI boundary types)
//! - `provider`: the one Provider trait + the fallback chain
//! - `queue`: the durable SQLite task queue (restored after restart)
//! - `db`: SQLite schema + open helpers
//! - `error`: typed errors with readable messages for the Kotlin UI
//!
//! HLS, the HTTP engine and provider implementations are added in later
//! build steps (see PROJECT spec, build order).

pub mod db;
pub mod error;
pub mod models;
pub mod provider;
pub mod queue;

pub use error::{SnapError, SnapResult};
pub use models::{DownloadState, DownloadTask, MediaInfo, Stream, StreamKind};
pub use provider::{Provider, ProviderChain};
pub use queue::Queue;

/// Maximum number of concurrent download tasks (PROJECT spec).
pub const MAX_CONCURRENT_TASKS: usize = 3;
