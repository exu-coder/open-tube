//! AES-128-CBC decrypt for HLS segments (EXT-X-KEY: method AES-128).

use aes::Aes128;
use cbc::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};

type Aes128CbcDec = cbc::Decryptor<Aes128>;

/// Default IV for HLS when EXT-X-KEY carries no IV: the segment's media
/// sequence number as a 16-byte big-endian integer (RFC 8216 section 5.2).
fn sequence_iv(sequence: u64) -> [u8; 16] {
    let mut iv = [0u8; 16];
    iv[8..].copy_from_slice(&sequence.to_be_bytes());
    iv
}

/// Decrypt one AES-128-CBC segment.
///
/// `iv` is the EXT-X-KEY IV when present; otherwise the segment's media
/// sequence number supplies the default IV. PKCS#7 padding is removed and
/// fully validated by `decrypt_padded_mut`, so corrupt data fails instead of
/// silently producing a truncated file.
pub fn decrypt(
    data: &[u8],
    key: &[u8],
    iv: Option<&[u8]>,
    sequence: u64,
) -> Result<Vec<u8>, String> {
    if key.len() != 16 {
        return Err(format!("AES-128 key must be 16 bytes, got {}", key.len()));
    }
    if data.is_empty() {
        return Err("empty segment: nothing to decrypt".into());
    }
    if !data.len().is_multiple_of(16) {
        return Err(format!(
            "ciphertext length {} is not a multiple of 16 (seq {sequence})",
            data.len()
        ));
    }
    let iv_bytes: [u8; 16] = match iv {
        Some(v) if v.len() == 16 => v.try_into().expect("length checked above"),
        Some(v) => return Err(format!("IV must be 16 bytes, got {}", v.len())),
        None => sequence_iv(sequence),
    };

    let mut buf = data.to_vec();
    let plain = Aes128CbcDec::new_from_slices(key, &iv_bytes)
        .map_err(|e| format!("bad key/iv: {e}"))?
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|_| format!("AES-128-CBC decrypt failed (bad padding, seq {sequence})"))?;
    Ok(plain.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 16] = [
        0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6, 0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf, 0x4f,
        0x3c,
    ];
    const IV: [u8; 16] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f,
    ];

    /// NIST SP 800-38A F.2.1 AES-128-CBC known-answer test.
    #[test]
    fn aes128_cbc_known_answer() {
        let ciphertext: [u8; 16] = [
            0x76, 0x49, 0xab, 0xac, 0x81, 0x19, 0xb2, 0x46, 0xce, 0xe9, 0x8e, 0x9b, 0x12, 0xe9,
            0x19, 0x7d,
        ];
        let expected: [u8; 16] = [
            0x6b, 0xc1, 0xbe, 0xe2, 0x2e, 0x40, 0x9f, 0x96, 0xe9, 0x3d, 0x7e, 0x11, 0x73, 0x93,
            0x17, 0x2a,
        ];
        let plain = decrypt(&ciphertext, &KEY, Some(&IV), 0).expect("known answer must decrypt");
        assert_eq!(plain, expected);
    }

    #[test]
    fn default_iv_is_sequence_number() {
        assert_eq!(sequence_iv(0), [0u8; 16]);
        assert_eq!(sequence_iv(5)[15], 5);
        assert_eq!(&sequence_iv(5)[..15], &[0u8; 15]);
    }

    #[test]
    fn rejects_bad_inputs() {
        assert!(decrypt(&[], &KEY, None, 0).is_err(), "empty input");
        assert!(
            decrypt(&[0u8; 15], &KEY, None, 0).is_err(),
            "not block aligned"
        );
        assert!(
            decrypt(&[0u8; 16], &[0u8; 15], None, 0).is_err(),
            "short key"
        );
        assert!(
            decrypt(&[0u8; 16], &KEY, Some(&[0u8; 8]), 0).is_err(),
            "short iv"
        );
        assert!(decrypt(&[0u8; 16], &KEY, None, 0).is_err(), "bad padding");
    }
}
