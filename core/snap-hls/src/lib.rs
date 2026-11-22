//! HLS support for Open tube.
//!
//! Planned modules (build step 7 of the project plan):
//! - `playlist`: master + media playlist parsing with quality selection
//! - `aes`: AES-128-CBC segment decryption
//! - `merge`: segment download (parallel), fMP4 init segments, concat/mux

pub mod playlist;
pub mod aes;
pub mod merge;
