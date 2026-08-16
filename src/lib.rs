pub mod converter;
pub mod downloader;
pub mod error;
pub mod extractor;
pub mod http;
pub mod models;
pub mod progress;
pub mod streamer;

use bytes::Bytes;
pub use downloader::YoutubeAudioDownloader;
pub use error::{Result, YoutubeAudioError};
pub use extractor::{extract_playlist_id, extract_video_id, is_youtube_url, youtube_id_to_i64};
use futures_util::Stream;
pub use http::{create_http_client, create_http_client_with_proxy};
pub use models::{
    AudioFormat, AudioQuality, AudioStreamInfo, AudioStreamResponse, DownloadedAudio,
    ExtractedMedia, VideoMetadata,
};
pub use progress::{ProgressEvent, ProgressHandler};
pub use streamer::AudioStreamer;

pub async fn download_audio<P: AsRef<std::path::Path>>(
    url_or_id: &str,
    output_dir: P,
) -> Result<DownloadedAudio> {
    YoutubeAudioDownloader::new()
        .output_dir(output_dir)
        .format(AudioFormat::Mp3)
        .download(url_or_id)
        .await
}

pub async fn get_stream_info(url_or_id: &str) -> Result<AudioStreamResponse> {
    AudioStreamer::default()
        .get_stream_response(url_or_id)
        .await
}

pub async fn get_stream_info_with_client(
    url_or_id: &str,
    client: reqwest::Client,
    proxy_url: Option<String>,
) -> Result<AudioStreamResponse> {
    AudioStreamer::with_proxy(client, proxy_url)
        .get_stream_response(url_or_id)
        .await
}

pub async fn stream_audio(
    url_or_id: &str,
) -> Result<(VideoMetadata, impl Stream<Item = reqwest::Result<Bytes>>)> {
    AudioStreamer::default().stream_bytes(url_or_id).await
}

pub async fn fetch_playlist(url_or_id: &str) -> Result<Vec<VideoMetadata>> {
    YoutubeAudioDownloader::new()
        .fetch_playlist(url_or_id)
        .await
}

pub async fn search_youtube(query: &str, limit: usize) -> Result<Vec<VideoMetadata>> {
    YoutubeAudioDownloader::new().search(query, limit).await
}
