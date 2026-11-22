//! The one provider trait plus the fallback chain.
//!
//! Resolution order (from PROJECT spec): NewPipe (Kotlin callback
//! implementation) -> yt-dlp (`snap-core`'s built-in extractor) -> sniff.
//! A failure in one provider falls through to the next.

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::{SnapError, SnapResult};
use crate::models::{MediaInfo, Stream, StreamKind};

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &str;
    fn can_handle(&self, url: &str) -> bool;
    async fn resolve(&self, url: &str) -> SnapResult<MediaInfo>;
}

#[derive(Default)]
pub struct ProviderChain {
    providers: Vec<Arc<dyn Provider>>,
}

impl ProviderChain {
    pub fn new() -> Self {
        Self {
            providers: Vec::new(),
        }
    }

    pub fn register(&mut self, provider: Arc<dyn Provider>) {
        self.providers.push(provider);
    }

    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }

    pub fn provider_names(&self) -> Vec<String> {
        self.providers
            .iter()
            .map(|p| p.name().to_string())
            .collect()
    }

    pub async fn resolve(&self, url: &str) -> SnapResult<MediaInfo> {
        for provider in &self.providers {
            if provider.can_handle(url) {
                match provider.resolve(url).await {
                    Ok(info) => return Ok(info),
                    Err(e) => {
                        tracing::warn!("provider {} failed for {}: {e}", provider.name(), url);
                    }
                }
            }
        }
        Err(SnapError::NoProvider(url.to_string()))
    }
}

/// Provider for direct media URLs (mp4/m3u8/mp3/...). This is the first
/// working end-to-end path: paste a media URL, get a single stream back.
pub struct DirectUrlProvider;

const DIRECT_EXTENSIONS: &[&str] = &[
    "mp4", "m4v", "mov", "webm", "mkv", "ts", "m3u8", "mp3", "m4a", "aac", "ogg", "opus", "wav",
];

fn url_extension(url: &str) -> String {
    let without_query = url.split(['?', '#']).next().unwrap_or(url);
    let filename = without_query.rsplit('/').next().unwrap_or("");
    filename
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default()
}

impl DirectUrlProvider {
    pub fn new() -> Self {
        Self
    }
}

impl Default for DirectUrlProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for DirectUrlProvider {
    fn name(&self) -> &str {
        "direct"
    }

    fn can_handle(&self, url: &str) -> bool {
        DIRECT_EXTENSIONS.contains(&url_extension(url).as_str())
    }

    async fn resolve(&self, url: &str) -> SnapResult<MediaInfo> {
        if !self.can_handle(url) {
            return Err(SnapError::UnsupportedSite(url.to_string()));
        }
        let ext = url_extension(url);
        let without_query = url.split(['?', '#']).next().unwrap_or(url);
        let title = without_query
            .rsplit('/')
            .next()
            .unwrap_or("media")
            .to_string();
        let kind = if ext == "mp3"
            || ext == "m4a"
            || ext == "aac"
            || ext == "ogg"
            || ext == "opus"
            || ext == "wav"
        {
            StreamKind::Audio
        } else {
            StreamKind::Muxed
        };
        Ok(MediaInfo {
            id: format!("direct-{title}"),
            title: title.clone(),
            uploader: "Direct URL".into(),
            duration_s: None,
            thumbnail: None,
            source_url: url.to_string(),
            streams: vec![Stream {
                url: url.to_string(),
                kind,
                container: Some(ext),
                codec: None,
                width: None,
                height: None,
                bitrate_bps: None,
                size_bytes: None,
                headers: Default::default(),
            }],
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StubProvider {
        name: &'static str,
        handles: bool,
        should_fail: bool,
    }

    #[async_trait]
    impl Provider for StubProvider {
        fn name(&self) -> &str {
            self.name
        }

        fn can_handle(&self, _url: &str) -> bool {
            self.handles
        }

        async fn resolve(&self, _url: &str) -> SnapResult<MediaInfo> {
            if self.should_fail {
                return Err(SnapError::Network("boom".into()));
            }
            Ok(MediaInfo {
                id: "stub".into(),
                title: "Stub".into(),
                uploader: "Stubber".into(),
                duration_s: None,
                thumbnail: None,
                source_url: "https://example.com/stub".into(),
                streams: Vec::new(),
            })
        }
    }

    #[tokio::test]
    async fn falls_through_on_failure() {
        let mut chain = ProviderChain::new();
        chain.register(Arc::new(StubProvider {
            name: "first",
            handles: true,
            should_fail: true,
        }));
        chain.register(Arc::new(StubProvider {
            name: "second",
            handles: true,
            should_fail: false,
        }));
        let info = chain.resolve("https://example.com/stub").await.unwrap();
        assert_eq!(info.title, "Stub");
    }

    #[tokio::test]
    async fn direct_url_provider() {
        let p = DirectUrlProvider::new();
        assert!(p.can_handle("https://cdn.example/video.mp4?token=1"));
        assert!(p.can_handle("https://cdn.example/master.m3u8"));
        assert!(!p.can_handle("https://example.com/watch?v=1"));
        let info = p
            .resolve("https://cdn.example/song.mp3?x=1")
            .await
            .expect("resolve");
        assert_eq!(info.title, "song.mp3");
        assert_eq!(info.streams[0].kind, StreamKind::Audio);
        assert_eq!(info.streams[0].container.as_deref(), Some("mp3"));
    }

    #[tokio::test]
    async fn no_provider_matches() {
        let chain = ProviderChain::new();
        assert!(chain.resolve("https://example.com/x").await.is_err());
    }
}
