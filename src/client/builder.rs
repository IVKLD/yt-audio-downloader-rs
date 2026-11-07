use crate::{
    client::client::{Client, ProxyUrlProvider, RetryConfig},
    error::YoutubeAudioError,
};

/// Builder for configuring and initializing a [`Client`] instance.
#[derive(Debug, Default)]
pub struct ClientBuilder {
    pub(crate) retry_config: RetryConfig,
    pub(crate) proxy_url: Option<String>,
    pub(crate) proxy_provider: Option<ProxyUrlProvider>,
    pub(crate) prefer_yt_dlp: bool,
}

impl ClientBuilder {
    /// Create a new `ClientBuilder` with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the maximum number of retry attempts for failed requests.
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.retry_config.max_retries = max_retries;
        self
    }

    /// Set a static proxy URL for all requests made by the client.
    pub fn with_proxy(mut self, proxy_url: String) -> Self {
        self.proxy_url = Some(proxy_url);
        self
    }

    /// Attach a dynamic, reactive proxy provider.
    pub fn with_proxy_provider(mut self, provider: ProxyUrlProvider) -> Self {
        self.proxy_provider = Some(provider);
        self
    }

    /// Prefer yt-dlp over native extraction.
    pub fn with_prefer_yt_dlp(mut self, prefer: bool) -> Self {
        self.prefer_yt_dlp = prefer;
        self
    }

    /// Build the Client with the configured settings.
    pub async fn build(self) -> Result<Client, YoutubeAudioError> {
        Client::from_builder(self).await
    }
}
