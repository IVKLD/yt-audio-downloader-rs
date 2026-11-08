pub mod id;
pub mod innertube;
pub mod strategy;
pub mod ytdlp;

use std::sync::Arc;

pub use id::{extract_playlist_id, extract_video_id, is_youtube_url, youtube_id_to_i64};
pub use innertube::InnertubeExtractor;
use reqwest::Client;
pub use strategy::MediaExtractor;
use tracing::debug;
pub use ytdlp::YtDlpExtractor;

use crate::{
    error::Result,
    models::ExtractedMedia,
};

pub struct YoutubeExtractor {
    primary: Arc<dyn MediaExtractor>,
    fallback: Arc<dyn MediaExtractor>,
}

impl YoutubeExtractor {
    pub fn with_proxy(client: Client, proxy: Option<String>) -> Self {
        Self {
            primary: Arc::new(InnertubeExtractor::new(client)),
            fallback: Arc::new(YtDlpExtractor::with_proxy(proxy)),
        }
    }

    pub async fn extract_media(&self, target: &str) -> Result<ExtractedMedia> {
        match self.primary.extract(target).await {
            Ok(media) => Ok(media),
            Err(e) => {
                debug!("Primary extractor failed ({e}), invoking fallback...");
                self.fallback.extract(target).await
            }
        }
    }
}
