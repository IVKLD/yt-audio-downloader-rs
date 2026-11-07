use std::{fmt, path::Path, pin::Pin, sync::Arc};

use bytes::Bytes;
use futures_util::Stream;
use tokio::sync::RwLock;

use crate::{
    client::builder::ClientBuilder,
    downloader::YoutubeAudioDownloader,
    error::YoutubeAudioError,
    extractor::{InnertubeExtractor, YoutubeExtractor},
    http::create_http_client_with_proxy,
    models::{AudioFormat, AudioQuality, AudioStreamResponse, DownloadedAudio, VideoMetadata},
    progress::{ProgressCallback, ProgressEvent},
    streamer::AudioStreamer,
};

#[derive(Clone)]
pub struct ProxyUrlProvider(pub Arc<dyn Fn() -> Option<String> + Send + Sync>);

impl fmt::Debug for ProxyUrlProvider {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ProxyUrlProvider")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryConfig {
    pub max_retries: u32,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self { max_retries: 3 }
    }
}

/// YouTube audio client managing persistent HTTP sessions, dynamic proxies, and extraction workflows.
pub struct Client {
    pub http_client: RwLock<reqwest::Client>,
    pub proxy_url: Option<String>,
    pub proxy_provider: Option<ProxyUrlProvider>,
    pub active_proxy: RwLock<Option<String>>,
    pub retry_config: RetryConfig,
    pub prefer_yt_dlp: bool,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("proxy_url", &self.proxy_url)
            .field("proxy_provider", &self.proxy_provider)
            .field("active_proxy", &self.active_proxy)
            .field("retry_config", &self.retry_config)
            .field("prefer_yt_dlp", &self.prefer_yt_dlp)
            .finish()
    }
}

impl Client {
    /// Create a new `Client` with default settings.
    pub async fn new() -> Result<Self, YoutubeAudioError> {
        ClientBuilder::new().build().await
    }

    pub(crate) async fn from_builder(builder: ClientBuilder) -> Result<Self, YoutubeAudioError> {
        let http_client = Self::build_http_client(builder.proxy_url.as_deref())?;
        let active_proxy = builder.proxy_url.clone();

        Ok(Self {
            http_client: RwLock::new(http_client),
            proxy_url: builder.proxy_url,
            proxy_provider: builder.proxy_provider,
            active_proxy: RwLock::new(active_proxy),
            retry_config: builder.retry_config,
            prefer_yt_dlp: builder.prefer_yt_dlp,
        })
    }

    /// Attach a dynamic proxy provider to this client.
    pub fn with_proxy_provider(mut self, provider: ProxyUrlProvider) -> Self {
        self.proxy_provider = Some(provider);
        self
    }

    /// Checks if the proxy provider returns a new proxy URL, and transparently updates the HTTP client if needed.
    pub async fn ensure_proxy_refreshed(&self) -> Result<(), YoutubeAudioError> {
        if let Some(ref provider) = self.proxy_provider {
            let next_proxy = (provider.0)();
            let current_proxy = self.active_proxy.read().await.clone();

            if next_proxy != current_proxy {
                let mut active_guard = self.active_proxy.write().await;
                if next_proxy != *active_guard {
                    let new_client = Self::build_http_client(next_proxy.as_deref())?;
                    let mut http_guard = self.http_client.write().await;
                    *http_guard = new_client;
                    *active_guard = next_proxy;
                }
            }
        }
        Ok(())
    }

    pub fn build_http_client(
        proxy_url: Option<&str>,
    ) -> Result<reqwest::Client, YoutubeAudioError> {
        Ok(create_http_client_with_proxy(proxy_url))
    }

    /// Get the currently active proxy URL (if any).
    pub async fn current_proxy_url(&self) -> Option<String> {
        self.active_proxy.read().await.clone()
    }

    /// Get a clone of the internal `reqwest::Client`.
    pub async fn get_http_client(&self) -> reqwest::Client {
        self.http_client.read().await.clone()
    }

