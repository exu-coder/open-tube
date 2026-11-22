//! HLS support for Open tube.
//!
//! - [`playlist`]: master + media playlist parsing with URI resolution
//! - [`aes`]: AES-128-CBC segment decryption (PKCS#7 validated)
//! - [`merge`]: segment concatenation / muxing (not implemented yet)

pub mod aes;
pub mod merge;
pub mod playlist;
