pub mod client;
pub mod parser;

use std::{collections::HashSet, future::Future, pin::Pin};

use client::InnertubeClient;
use parser::{parse_player_response, parse_playlist_contents, parse_search_results_with_token};
use reqwest::Client;

use super::{
    id::{extract_playlist_id, extract_video_id},
    strategy::MediaExtractor,
};
use crate::{
    error::{Result, YoutubeAudioError},
    models::{ExtractedMedia, VideoMetadata},
};

pub struct InnertubeExtractor {
    client: Client,
}

impl InnertubeExtractor {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    pub async fn fetch_playlist(&self, url_or_id: &str) -> Result<Vec<VideoMetadata>> {
        let playlist_id = extract_playlist_id(url_or_id).ok_or_else(|| {
            YoutubeAudioError::InvalidUrl("Not a valid playlist URL or ID".into())
        })?;

        let browse_id = if playlist_id.starts_with("VL") {
            playlist_id
        } else {
            format!("VL{}", playlist_id)
        };

        let innertube_client = InnertubeClient::new(&self.client);
        let json = innertube_client.fetch_browse_json(&browse_id).await?;
        Ok(parse_playlist_contents(&json))
    }

    pub async fn search_page(
        &self,
        query: &str,
        continuation: Option<&str>,
    ) -> Result<(Vec<VideoMetadata>, Option<String>)> {
        let innertube_client = InnertubeClient::new(&self.client);
        let json = match continuation {
            Some(token) if !token.is_empty() => {
                innertube_client
                    .fetch_search_continuation_json(token)
                    .await?
            }
            _ => innertube_client.fetch_search_json(query).await?,
        };

        Ok(parse_search_results_with_token(&json))
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<VideoMetadata>> {
        let (tracks, continuation_token) = self.search_page(query, None).await?;
        if tracks.len() >= limit || limit <= 25 || continuation_token.is_none() {
            let mut result = tracks;
            result.truncate(limit);
            return Ok(result);
        }

        let mut all_tracks = tracks;
        let mut seen_ids: HashSet<_> = all_tracks.iter().map(|t| t.id.clone()).collect();

        if let Some(token) = continuation_token
            && let Ok((next_tracks, _)) = self.search_page(query, Some(&token)).await
        {
            for track in next_tracks {
                if seen_ids.insert(track.id.clone()) {
                    all_tracks.push(track);
                }
            }
        }

        all_tracks.truncate(limit);
        Ok(all_tracks)
    }
}

impl MediaExtractor for InnertubeExtractor {
    fn extract<'a>(
        &'a self,
        target: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<ExtractedMedia>> + Send + 'a>> {
        Box::pin(async move {
            let video_id = extract_video_id(target)?;
            let innertube_client = InnertubeClient::new(&self.client);

            // Fast-path: ANDROID_VR gives direct unthrottled audio streams (opus/m4a) without cipher
            if let Ok(json) = innertube_client
                .fetch_player_json(&video_id, "ANDROID_VR", "1.56.21")
                .await
                && let Ok(media) = parse_player_response(&video_id, &json)
                && !media.streams.is_empty()
            {
                return Ok(media);
            }

            // Fallback Innertube: WEB client
            if let Ok(json) = innertube_client
                .fetch_player_json(&video_id, "WEB", "2.20240725.01.00")
                .await
                && let Ok(media) = parse_player_response(&video_id, &json)
                && !media.streams.is_empty()
            {
                return Ok(media);
            }

            Err(YoutubeAudioError::NoAudioStreamFound)
        })
    }
}
