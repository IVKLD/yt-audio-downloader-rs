use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use reqwest::Client;
use tokio::{
    fs::File,
    io::{AsyncWriteExt, BufWriter},
};

use crate::{
    converter::{AudioConverter, sanitize_filename},
    error::{Result, YoutubeAudioError},
    extractor::{InnertubeExtractor, YoutubeExtractor, YtDlpExtractor, extract_video_id},
    http::create_http_client,
    models::{AudioFormat, AudioQuality, AudioStreamResponse, DownloadedAudio, VideoMetadata},
    progress::{ProgressEvent, ProgressHandler},
    streamer::AudioStreamer,
};

pub struct YoutubeAudioDownloader {
    client: Client,
    proxy: Option<String>,
    output_dir: PathBuf,
    output_file: Option<PathBuf>,
    format: AudioFormat,
    quality: AudioQuality,
    progress_handler: Option<ProgressHandler>,
    prefer_yt_dlp: bool,
    embed_metadata: bool,
}

impl Default for YoutubeAudioDownloader {
    fn default() -> Self {
        Self {
            client: create_http_client(),
            proxy: None,
            output_dir: PathBuf::from("downloads"),
            output_file: None,
            format: AudioFormat::Mp3,
            quality: AudioQuality::Best,
            progress_handler: None,
            prefer_yt_dlp: false,
            embed_metadata: true,
        }
    }
}

impl YoutubeAudioDownloader {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn client(mut self, client: Client) -> Self {
        self.client = client;
        self
    }

    pub fn proxy<S: Into<String>>(mut self, proxy: S) -> Self {
        self.proxy = Some(proxy.into());
        self
    }

    pub fn output_dir<P: AsRef<Path>>(mut self, dir: P) -> Self {
        self.output_dir = dir.as_ref().to_path_buf();
        self
    }

    pub fn output_file<P: AsRef<Path>>(mut self, path: P) -> Self {
        self.output_file = Some(path.as_ref().to_path_buf());
        self
    }

    pub fn format(mut self, format: AudioFormat) -> Self {
        self.format = format;
        self
    }

    pub fn quality(mut self, quality: AudioQuality) -> Self {
        self.quality = quality;
        self
    }

    pub fn on_progress<F>(mut self, handler: F) -> Self
    where
        F: Fn(ProgressEvent) + Send + Sync + 'static,
    {
        self.progress_handler = Some(Arc::new(handler));
        self
    }

    pub fn prefer_yt_dlp(mut self, prefer: bool) -> Self {
        self.prefer_yt_dlp = prefer;
        self
    }

    pub fn embed_metadata(mut self, embed: bool) -> Self {
        self.embed_metadata = embed;
        self
    }

    fn emit_progress(&self, event: ProgressEvent) {
        if let Some(ref handler) = self.progress_handler {
            handler(event);
        }
    }

    pub async fn fetch_metadata(&self, url_or_id: &str) -> Result<VideoMetadata> {
        let extractor = YoutubeExtractor::with_proxy(self.client.clone(), self.proxy.clone());
        let media = extractor.extract_media(url_or_id).await?;
        Ok(media.metadata)
    }

