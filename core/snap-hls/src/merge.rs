//! Segment concatenation / muxing for HLS.
//!
//! Not implemented yet: the download engine owns segment ordering, `EXT-X-MAP`
//! init sections and container-specific joining (fMP4 needs the init segment
//! prepended; MPEG-TS needs continuity counters handled). This module will host
//! that logic so the engine stays container-agnostic.

/// Concatenate already-ordered segment payloads.
///
/// Only correct for byte-concatenable formats (fMP4 with the init section
/// first). Returns an error until the container-aware implementation lands.
pub fn merge_segments(_segment_bytes: Vec<Vec<u8>>) -> Result<Vec<u8>, String> {
    Err("segment merging is not implemented yet (HLS download engine, build step 7)".into())
}
