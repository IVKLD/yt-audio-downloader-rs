use bytes::Bytes;
use futures_util::Stream;
use reqwest::Client;

use crate::{
    error::{Result, YoutubeAudioError},
    extractor::YoutubeExtractor,
    http::create_http_client,
    models::{AudioStreamResponse, VideoMetadata},
};

pub struct AudioStreamer {
    client: Client,
    proxy: Option<String>,
}

impl Default for AudioStreamer {
    fn default() -> Self {
        Self {
            client: create_http_client(),
            proxy: None,
        }
    }
}

impl AudioStreamer {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            proxy: None,
        }
    }

    pub fn with_proxy(client: Client, proxy: Option<String>) -> Self {
        Self { client, proxy }
    }

    pub async fn get_stream_response(&self, url_or_id: &str) -> Result<AudioStreamResponse> {
        let extractor = YoutubeExtractor::with_proxy(self.client.clone(), self.proxy.clone());
        let media = extractor.extract_media(url_or_id).await?;
        let best = media
            .best_stream()
            .cloned()
            .ok_or(YoutubeAudioError::NoAudioStreamFound)?;
        let metadata = media.metadata;

        Ok(AudioStreamResponse {
            metadata,
            stream_url: best.url.clone(),
            mime_type: best.mime_type.clone(),
            content_length: best.content_length,
            stream_info: best,
        })
    }

    pub async fn stream_bytes(
        self,
        url_or_id: &str,
    ) -> Result<(VideoMetadata, impl Stream<Item = reqwest::Result<Bytes>>)> {
        let response_info = self.get_stream_response(url_or_id).await?;
        let http_response = self.client.get(&response_info.stream_url).send().await?;
        Ok((response_info.metadata, http_response.bytes_stream()))
    }
}
