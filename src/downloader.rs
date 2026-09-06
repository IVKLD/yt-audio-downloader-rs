use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
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
    http::{create_http_client, select_user_agent_for_url},
    models::{AudioFormat, AudioQuality, AudioStreamResponse, DownloadedAudio, VideoMetadata},
    progress::{ProgressEvent, ProgressHandler},
    streamer::AudioStreamer,
};

pub struct YoutubeAudioDownloader {
    client: Client,
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
        let extractor = YoutubeExtractor::new(self.client.clone());
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

        let extractor = YoutubeExtractor::new(self.client.clone());
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

                if let Err(err) = self
                    .download_stream_to_file(&best_stream.url, &temp_path)
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

    async fn download_stream_to_file(&self, stream_url: &str, output_path: &Path) -> Result<()> {
        let user_agent = select_user_agent_for_url(stream_url);

        let probe = self
            .client
            .get(stream_url)
            .header(reqwest::header::USER_AGENT, user_agent)
            .header(reqwest::header::RANGE, "bytes=0-0")
            .send()
            .await;

        let total_size = match probe {
            Ok(ref res) => res
                .headers()
                .get(reqwest::header::CONTENT_RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.rfind('/').map(|i| &s[i + 1..]))
                .and_then(|s| s.parse::<u64>().ok())
                .or_else(|| res.content_length()),
            Err(_) => None,
        };

        if let Some(total_bytes) = total_size
            && total_bytes > 0
        {
            return self
                .download_chunked_parallel(stream_url, user_agent, output_path, total_bytes)
                .await;
        }

        self.download_sequential(stream_url, user_agent, output_path)
            .await
    }

    async fn download_chunked_parallel(
        &self,
        stream_url: &str,
        user_agent: &'static str,
        output_path: &Path,
        total_bytes: u64,
    ) -> Result<()> {
        const CHUNK_SIZE: u64 = 1024 * 1024; // 1 MB slices bypass YouTube CDN throttling
        let num_chunks = total_bytes.div_ceil(CHUNK_SIZE);

        let downloaded_bytes = Arc::new(AtomicU64::new(0));

        let chunks: Vec<(usize, u64, u64)> = (0..num_chunks)
            .map(|i| {
                let start = i * CHUNK_SIZE;
                let end = std::cmp::min(start + CHUNK_SIZE - 1, total_bytes - 1);
                (i as usize, start, end)
            })
            .collect();

        let client = self.client.clone();
        let downloaded_ref = downloaded_bytes.clone();
        let url = stream_url.to_string();

        let mut stream = futures_util::stream::iter(chunks)
            .map(|(idx, start, end)| {
                let client = client.clone();
                let url = url.clone();
                let downloaded = downloaded_ref.clone();
                async move {
                    let resp = client
                        .get(&url)
                        .header(reqwest::header::USER_AGENT, user_agent)
                        .header(reqwest::header::RANGE, format!("bytes={start}-{end}"))
                        .send()
                        .await?;

                    if !resp.status().is_success() && resp.status().as_u16() != 206 {
                        return Err(YoutubeAudioError::DownloadFailed(format!(
                            "HTTP status {}",
                            resp.status()
                        )));
                    }

                    let bytes = resp.bytes().await?;
                    let downloaded_total = downloaded
                        .fetch_add(bytes.len() as u64, Ordering::Relaxed)
                        + bytes.len() as u64;

                    self.emit_progress(ProgressEvent::Downloading {
                        bytes_downloaded: downloaded_total,
                        total_bytes: Some(total_bytes),
                        percentage: Some((downloaded_total as f32 / total_bytes as f32) * 100.0),
                    });

                    Ok::<(usize, Bytes), YoutubeAudioError>((idx, bytes))
                }
            })
            .buffer_unordered(6);

        let mut collected: Vec<(usize, Bytes)> = Vec::with_capacity(num_chunks as usize);
        while let Some(res) = stream.next().await {
            collected.push(res?);
        }

        collected.sort_by_key(|(idx, _)| *idx);

        let mut writer = BufWriter::new(File::create(output_path).await?);
        for (_, chunk) in collected {
            writer.write_all(&chunk).await?;
        }
        writer.flush().await?;

        Ok(())
    }

    async fn download_sequential(
        &self,
        stream_url: &str,
        user_agent: &'static str,
        output_path: &Path,
    ) -> Result<()> {
        let resp = self
            .client
            .get(stream_url)
            .header(reqwest::header::USER_AGENT, user_agent)
            .send()
            .await?;

        if !resp.status().is_success() {
            return Err(YoutubeAudioError::DownloadFailed(format!(
                "HTTP {}",
                resp.status()
            )));
        }

        let total_bytes = resp.content_length();
        let mut downloaded: u64 = 0;
        let mut stream = resp.bytes_stream();
        let mut writer = BufWriter::new(File::create(output_path).await?);

        while let Some(chunk_res) = stream.next().await {
            let chunk = chunk_res?;
            writer.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;

            let percentage = total_bytes.map(|total| (downloaded as f32 / total as f32) * 100.0);
            self.emit_progress(ProgressEvent::Downloading {
                bytes_downloaded: downloaded,
                total_bytes,
                percentage,
            });
        }

        writer.flush().await?;
        Ok(())
    }

    async fn download_fallback(&self, url_or_id: &str) -> Result<DownloadedAudio> {
        let (metadata, _) = YoutubeExtractor::fetch_fallback(url_or_id).await?;

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

        let ytdlp = YtDlpExtractor::default();
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
