pub mod id;
pub mod innertube;
pub mod strategy;
pub mod ytdlp;

use std::{sync::Arc, time::Duration};

pub use id::{extract_playlist_id, extract_video_id, is_youtube_url, youtube_id_to_i64};
pub use innertube::InnertubeExtractor;
use reqwest::Client;
pub use strategy::MediaExtractor;
use tokio::time::timeout;
use tracing::debug;
pub use ytdlp::YtDlpExtractor;

use crate::{
    error::Result,
    models::{AudioStreamInfo, ExtractedMedia, VideoMetadata},
};

pub struct YoutubeExtractor {
    primary: Arc<dyn MediaExtractor>,
    fallback: Arc<dyn MediaExtractor>,
}

impl YoutubeExtractor {
    pub fn new(client: Client) -> Self {
        Self::with_proxy(client, None)
    }

    pub fn with_proxy(client: Client, proxy: Option<String>) -> Self {
        Self {
            primary: Arc::new(InnertubeExtractor::new(client)),
            fallback: Arc::new(YtDlpExtractor::with_proxy(proxy)),
        }
    }

    pub async fn extract_media(&self, target: &str) -> Result<ExtractedMedia> {
        let innertube_result =
            timeout(Duration::from_millis(1500), self.primary.extract(target)).await;

        match innertube_result {
            Ok(Ok(media)) => Ok(media),
            Ok(Err(e)) => {
                debug!("Innertube extractor failed ({e}), invoking yt-dlp fallback...");
                self.fallback.extract(target).await
            }
            Err(_) => {
                debug!("Innertube extractor timed out (1.5s), invoking yt-dlp fallback...");
                self.fallback.extract(target).await
            }
        }
    }

    pub async fn fetch_natively(
        &self,
        video_id: &str,
    ) -> Result<(VideoMetadata, Vec<AudioStreamInfo>)> {
        let media = self.primary.extract(video_id).await?;
        Ok((media.metadata, media.streams))
    }

    pub async fn fetch_fallback(video_url: &str) -> Result<(VideoMetadata, String)> {
        let extractor = YtDlpExtractor::default();
        let media = extractor.extract(video_url).await?;
        let url = media
            .best_stream()
            .map(|s| s.url.clone())
            .unwrap_or_default();
        Ok((media.metadata, url))
    }
}
