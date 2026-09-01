use std::pin::Pin;

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
    ) -> Result<(
        VideoMetadata,
        Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
    )> {
        let response_info = self.get_stream_response(url_or_id).await?;
        let user_agent = crate::http::select_user_agent_for_url(&response_info.stream_url);
        let total_size = response_info.content_length;
        let url = response_info.stream_url.clone();
        let client = self.client.clone();

        if let Some(total) = total_size
            && total > 0
        {
            const CHUNK_SIZE: u64 = 512 * 1024;
            let stream = futures_util::stream::unfold(
                (0u64, client, url, user_agent, total),
                |(offset, client, url, user_agent, total)| async move {
                    if offset >= total {
                        return None;
                    }
                    let end = std::cmp::min(offset + CHUNK_SIZE - 1, total - 1);
                    let res = client
                        .get(&url)
                        .header(reqwest::header::USER_AGENT, user_agent)
                        .header(reqwest::header::RANGE, format!("bytes={offset}-{end}"))
                        .send()
                        .await;

                    match res {
                        Ok(resp) => {
                            let bytes_res = resp.bytes().await;
                            match bytes_res {
                                Ok(b) => {
                                    let next_offset = offset + b.len() as u64;
                                    Some((Ok(b), (next_offset, client, url, user_agent, total)))
                                }
                                Err(e) => Some((Err(e), (total, client, url, user_agent, total))),
                            }
                        }
                        Err(e) => Some((Err(e), (total, client, url, user_agent, total))),
                    }
                },
            );
            return Ok((response_info.metadata, Box::pin(stream)));
        }

        let http_response = self
            .client
            .get(&response_info.stream_url)
            .header(reqwest::header::USER_AGENT, user_agent)
            .send()
            .await?;
        Ok((response_info.metadata, Box::pin(http_response.bytes_stream())))
    }
}
