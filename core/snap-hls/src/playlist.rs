//! HLS playlist parsing on top of `m3u8-rs`.
//!
//! Relative URIs (segments, keys, init sections, variants) are resolved
//! against the playlist URL, because HLS bodies are almost always relative.

use m3u8_rs::playlist::{ByteRange as M3u8ByteRange, Key, Playlist};
use snap_core::{SnapError, SnapResult};
use url::Url;

/// AES-128 key reference for a segment (`EXT-X-KEY`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyInfo {
    /// Absolute key URI (the segment/playlist URL resolved).
    pub uri: String,
    /// Explicit IV from the playlist; `None` means "use the media sequence
    /// number as the IV" (RFC 8216 section 5.2).
    pub iv: Option<[u8; 16]>,
}

/// Byte range of a segment or init section (`EXT-X-BYTERANGE`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteRange {
    pub length: u64,
    pub offset: Option<u64>,
}

/// One media segment.
#[derive(Clone, Debug)]
pub struct MediaSegment {
    /// Absolute segment URI.
    pub uri: String,
    pub duration_s: f64,
    /// Media sequence number (`EXT-X-MEDIA-SEQUENCE` + index) used for the
    /// default IV and for resuming partial downloads.
    pub sequence: u64,
    pub key: Option<KeyInfo>,
    /// Absolute URI of the `EXT-X-MAP` init section, if any.
    pub init: Option<String>,
    pub byte_range: Option<ByteRange>,
}

/// A parsed media playlist.
#[derive(Clone, Debug)]
pub struct MediaPlaylist {
    pub target_duration_s: f64,
    pub media_sequence: u64,
    pub end_list: bool,
    pub segments: Vec<MediaSegment>,
}

/// One entry of a master playlist.
#[derive(Clone, Debug)]
pub struct Variant {
    pub uri: String,
    pub bandwidth_bps: Option<u64>,
    pub resolution: Option<(u32, u32)>,
    pub codecs: Option<String>,
}

/// Parse a media playlist, resolving every URI against `base`.
pub fn parse(text: &str, base: &Url) -> SnapResult<MediaPlaylist> {
    require_header(text)?;
    match m3u8_rs::parse_playlist_res(text.as_bytes()) {
        Ok(Playlist::MediaPlaylist(p)) => {
            let media_sequence = p.media_sequence.max(0) as u64;
            let mut segments = Vec::with_capacity(p.segments.len());
            for (index, seg) in p.segments.iter().enumerate() {
                segments.push(MediaSegment {
                    uri: join(base, &seg.uri)?,
                    duration_s: f64::from(seg.duration),
                    sequence: media_sequence + index as u64,
                    key: seg.key.as_ref().map(|k| key_info(base, k)).transpose()?,
                    init: seg.map.as_ref().map(|m| join(base, &m.uri)).transpose()?,
                    byte_range: seg.byte_range.as_ref().map(byte_range),
                });
            }
            if segments.is_empty() {
                return Err(SnapError::Parse("media playlist has no segments".into()));
            }
            Ok(MediaPlaylist {
                target_duration_s: f64::from(p.target_duration),
                media_sequence,
                end_list: p.end_list,
                segments,
            })
        }
        Ok(Playlist::MasterPlaylist(_)) => Err(SnapError::Parse(
            "expected a media playlist, got a master playlist".into(),
        )),
        Err(e) => Err(SnapError::Parse(format!("invalid media playlist: {e:?}"))),
    }
}

/// Parse a master playlist, resolving variant URIs against `base`.
pub fn parse_master(text: &str, base: &Url) -> SnapResult<Vec<Variant>> {
    require_header(text)?;
    match m3u8_rs::parse_playlist_res(text.as_bytes()) {
        Ok(Playlist::MasterPlaylist(m)) => {
            let variants: Vec<Variant> = m
                .variants
                .iter()
                .map(|v| {
                    Ok(Variant {
                        uri: join(base, &v.uri)?,
                        bandwidth_bps: v.bandwidth.trim().parse().ok(),
                        resolution: v.resolution.as_deref().and_then(parse_resolution),
                        codecs: v.codecs.clone(),
                    })
                })
                .collect::<SnapResult<_>>()?;
            if variants.is_empty() {
                return Err(SnapError::Parse("master playlist has no variants".into()));
            }
            Ok(variants)
        }
        Ok(Playlist::MediaPlaylist(_)) => Err(SnapError::Parse(
            "expected a master playlist, got a media playlist".into(),
        )),
        Err(e) => Err(SnapError::Parse(format!("invalid master playlist: {e:?}"))),
    }
}

/// m3u8-rs accepts arbitrary text as an empty playlist; require the HLS
/// header so garbage input cannot silently become "0 segments downloaded".
fn require_header(text: &str) -> SnapResult<()> {
    if !text.trim_start().starts_with("#EXTM3U") {
        return Err(SnapError::Parse(
            "playlist is missing the #EXTM3U header".into(),
        ));
    }
    Ok(())
}

