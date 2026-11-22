//! Typed errors for the core. Each variant maps to a stable, readable
//! message string shown in the Kotlin UI.

use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SnapError {
    #[error("no network connection")]
    NoNetwork,
    #[error("network error: {0}")]
    Network(String),
    #[error("unsupported site or url: {0}")]
    UnsupportedSite(String),
    #[error("extractor is outdated; please update the app")]
    ExtractorOutdated,
    #[error("http status {0}")]
    HttpStatus(u16),
    #[error("storage full or cannot write file: {0}")]
    StorageFull(String),
    #[error("io error: {0}")]
    Io(#[from] io::Error),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("no provider could resolve {0}")]
    NoProvider(String),
    #[error("internal error: {0}")]
    Internal(String),
}

pub type SnapResult<T> = Result<T, SnapError>;
