//! The one provider trait plus the fallback chain.
//!
//! Resolution order (from PROJECT spec): NewPipe (Kotlin callback
//! implementation) -> yt-dlp (`snap-core`'s built-in extractor) -> sniff.
//! A failure in one provider falls through to the next.

use std::sync::Arc;

use async_trait::async_trait;

use crate::error::{SnapError, SnapResult};
use crate::models::MediaInfo;

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
    async fn no_provider_matches() {
        let chain = ProviderChain::new();
        assert!(chain.resolve("https://example.com/x").await.is_err());
    }
}
