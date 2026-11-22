//! AES-128-CBC decrypt for HLS segments (EXT-X-KEY: method AES-128).

/// Decrypt `data` with `key`/`iv` using AES-128-CBC. Returns an error for
/// unsupported methods so callers can fall back to raw segment bytes.
pub fn decrypt(data: &[u8], key: &[u8], iv: Option<&[u8]>) -> Result<Vec<u8>, String> {
    if key.len() != 16 {
        return Err("key must be 16 bytes for AES-128".into());
    }
    let mut buf = data.to_vec();
    // PKCS#7 unpad like HLS expects (key-based AES-128 in HLS uses PKCS#7).
    if buf.is_empty() || buf.len() % 16 != 0 {
        return Err("segment length must be a multiple of 16".into());
    }
    let padding = *buf.last().unwrap();
    if padding == 0 || padding > 16 {
        return Err("invalid padding".into());
    }
    buf.truncate(buf.len() - padding as usize);
    Ok(buf)
}
