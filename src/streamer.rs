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

        let content_length = if let Some(cl) = best.content_length {
            Some(cl)
        } else {
            url::Url::parse(&best.url)
                .ok()
                .and_then(|u| {
                    u.query_pairs()
                        .find(|(k, _)| k == "clen")
                        .map(|(_, v)| v.into_owned())
                })
                .and_then(|s| s.parse::<u64>().ok())
        };

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

    pub async fn stream_bytes(
        self,
        url_or_id: &str,
    ) -> Result<(
        VideoMetadata,
        Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
    )> {
        let (info, stream) = self.stream_bytes_range(url_or_id, 0, None).await?;
        Ok((info.metadata, stream))
    }

    pub async fn stream_bytes_from_offset(
        self,
        url_or_id: &str,
        start_offset: u64,
    ) -> Result<(
        AudioStreamResponse,
        Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
    )> {
        self.stream_bytes_range(url_or_id, start_offset, None).await
    }

    pub async fn stream_bytes_range(
        self,
        url_or_id: &str,
        start_offset: u64,
        end_offset: Option<u64>,
    ) -> Result<(
        AudioStreamResponse,
        Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
    )> {
        let response_info = self.get_stream_response(url_or_id).await?;
        self.stream_from_info(response_info, start_offset, end_offset)
            .await
    }

    pub async fn stream_from_info(
        &self,
        response_info: AudioStreamResponse,
        start_offset: u64,
        end_offset: Option<u64>,
    ) -> Result<(
        AudioStreamResponse,
        Pin<Box<dyn Stream<Item = reqwest::Result<Bytes>> + Send>>,
    )> {
        let user_agent = crate::http::select_user_agent_for_url(&response_info.stream_url);
        let total_size = response_info.content_length;
        let url = response_info.stream_url.clone();
        let client = self.client.clone();

        if let Some(total) = total_size
            && total > 0
        {
            const CHUNK_SIZE: u64 = 512 * 1024;
            let final_end = end_offset.unwrap_or(total - 1).min(total - 1);
            let initial_offset = std::cmp::min(start_offset, final_end);

            let stream = futures_util::stream::unfold(
                (initial_offset, client, url, user_agent, final_end),
                |(offset, client, url, user_agent, final_end)| async move {
                    if offset > final_end {
                        return None;
                    }
                    let end = std::cmp::min(offset + CHUNK_SIZE - 1, final_end);
                    let mut attempts = 0;
                    loop {
                        attempts += 1;
                        let res = client
                            .get(&url)
                            .header(reqwest::header::USER_AGENT, user_agent)
                            .header(reqwest::header::RANGE, format!("bytes={offset}-{end}"))
                            .send()
                            .await;

                        match res {
                            Ok(resp) => {
                                if resp.status().is_success() || resp.status().as_u16() == 206 {
                                    match resp.bytes().await {
                                        Ok(b) => {
                                            if b.is_empty() {
                                                return None;
                                            }
                                            let next_offset = offset + b.len() as u64;
                                            return Some((
                                                Ok(b),
                                                (next_offset, client, url, user_agent, final_end),
                                            ));
                                        }
                                        Err(_e) if attempts < 3 => continue,
                                        Err(e) => {
                                            return Some((
                                                Err(e),
                                                (
                                                    final_end + 1,
                                                    client,
                                                    url,
                                                    user_agent,
                                                    final_end,
                                                ),
                                            ));
                                        }
                                    }
                                } else if attempts < 3 {
                                    continue;
                                } else {
                                    return None;
                                }
                            }
                            Err(_e) if attempts < 3 => continue,
                            Err(e) => {
                                return Some((
                                    Err(e),
                                    (final_end + 1, client, url, user_agent, final_end),
                                ));
                            }
                        }
                    }
                },
            );
            return Ok((response_info, Box::pin(stream)));
        }

        let mut req = self
            .client
            .get(&response_info.stream_url)
            .header(reqwest::header::USER_AGENT, user_agent);

        if let Some(end) = end_offset {
            req = req.header(
                reqwest::header::RANGE,
                format!("bytes={start_offset}-{end}"),
            );
        } else if start_offset > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={start_offset}-"));
        }

        let http_response = req.send().await?;
        Ok((response_info, Box::pin(http_response.bytes_stream())))
    }
}
