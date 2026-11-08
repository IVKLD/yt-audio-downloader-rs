//! Fast, robust YouTube audio extraction, streaming, and client library.
//!
//! The primary entry point for applications is [`Client`].

pub mod client;
mod converter;
mod downloader;
pub mod error;
mod extractor;
pub mod fs;
mod http;
pub mod models;
pub mod progress;
mod streamer;

pub use client::{Client, ClientBuilder, ProxyUrlProvider, RetryConfig};
pub use error::{Result, YoutubeAudioError};
pub use extractor::{extract_playlist_id, extract_video_id, is_youtube_url, youtube_id_to_i64};
pub use fs::FileService;
pub use models::{
    AudioFormat, AudioQuality, AudioStreamInfo, AudioStreamResponse, DownloadedAudio,
    ExtractedMedia, VideoMetadata,
};
pub use progress::ProgressCallback;
pub use streamer::AudioByteStream;

pub async fn download_audio<P: AsRef<std::path::Path>>(
    url_or_id: &str,
    output_dir: P,
) -> Result<DownloadedAudio> {
    let downloader = downloader::YoutubeAudioDownloader {
        output_dir: output_dir.as_ref().to_path_buf(),
        format: AudioFormat::Mp3,
        ..Default::default()
    };
    downloader.download(url_or_id).await
}

pub async fn get_stream_info(url_or_id: &str) -> Result<AudioStreamResponse> {
    streamer::AudioStreamer::default()
        .get_stream_response(url_or_id)
        .await
}

pub async fn stream_audio(
    url_or_id: &str,
) -> Result<(VideoMetadata, AudioByteStream)> {
    streamer::AudioStreamer::default().stream_bytes(url_or_id).await
}

pub async fn fetch_playlist(url_or_id: &str) -> Result<Vec<VideoMetadata>> {
    extractor::InnertubeExtractor::new(http::create_http_client())
        .fetch_playlist(url_or_id)
        .await
}

pub async fn search_youtube(query: &str, limit: usize) -> Result<Vec<VideoMetadata>> {
    extractor::InnertubeExtractor::new(http::create_http_client())
        .search(query, limit)
        .await
}

pub async fn search_youtube_page(
    query: &str,
    continuation: Option<&str>,
) -> Result<(Vec<VideoMetadata>, Option<String>)> {
    extractor::InnertubeExtractor::new(http::create_http_client())
        .search_page(query, continuation)
        .await
}
