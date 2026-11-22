//! Segment download + concatenation/muxing for HLS.

/// Placeholder for the segment download and merge pipeline.
pub fn merge_segments(_segment_bytes: Vec<Vec<u8>>) -> Result<Vec<u8>, String> {
    Err("merge is implemented in the download engine (build step 7)".into())
}
