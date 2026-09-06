pub mod client;
pub mod parser;

use std::{future::Future, pin::Pin};

use client::InnertubeClient;
use parser::{
    parse_player_response, parse_playlist_contents, parse_search_results_with_token,
};
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
                innertube_client.fetch_search_continuation_json(token).await?
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
        let mut seen_ids: std::collections::HashSet<_> =
            all_tracks.iter().map(|t| t.id.clone()).collect();

        if let Some(token) = continuation_token {
            if let Ok((next_tracks, _)) = self.search_page(query, Some(&token)).await {
                for track in next_tracks {
                    if seen_ids.insert(track.id.clone()) {
                        all_tracks.push(track);
                    }
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
            let client_configs = [
                ("ANDROID_VR", "1.56.21"),
                ("ANDROID_MUSIC", "6.42.52"),
                ("IOS", "19.29.1"),
                ("ANDROID", "19.11.38"),
            ];

            let futures: Vec<_> = client_configs
                .iter()
                .map(|(c_name, c_ver)| {
                    let v_id = video_id.clone();
                    let client = &innertube_client;
                    Box::pin(async move {
                        let json = client.fetch_player_json(&v_id, c_name, c_ver).await?;
                        let media = parse_player_response(&v_id, &json)?;
                        if media.streams.is_empty() {
                            Err(YoutubeAudioError::NoAudioStreamFound)
                        } else {
                            Ok(media)
                        }
                    })
                })
                .collect();

            if let Ok((media, _)) = futures_util::future::select_ok(futures).await {
                return Ok(media);
            }

            Err(YoutubeAudioError::NoAudioStreamFound)
        })
    }
}
