pub mod client;
pub mod parser;

use std::{future::Future, pin::Pin};

use client::InnertubeClient;
use parser::{parse_player_response, parse_playlist_contents, parse_search_results};
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

    pub async fn fetch_playlist(&self, target: &str) -> Result<Vec<VideoMetadata>> {
        let playlist_id = extract_playlist_id(target).ok_or_else(|| {
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

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<VideoMetadata>> {
        let innertube_client = InnertubeClient::new(&self.client);
        let json = innertube_client.fetch_search_json(query).await?;
        Ok(parse_search_results(&json, limit))
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