fn join(base: &Url, reference: &str) -> SnapResult<String> {
    base.join(reference)
        .map(|u| u.to_string())
        .map_err(|e| SnapError::Parse(format!("bad uri {reference:?}: {e}")))
}

fn key_info(base: &Url, key: &Key) -> SnapResult<KeyInfo> {
    if !key.method.eq_ignore_ascii_case("AES-128") {
        return Err(SnapError::UnsupportedSite(format!(
            "unsupported HLS encryption method {:?}",
            key.method
        )));
    }
    let uri = key
        .uri
        .as_deref()
        .ok_or_else(|| SnapError::Parse("EXT-X-KEY without URI".into()))?;
    Ok(KeyInfo {
        uri: join(base, uri)?,
        iv: key.iv.as_deref().map(parse_iv).transpose()?,
    })
}

/// Parse a 128-bit hex IV (`0x...`), left-padding short values.
fn parse_iv(raw: &str) -> SnapResult<[u8; 16]> {
    let hex = raw
        .strip_prefix("0x")
        .or_else(|| raw.strip_prefix("0X"))
        .unwrap_or(raw);
    if hex.is_empty() || hex.len() > 32 || !hex.len().is_multiple_of(2) {
        return Err(SnapError::Parse(format!("invalid EXT-X-KEY IV {raw:?}")));
    }
    let mut iv = [0u8; 16];
    let bytes = hex.len() / 2;
    for i in 0..bytes {
        let byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16)
            .map_err(|_| SnapError::Parse(format!("invalid EXT-X-KEY IV {raw:?}")))?;
        iv[16 - bytes + i] = byte;
    }
    Ok(iv)
}

fn byte_range(r: &M3u8ByteRange) -> ByteRange {
    ByteRange {
        length: r.length.max(0) as u64,
        offset: r.offset.map(|o| o.max(0) as u64),
    }
}

fn parse_resolution(raw: &str) -> Option<(u32, u32)> {
    let (w, h) = raw.split_once('x')?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Url {
        Url::parse("https://cdn.example/hls/x.m3u8").unwrap()
    }

    #[test]
    fn parses_media_playlist_with_key_and_sequence() {
        let text = "#EXTM3U\n\
                    #EXT-X-VERSION:3\n\
                    #EXT-X-TARGETDURATION:4\n\
                    #EXT-X-MEDIA-SEQUENCE:7\n\
                    #EXT-X-KEY:METHOD=AES-128,URI=\"key.bin\",IV=0x0102030405060708090a0b0c0d0e0f10\n\
                    #EXTINF:4.0,\nseg0.ts\n\
                    #EXTINF:3.5,\nsub/seg1.ts\n\
                    #EXT-X-ENDLIST\n";
        let pl = parse(text, &base()).expect("media playlist must parse");
        assert_eq!(pl.segments.len(), 2);
        assert_eq!(pl.media_sequence, 7);
        assert!(pl.end_list);
        assert_eq!(pl.segments[0].uri, "https://cdn.example/hls/seg0.ts");
        assert_eq!(pl.segments[1].uri, "https://cdn.example/hls/sub/seg1.ts");
        assert_eq!(pl.segments[0].sequence, 7);
        assert_eq!(pl.segments[1].sequence, 8);
        let key = pl.segments[0].key.as_ref().expect("key present");
        assert_eq!(key.uri, "https://cdn.example/hls/key.bin");
        assert_eq!(key.iv.expect("iv parsed")[0], 0x01);
    }

    #[test]
    fn parses_master_playlist_variants() {
        let text = "#EXTM3U\n\
                    #EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,CODECS=\"avc1.42e01e\"\n\
                    low/index.m3u8\n\
                    #EXT-X-STREAM-INF:BANDWIDTH=2400000,RESOLUTION=1280x720\n\
                    https://other.example/hd.m3u8\n";
        let variants = parse_master(text, &base()).expect("master playlist must parse");
        assert_eq!(variants.len(), 2);
        assert_eq!(variants[0].uri, "https://cdn.example/hls/low/index.m3u8");
        assert_eq!(variants[0].bandwidth_bps, Some(800_000));
        assert_eq!(variants[0].resolution, Some((640, 360)));
        assert_eq!(variants[1].uri, "https://other.example/hd.m3u8");
    }

    #[test]
    fn rejects_wrong_playlist_kind_and_garbage() {
        let master = "#EXTM3U\n#EXT-X-STREAM-INF:BANDWIDTH=1\nv.m3u8\n";
        assert!(parse(master, &base()).is_err());
        let media = "#EXTM3U\n#EXT-X-TARGETDURATION:4\n#EXTINF:4,\na.ts\n";
        assert!(parse_master(media, &base()).is_err());
        assert!(parse("not a playlist", &base()).is_err());
    }

    #[test]
    fn parses_short_and_long_ivs() {
        assert_eq!(parse_iv("0x1f").unwrap(), {
            let mut v = [0u8; 16];
            v[15] = 0x1f;
            v
        });
        assert!(parse_iv("0x").is_err());
        assert!(parse_iv("0xzz").is_err());
        assert!(parse_iv("0x123").is_err());
    }
}
