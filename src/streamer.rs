use std::pin::Pin;

use bytes::Bytes;
use futures_util::{Stream, stream::unfold};
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

    pub async fn stream_bytes(
        self,
        url_or_id: &str,
    ) -> Result<(VideoMetadata, AudioByteStream)> {
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
        let url = stream_url.to_string();
        let client = self.client.clone();

        if let Some(total) = content_length
            && total > 0
        {
            const CHUNK_SIZE: u64 = 512 * 1024;
            let final_end = end_offset.unwrap_or(total - 1).min(total - 1);
            let initial_offset = std::cmp::min(start_offset, final_end);

            let first_end = std::cmp::min(initial_offset + CHUNK_SIZE - 1, final_end);
            let first_res = client
                .get(&url)
                .header(USER_AGENT, user_agent)
                .header(RANGE, format!("bytes={initial_offset}-{first_end}"))
                .send()
                .await?
                .error_for_status()?;

            let first_bytes = first_res.bytes().await?;
            let next_offset = initial_offset + first_bytes.len() as u64;

            let stream = unfold(
                (Some(first_bytes), next_offset, client, url, user_agent, final_end),
                |(initial, offset, client, url, user_agent, final_end)| async move {
                    if let Some(b) = initial {
                        return Some((Ok(b), (None, offset, client, url, user_agent, final_end)));
                    }

                    if offset > final_end {
                        return None;
                    }

                    let end = std::cmp::min(offset + CHUNK_SIZE - 1, final_end);
                    for _ in 0..3 {
                        let res = client
                            .get(&url)
                            .header(USER_AGENT, user_agent)
                            .header(RANGE, format!("bytes={offset}-{end}"))
                            .send()
                            .await;

                        if let Ok(resp) = res
                            && (resp.status().is_success() || resp.status() == 206)
                            && let Ok(b) = resp.bytes().await
                        {
                            if b.is_empty() {
                                return None;
                            }
                            let next = offset + b.len() as u64;
                            return Some((Ok(b), (None, next, client, url, user_agent, final_end)));
                        }
                    }
                    None
                },
            );
            return Ok(Box::pin(stream));
        }

        let mut req = self
            .client
            .get(stream_url)
            .header(USER_AGENT, user_agent);

        if let Some(end) = end_offset {
            req = req.header(RANGE, format!("bytes={start_offset}-{end}"));
        } else if start_offset > 0 {
            req = req.header(RANGE, format!("bytes={start_offset}-"));
        }

        let http_response = req.send().await?.error_for_status()?;
        Ok(Box::pin(http_response.bytes_stream()))
    }
}