    /// Perform a quick check to see if YouTube is accessible.
    pub async fn health_check(&self) -> bool {
        self.fetch_metadata("dQw4w9WgXcQ").await.is_ok()
    }

    /// Fetch metadata for a YouTube video URL or ID.
    pub async fn fetch_metadata(
        &self,
        url_or_id: &str,
    ) -> Result<VideoMetadata, YoutubeAudioError> {
        self.ensure_proxy_refreshed().await?;
        let client = self.get_http_client().await;
        let proxy = self.current_proxy_url().await;
        let extractor = YoutubeExtractor::with_proxy(client, proxy);
        let media = extractor.extract_media(url_or_id).await?;
        Ok(media.metadata)
    }

    /// Fetch all video items in a YouTube playlist.
    pub async fn fetch_playlist(
        &self,
        url_or_id: &str,
    ) -> Result<Vec<VideoMetadata>, YoutubeAudioError> {
        self.ensure_proxy_refreshed().await?;
        let client = self.get_http_client().await;
        let extractor = InnertubeExtractor::new(client);
        extractor.fetch_playlist(url_or_id).await
    }

    /// Search YouTube for videos matching a query.
    pub async fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<VideoMetadata>, YoutubeAudioError> {
        self.ensure_proxy_refreshed().await?;
        let client = self.get_http_client().await;
        let extractor = InnertubeExtractor::new(client);
        extractor.search(query, limit).await
    }

    /// Search YouTube with pagination support (continuation token).
    pub async fn search_page(
        &self,
        query: &str,
        continuation: Option<&str>,
    ) -> Result<(Vec<VideoMetadata>, Option<String>), YoutubeAudioError> {
        self.ensure_proxy_refreshed().await?;
        let client = self.get_http_client().await;
        let extractor = InnertubeExtractor::new(client);
        extractor.search_page(query, continuation).await
    }

    /// Resolve audio stream info and direct media URL.
    pub async fn resolve_stream(
        &self,
        url_or_id: &str,
    ) -> Result<AudioStreamResponse, YoutubeAudioError> {
        self.ensure_proxy_refreshed().await?;
        let client = self.get_http_client().await;
        let proxy = self.current_proxy_url().await;
        AudioStreamer::with_proxy(client, proxy)
            .get_stream_response(url_or_id)
            .await
    }

    /// Stream audio bytes directly.
    pub async fn stream_bytes(
        &self,
        url_or_id: &str,
    ) -> Result<
        (
            VideoMetadata,
            Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
        ),
        YoutubeAudioError,
    > {
        self.ensure_proxy_refreshed().await?;
        let client = self.get_http_client().await;
        let proxy = self.current_proxy_url().await;
        AudioStreamer::with_proxy(client, proxy)
            .stream_bytes(url_or_id)
            .await
    }

    /// Download and convert audio to a specific output file path.
    pub async fn download(
        &self,
        url_or_id: &str,
        output_file: &Path,
        format: AudioFormat,
        on_progress: Option<&ProgressCallback>,
    ) -> Result<DownloadedAudio, YoutubeAudioError> {
        self.ensure_proxy_refreshed().await?;
        let client = self.get_http_client().await;
        let proxy = self.current_proxy_url().await;

        let mut downloader = YoutubeAudioDownloader::new()
            .client(client)
            .output_file(output_file)
            .format(format)
            .quality(AudioQuality::Best)
            .prefer_yt_dlp(self.prefer_yt_dlp);

        if let Some(p) = proxy {
            downloader = downloader.proxy(p);
        }

        if let Some(cb) = on_progress {
            let cb = cb.clone();
            downloader = downloader.on_progress(move |event| {
                if let ProgressEvent::Downloading {
                    bytes_downloaded,
                    total_bytes,
                    ..
                } = event
                {
                    cb(bytes_downloaded, total_bytes);
                }
            });
        }

        downloader.download(url_or_id).await
    }
}
