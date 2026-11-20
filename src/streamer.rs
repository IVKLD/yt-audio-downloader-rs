use std::pin::Pin;

use bytes::Bytes;
use futures_util::Stream;
use reqwest::{
    Client,
    header::{RANGE, USER_AGENT},
};
use url::Url;

use crate::{
    error::{Result, YoutubeAudioError},
    extractor::YoutubeExtractor,
    http::{create_http_client, select_user_agent_for_url},
    models::{AudioStreamResponse, ExtractedMedia, VideoMetadata},
};

/// Asynchronous stream of audio byte chunks.
pub type AudioByteStream = Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>;

pub(crate) struct AudioStreamer {
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
    pub fn with_proxy(client: Client, proxy: Option<String>) -> Self {
        Self { client, proxy }
    }

    fn convert_media_to_response(media: ExtractedMedia) -> Result<AudioStreamResponse> {
        let best = media
            .best_stream()
            .cloned()
            .ok_or(YoutubeAudioError::NoAudioStreamFound)?;

        let content_length = best.content_length.or_else(|| {
            Url::parse(&best.url).ok().and_then(|u| {
                u.query_pairs()
                    .find(|(k, _)| k == "clen")
                    .and_then(|(_, v)| v.parse().ok())
            })
        });

        let mut stream_info = best.clone();
        stream_info.content_length = content_length;

        Ok(AudioStreamResponse {
            metadata: media.metadata,
            stream_url: best.url,
            mime_type: best.mime_type,
            content_length,
            stream_info,
        })
    }

    pub async fn get_stream_response(&self, url_or_id: &str) -> Result<AudioStreamResponse> {
        let extractor = YoutubeExtractor::with_proxy(self.client.clone(), self.proxy.clone());
        let media = extractor.extract_media(url_or_id).await?;
        Self::convert_media_to_response(media)
    }

    pub async fn stream_bytes(self, url_or_id: &str) -> Result<(VideoMetadata, AudioByteStream)> {
        let info = self.get_stream_response(url_or_id).await?;
        let metadata = info.metadata.clone();
        let stream = self.stream_from_info(&info, 0, None).await?;
        Ok((metadata, stream))
    }

    pub async fn stream_from_info(
        &self,
        response_info: &AudioStreamResponse,
        start_offset: u64,
        end_offset: Option<u64>,
    ) -> Result<AudioByteStream> {
        self.stream_from_url(
            &response_info.stream_url,
            response_info.content_length,
            start_offset,
            end_offset,
        )
        .await
    }

    pub async fn stream_from_url(
        &self,
        stream_url: &str,
        content_length: Option<u64>,
        start_offset: u64,
        end_offset: Option<u64>,
    ) -> Result<AudioByteStream> {
        let user_agent = select_user_agent_for_url(stream_url);
        let mut req = self.client.get(stream_url).header(USER_AGENT, user_agent);

        let is_full_file = start_offset == 0
            && match (end_offset, content_length) {
                (None, _) => true,
                (Some(end), Some(total)) if total > 0 => end >= total - 1,
                _ => false,
            };

        if !is_full_file {
            let clamped_end = match (end_offset, content_length) {
                (Some(end), Some(total)) if total > 0 => Some(end.min(total - 1)),
                (end, _) => end,
            };

            let range_header = match clamped_end {
                Some(end) => format!("bytes={start_offset}-{end}"),
                None => format!("bytes={start_offset}-"),
            };
            req = req.header(RANGE, range_header);
        }

        let response = req.send().await?.error_for_status()?;
        Ok(Box::pin(response.bytes_stream()))
    }
}