    pub async fn fetch_playlist(&self, url_or_id: &str) -> Result<Vec<VideoMetadata>> {
        let extractor = InnertubeExtractor::new(self.client.clone());
        extractor.fetch_playlist(url_or_id).await
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<VideoMetadata>> {
        let extractor = InnertubeExtractor::new(self.client.clone());
        extractor.search(query, limit).await
    }

    pub async fn search_page(
        &self,
        query: &str,
        continuation: Option<&str>,
    ) -> Result<(Vec<VideoMetadata>, Option<String>)> {
        let extractor = InnertubeExtractor::new(self.client.clone());
        extractor.search_page(query, continuation).await
    }

    pub async fn get_stream(&self, url_or_id: &str) -> Result<AudioStreamResponse> {
        let streamer = AudioStreamer::new(self.client.clone());
        streamer.get_stream_response(url_or_id).await
    }

    pub async fn stream_bytes(
        self,
        url_or_id: &str,
    ) -> Result<(VideoMetadata, impl Stream<Item = reqwest::Result<Bytes>>)> {
        AudioStreamer::new(self.client.clone())
            .stream_bytes(url_or_id)
            .await
    }

    pub async fn download(&self, url_or_id: &str) -> Result<DownloadedAudio> {
        let video_id = extract_video_id(url_or_id)?;
        self.emit_progress(ProgressEvent::Initializing {
            video_id: video_id.clone(),
        });

        tokio::fs::create_dir_all(&self.output_dir).await?;

        if self.prefer_yt_dlp {
            return self.download_fallback(url_or_id).await;
        }

        let extractor = YoutubeExtractor::with_proxy(self.client.clone(), self.proxy.clone());
        match extractor.extract_media(&video_id).await {
            Ok(media) => {
                let Some(best_stream) = media.best_stream().cloned() else {
                    return self.download_fallback(url_or_id).await;
                };
                let metadata = media.metadata;

                self.emit_progress(ProgressEvent::MetadataFetched {
                    title: metadata.title.clone(),
                    author: metadata.author.clone(),
                });

                let temp_path = self
                    .output_dir
                    .join(format!("temp_{video_id}.{}", best_stream.container));

                let stream_response = AudioStreamResponse {
                    metadata: metadata.clone(),
                    stream_url: best_stream.url.clone(),
                    mime_type: best_stream.mime_type.clone(),
                    content_length: best_stream.content_length,
                    stream_info: best_stream.clone(),
                };

                if let Err(err) = self
                    .download_stream_to_file(stream_response, &temp_path)
                    .await
                {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    if YtDlpExtractor::is_available().await {
                        return self.download_fallback(url_or_id).await;
                    }
                    return Err(err);
                }

                self.emit_progress(ProgressEvent::Converting {
                    target_format: self.format.extension().to_string(),
                });

                let target_path = self.resolve_target_path(&metadata);
                let meta_param = if self.embed_metadata {
                    Some(&metadata)
                } else {
                    None
                };

                let final_path = match AudioConverter::convert(
                    &temp_path,
                    &target_path,
                    self.format,
                    self.quality,
                    meta_param,
                )
                .await
                {
                    Ok(p) => {
                        let _ = tokio::fs::remove_file(&temp_path).await;
                        p
                    }
                    Err(err) => {
                        let _ = tokio::fs::remove_file(&temp_path).await;
                        if YtDlpExtractor::is_available().await {
                            return self.download_fallback(url_or_id).await;
                        }
                        return Err(err);
                    }
                };

                let file_size = tokio::fs::metadata(&final_path).await?.len();
                self.emit_progress(ProgressEvent::Finished {
                    output_path: final_path.clone(),
                    total_bytes: file_size,
                });

                Ok(DownloadedAudio {
                    file_path: final_path,
                    metadata,
                    format: self.format,
                    file_size_bytes: file_size,
                })
            }
            Err(_) => self.download_fallback(url_or_id).await,
        }
    }

    async fn download_stream_to_file(
        &self,
        stream_response: AudioStreamResponse,
        output_path: &Path,
    ) -> Result<()> {
        let total_size = stream_response.content_length;
        let streamer = AudioStreamer::new(self.client.clone());
        let (_, mut stream) = streamer.stream_from_info(stream_response, 0, None).await?;
        let mut writer = BufWriter::new(File::create(output_path).await?);
        let mut downloaded: u64 = 0;

        while let Some(chunk_res) = stream.next().await {
            let chunk = chunk_res?;
            writer.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;

            self.emit_progress(ProgressEvent::Downloading {
                bytes_downloaded: downloaded,
                total_bytes: total_size,
                percentage: total_size.map(|t| (downloaded as f32 / t as f32) * 100.0),
            });
        }

        writer.flush().await?;
        Ok(())
    }

    async fn download_fallback(&self, url_or_id: &str) -> Result<DownloadedAudio> {
        let (metadata, _) =
            YoutubeExtractor::fetch_fallback_with_proxy(url_or_id, self.proxy.clone()).await?;

        self.emit_progress(ProgressEvent::MetadataFetched {
            title: metadata.title.clone(),
            author: metadata.author.clone(),
        });

        let target_path = self.resolve_target_path(&metadata);
        let target_dir = target_path.parent().unwrap_or(&self.output_dir);
        let target_stem = target_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("output");
        let output_template = target_dir.join(format!("{target_stem}.%(ext)s"));

        let ytdlp = YtDlpExtractor::with_proxy(self.proxy.clone());
        ytdlp
            .download_audio(
                url_or_id,
                &output_template,
                self.format,
                self.quality,
                self.embed_metadata,
            )
            .await?;

        let final_path = if target_path.exists() {
            target_path
        } else {
            let mut found = None;
            if let Ok(mut entries) = tokio::fs::read_dir(target_dir).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    if let Some(name) = path.file_name().and_then(|n| n.to_str())
                        && name.starts_with(target_stem)
                        && !name.ends_with(".part")
                        && !name.ends_with(".ytdl")
                    {
                        found = Some(path);
                        break;
                    }
                }
            }
            found.ok_or_else(|| {
                YoutubeAudioError::DownloadFailed("Downloaded file not found".into())
            })?
        };

        let file_size = tokio::fs::metadata(&final_path).await?.len();
        self.emit_progress(ProgressEvent::Finished {
            output_path: final_path.clone(),
            total_bytes: file_size,
        });

        Ok(DownloadedAudio {
            file_path: final_path,
            metadata,
            format: self.format,
            file_size_bytes: file_size,
        })
    }

    fn resolve_target_path(&self, metadata: &VideoMetadata) -> PathBuf {
        if let Some(ref path) = self.output_file {
            return path.clone();
        }

        let full_title = if metadata.author.is_empty()
            || metadata.author == "Unknown"
            || metadata.author == "YouTube"
            || metadata.title.contains(&metadata.author)
        {
            metadata.title.clone()
        } else {
            format!("{} - {}", metadata.author, metadata.title)
        };

        let sanitized = sanitize_filename(&full_title);
        self.output_dir
            .join(format!("{sanitized}.{}", self.format.extension()))
    }
}
