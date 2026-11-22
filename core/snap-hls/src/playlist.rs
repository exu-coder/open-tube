//! HLS playlist parsing (media playlist with EXT-X-KEY, EXT-X-MAP, etc.).

/// A parsed media playlist with its download segments.
pub struct MediaPlaylist {
    pub segments: Vec<MediaSegment>,
    pub is_master: bool,
}

/// One media segment from a playlist.
pub struct MediaSegment {
    pub uri: String,
    pub duration: f64,
}

impl MediaPlaylist {
    /// Parse the playlist from `text` (EXT-X-VERSION 3+ bodies).
    pub fn parse(text: &str) -> Self {
        MediaPlaylist {
            segments: Vec::new(),
            is_master: false,
        }
    }
}

/// Parse a master playlist's variant URIs.
pub fn parse_master(text: &str) -> Vec<String> {
    Vec::new()
}
