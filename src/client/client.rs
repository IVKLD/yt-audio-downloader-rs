use std::{fmt, path::Path, sync::Arc};

use tokio::sync::RwLock;

use crate::{
    downloader::YoutubeAudioDownloader,
    error::YoutubeAudioError,
    extractor::{InnertubeExtractor, YoutubeExtractor},
    http::create_http_client_with_proxy,
    models::{AudioFormat, AudioQuality, AudioStreamResponse, DownloadedAudio, VideoMetadata},
    progress::ProgressCallback,
    streamer::{AudioByteStream, AudioStreamer},
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

/// Builder for configuring and initializing a [`Client`] instance.
#[derive(Debug, Default)]
pub struct ClientBuilder {
    pub retry_config: RetryConfig,
    pub proxy_url: Option<String>,
    pub proxy_provider: Option<ProxyUrlProvider>,
    pub prefer_yt_dlp: bool,
}

impl ClientBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.retry_config.max_retries = max_retries;
        self
    }

    pub fn with_proxy(mut self, proxy_url: impl Into<String>) -> Self {
        self.proxy_url = Some(proxy_url.into());
        self
    }

    pub fn with_proxy_provider(mut self, provider: ProxyUrlProvider) -> Self {
        self.proxy_provider = Some(provider);
        self
    }

    pub fn with_prefer_yt_dlp(mut self, prefer: bool) -> Self {
        self.prefer_yt_dlp = prefer;
        self
    }

    pub fn build(self) -> Result<Client, YoutubeAudioError> {
        let http_client = Client::build_http_client(self.proxy_url.as_deref())?;
        let active_proxy = self.proxy_url.clone();

        Ok(Client {
            http_client: RwLock::new(http_client),
            proxy_url: self.proxy_url,
            proxy_provider: self.proxy_provider,
            active_proxy: RwLock::new(active_proxy),
            retry_config: self.retry_config,
            prefer_yt_dlp: self.prefer_yt_dlp,
        })
    }
}

impl Client {
    /// Create a new `Client` with default settings.
    pub fn new() -> Result<Self, YoutubeAudioError> {
        ClientBuilder::default().build()
    }

    /// Create a new `ClientBuilder` to customize client options.
    pub fn builder() -> ClientBuilder {
        ClientBuilder::default()
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

    async fn session(&self) -> Result<(reqwest::Client, Option<String>), YoutubeAudioError> {
        self.ensure_proxy_refreshed().await?;
        Ok((self.get_http_client().await, self.current_proxy_url().await))
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
        let (client, proxy) = self.session().await?;
        let extractor = YoutubeExtractor::with_proxy(client, proxy);
        let media = extractor.extract_media(url_or_id).await?;
        Ok(media.metadata)
    }

    /// Fetch all video items in a YouTube playlist.
    pub async fn fetch_playlist(
        &self,
        url_or_id: &str,
    ) -> Result<Vec<VideoMetadata>, YoutubeAudioError> {
        let (client, _) = self.session().await?;
        InnertubeExtractor::new(client).fetch_playlist(url_or_id).await
    }

    /// Search YouTube for videos matching a query.
    pub async fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<VideoMetadata>, YoutubeAudioError> {
        let (client, _) = self.session().await?;
        InnertubeExtractor::new(client).search(query, limit).await
    }

    /// Search YouTube with pagination support (continuation token).
    pub async fn search_page(
        &self,
        query: &str,
        continuation: Option<&str>,
    ) -> Result<(Vec<VideoMetadata>, Option<String>), YoutubeAudioError> {
        let (client, _) = self.session().await?;
        InnertubeExtractor::new(client)
            .search_page(query, continuation)
            .await
    }

    /// Resolve audio stream info and direct media URL.
    pub async fn resolve_stream(
        &self,
        url_or_id: &str,
    ) -> Result<AudioStreamResponse, YoutubeAudioError> {
        let (client, proxy) = self.session().await?;
        AudioStreamer::with_proxy(client, proxy)
            .get_stream_response(url_or_id)
            .await
    }

    /// Stream audio bytes for a previously resolved [`AudioStreamResponse`] within an optional byte range.
    ///
    /// # Arguments
    /// * `info` - Audio stream response previously resolved via [`Client::resolve_stream`].
    /// * `start_offset` - Starting byte offset (0-indexed).
    /// * `end_offset` - Optional ending byte offset (inclusive).
    pub async fn stream_from_info(
        &self,
        info: &AudioStreamResponse,
        start_offset: u64,
        end_offset: Option<u64>,
    ) -> Result<AudioByteStream, YoutubeAudioError> {
        let (client, proxy) = self.session().await?;
        AudioStreamer::with_proxy(client, proxy)
            .stream_from_info(info, start_offset, end_offset)
            .await
    }

    /// Stream audio bytes directly.
    pub async fn stream_bytes(
        &self,
        url_or_id: &str,
    ) -> Result<(VideoMetadata, AudioByteStream), YoutubeAudioError> {
        let (client, proxy) = self.session().await?;
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
        let (client, proxy) = self.session().await?;

        let downloader = YoutubeAudioDownloader {
            client,
            proxy,
            output_file: Some(output_file.to_path_buf()),
            format,
            quality: AudioQuality::Best,
            progress: on_progress.cloned(),
            prefer_yt_dlp: self.prefer_yt_dlp,
            ..Default::default()
        };

        downloader.download(url_or_id).await
    }
}
