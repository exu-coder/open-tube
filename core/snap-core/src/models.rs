//! Shared data models exposed to the Kotlin side through UniFFI.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StreamKind {
    Video,
    Audio,
    Muxed,
}

/// One downloadable stream (video, audio, or pre-muxed file).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Stream {
    pub url: String,
    pub kind: StreamKind,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bitrate_bps: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub headers: HashMap<String, String>,
}

/// Everything the UI needs to present a video or music item.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MediaInfo {
    pub id: String,
    pub title: String,
    pub uploader: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_s: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumbnail: Option<String>,
    pub source_url: String,
    pub streams: Vec<Stream>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DownloadState {
    Queued,
    Running,
    Paused,
    Merging,
    Done,
    Failed,
}

impl DownloadState {
    pub fn as_str(&self) -> &'static str {
        match self {
            DownloadState::Queued => "queued",
            DownloadState::Running => "running",
            DownloadState::Paused => "paused",
            DownloadState::Merging => "merging",
            DownloadState::Done => "done",
            DownloadState::Failed => "failed",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        Some(match s {
            "queued" => DownloadState::Queued,
            "running" => DownloadState::Running,
            "paused" => DownloadState::Paused,
            "merging" => DownloadState::Merging,
            "done" => DownloadState::Done,
            "failed" => DownloadState::Failed,
            _ => return None,
        })
    }
}

/// A persistent download task as stored in SQLite.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DownloadTask {
    pub id: String,
    pub media_info: MediaInfo,
    pub chosen_streams: Vec<Stream>,
    pub state: DownloadState,
    pub progress: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_bps: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eta_s: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}
