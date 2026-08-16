use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};

use bytes::Bytes;
use futures_util::{Stream, StreamExt};
use reqwest::Client;
use tokio::{
    fs::File,
    io::{AsyncSeekExt, AsyncWriteExt},
    process::Command,
    sync::Mutex,
    task::JoinSet,
};

use crate::{
    converter::{sanitize_filename, AudioConverter},
    error::{Result, YoutubeAudioError},
    extractor::{extract_video_id, YoutubeExtractor},
    http::create_http_client,
    models::{AudioFormat, AudioQuality, AudioStreamResponse, DownloadedAudio, VideoMetadata},
    progress::{ProgressEvent, ProgressHandler},
    streamer::AudioStreamer,
};

struct TempFileGuard(PathBuf);
impl Drop for TempFileGuard {
    fn drop(&mut self) {
        if self.0.exists() {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}

pub struct YoutubeAudioDownloader {
    client: Client,
    output_dir: PathBuf,
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

    pub fn output_dir<P: AsRef<Path>>(mut self, dir: P) -> Self {
        self.output_dir = dir.as_ref().to_path_buf();
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
        let extractor = crate::extractor::InnertubeExtractor::new(self.client.clone());
        extractor.fetch_playlist(url_or_id).await
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<VideoMetadata>> {
        let extractor = crate::extractor::InnertubeExtractor::new(self.client.clone());
        extractor.search(query, limit).await
    }

    pub async fn get_stream(&self, url_or_id: &str) -> Result<AudioStreamResponse> {
        let streamer = AudioStreamer::new(self.client.clone());
        streamer.get_stream_response(url_or_id).await
    }

    pub async fn stream_bytes(
        &self,
        url_or_id: &str,
    ) -> Result<(VideoMetadata, impl Stream<Item = reqwest::Result<Bytes>>)> {
        let streamer = AudioStreamer::new(self.client.clone());
        streamer.stream_bytes(url_or_id).await
    }

    pub async fn download(&self, url_or_id: &str) -> Result<DownloadedAudio> {
        let video_id = extract_video_id(url_or_id)?;
        self.emit_progress(ProgressEvent::Initializing {
            video_id: video_id.clone(),
        });

        tokio::fs::create_dir_all(&self.output_dir).await?;

        if self.prefer_yt_dlp {
            return self.download_with_ytdlp(url_or_id).await;
        }

        let extractor = YoutubeExtractor::new(self.client.clone());
        match extractor.extract_media(&video_id).await {
            Ok(media) => {
                let best_stream = match media.best_stream().cloned() {
                    Some(s) => s,
                    None => return self.download_with_ytdlp(url_or_id).await,
                };
                let metadata = media.metadata;

                self.emit_progress(ProgressEvent::MetadataFetched {
                    title: metadata.title.clone(),
                    author: metadata.author.clone(),
                });

                let temp_path = self
                    .output_dir
                    .join(format!("temp_{}.{}", video_id, best_stream.container));
                let _temp_guard = TempFileGuard(temp_path.clone());

                if let Err(err) = self
                    .download_stream_to_file(&best_stream.url, &temp_path)
                    .await
                {
                    let _ = tokio::fs::remove_file(&temp_path).await;
                    if is_ytdlp_installed().await {
                        return self.download_with_ytdlp(url_or_id).await;
                    }
                    return Err(err);
                }

                self.emit_progress(ProgressEvent::Converting {
                    target_format: self.format.extension().to_string(),
                });

                let meta_param = if self.embed_metadata {
                    Some(&metadata)
                } else {
                    None
                };
                let final_path = match AudioConverter::convert(
                    &temp_path,
                    &self.output_dir,
                    &metadata.title,
                    self.format,
                    self.quality,
                    meta_param,
                )
                .await
                {
                    Ok(p) => p,
                    Err(err) => {
                        let _ = tokio::fs::remove_file(&temp_path).await;
                        if is_ytdlp_installed().await {
                            return self.download_with_ytdlp(url_or_id).await;
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
            _ => self.download_with_ytdlp(url_or_id).await,
        }
    }

    async fn download_stream_to_file(&self, stream_url: &str, output_path: &Path) -> Result<()> {
        let initial_res = self
            .client
            .get(stream_url)
            .header("Range", "bytes=0-0")
            .send()
            .await;

        let content_length = match initial_res {
            Ok(ref res) => res
                .headers()
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.rfind('/').map(|i| &s[i + 1..]))
                .and_then(|s| s.parse::<u64>().ok())
                .or_else(|| res.content_length()),
            Err(_) => None,
        };

        if let Some(total_size) = content_length {
            if total_size > 0 {
                return self
                    .download_stream_chunked(stream_url, output_path, total_size)
                    .await;
            }
        }

        self.download_stream_sequential(stream_url, output_path)
            .await
    }

    async fn download_stream_chunked(
        &self,
        stream_url: &str,
        output_path: &Path,
        total_size: u64,
    ) -> Result<()> {
        let file = File::create(output_path).await?;
        file.set_len(total_size).await?;
        let file = Arc::new(Mutex::new(file));

        let chunk_size: u64 = 1024 * 1024; // 1 MB chunks
        let total_chunks = (total_size + chunk_size - 1) / chunk_size;

        let downloaded_bytes = Arc::new(AtomicU64::new(0));
        let semaphore = Arc::new(tokio::sync::Semaphore::new(6));

        let mut join_set = JoinSet::new();

        for i in 0..total_chunks {
            let start_byte = i * chunk_size;
            let end_byte = std::cmp::min((i + 1) * chunk_size - 1, total_size - 1);
            let client = self.client.clone();
            let url = stream_url.to_string();
            let file = file.clone();
            let downloaded_bytes = downloaded_bytes.clone();
            let semaphore = semaphore.clone();
            let progress_handler = self.progress_handler.clone();

            join_set.spawn(async move {
                let _permit = semaphore
                    .acquire()
                    .await
                    .map_err(|_| YoutubeAudioError::DownloadFailed("Semaphore closed".into()))?;

                let mut retries = 0;
                const MAX_RETRIES: u32 = 5;

                loop {
                    let req = client
                        .get(&url)
                        .header("Range", format!("bytes={start_byte}-{end_byte}"));

                    match req.send().await {
                        Ok(mut resp) => {
                            if !resp.status().is_success() {
                                if retries < MAX_RETRIES {
                                    retries += 1;
                                    tokio::time::sleep(std::time::Duration::from_millis(
                                        200 * retries as u64,
                                    ))
                                    .await;
                                    continue;
                                }
                                return Err(YoutubeAudioError::DownloadFailed(format!(
                                    "HTTP status {}",
                                    resp.status()
                                )));
                            }

                            let mut current_offset = start_byte;
                            let mut chunk_err = false;

                            while let Some(chunk_res) = resp.chunk().await.transpose() {
                                match chunk_res {
                                    Ok(chunk) => {
                                        {
                                            let mut f = file.lock().await;
                                            f.seek(std::io::SeekFrom::Start(current_offset))
                                                .await?;
                                            f.write_all(&chunk).await?;
                                        }
                                        current_offset += chunk.len() as u64;
                                        let total = downloaded_bytes
                                            .fetch_add(chunk.len() as u64, Ordering::Relaxed)
                                            + chunk.len() as u64;

                                        if let Some(ref handler) = progress_handler {
                                            let percentage =
                                                Some((total as f32 / total_size as f32) * 100.0);
                                            handler(ProgressEvent::Downloading {
                                                bytes_downloaded: total,
                                                total_bytes: Some(total_size),
                                                percentage,
                                            });
                                        }
                                    }
                                    Err(_) => {
                                        chunk_err = true;
                                        break;
                                    }
                                }
                            }

                            if !chunk_err && current_offset > end_byte {
                                return Ok(());
                            }
                        }
                        Err(_) => {
                            if retries < MAX_RETRIES {
                                retries += 1;
                                tokio::time::sleep(std::time::Duration::from_millis(
                                    200 * retries as u64,
                                ))
                                .await;
                                continue;
                            }
                            return Err(YoutubeAudioError::DownloadFailed(
                                "Max retries reached".into(),
                            ));
                        }
                    }

                    retries += 1;
                    if retries >= MAX_RETRIES {
                        return Err(YoutubeAudioError::DownloadFailed(
                            "Max retries reached".into(),
                        ));
                    }
                }
            });
        }

        while let Some(res) = join_set.join_next().await {
            match res {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    join_set.abort_all();
                    return Err(e);
                }
                Err(e) => {
                    join_set.abort_all();
                    return Err(YoutubeAudioError::DownloadFailed(e.to_string()));
                }
            }
        }

        let mut f = file.lock().await;
        f.flush().await?;

        Ok(())
    }

    async fn download_stream_sequential(&self, stream_url: &str, output_path: &Path) -> Result<()> {
        let mut file = File::create(output_path).await?;
        let mut downloaded: u64 = 0;
        let mut total_size: Option<u64> = None;
        let mut retries = 0;
        const MAX_RETRIES: u32 = 15;

        while retries < MAX_RETRIES {
            let mut req = self.client.get(stream_url);
            if downloaded > 0 {
                req = req.header("Range", format!("bytes={}-", downloaded));
            }

            match req.send().await {
                Ok(res) => {
                    if total_size.is_none() {
                        if downloaded == 0 {
                            total_size = res.content_length();
                        } else if let Some(content_range) = res.headers().get("content-range") {
                            if let Ok(range_str) = content_range.to_str() {
                                if let Some(slash_idx) = range_str.rfind('/') {
                                    if let Ok(total) = range_str[slash_idx + 1..].parse::<u64>() {
                                        total_size = Some(total);
                                    }
                                }
                            }
                        }
                    }

                    let mut stream = res.bytes_stream();
                    let mut chunk_err = false;

                    while let Some(chunk_result) = stream.next().await {
                        match chunk_result {
                            Ok(chunk) => {
                                file.write_all(&chunk).await?;
                                downloaded += chunk.len() as u64;
                                let percentage = total_size
                                    .map(|total| (downloaded as f32 / total as f32) * 100.0);

                                self.emit_progress(ProgressEvent::Downloading {
                                    bytes_downloaded: downloaded,
                                    total_bytes: total_size,
                                    percentage,
                                });
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "YouTube stream chunk error: {e}. Retrying from byte {downloaded}..."
                                );
                                chunk_err = true;
                                break;
                            }
                        }
                    }

                    if !chunk_err {
                        if let Some(total) = total_size {
                            if downloaded >= total {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("HTTP request error on download attempt {retries}: {e}");
                }
            }

            retries += 1;
        }

        file.flush().await?;
        Ok(())
    }

    pub async fn download_with_ytdlp(&self, url_or_id: &str) -> Result<DownloadedAudio> {
        let (metadata, _) = YoutubeExtractor::fetch_fallback(url_or_id).await?;

        self.emit_progress(ProgressEvent::MetadataFetched {
            title: metadata.title.clone(),
            author: metadata.author.clone(),
        });

        let full_title = if metadata.author.is_empty()
            || metadata.author == "Unknown"
            || metadata.author == "YouTube"
            || metadata.title.contains(&metadata.author)
        {
            metadata.title.clone()
        } else {
            format!("{} - {}", metadata.author, metadata.title)
        };
        let sanitized_title = sanitize_filename(&full_title);
        let output_template = self.output_dir.join(format!("{}.%(ext)s", sanitized_title));

        struct YtDlpCleanupGuard {
            output_dir: PathBuf,
            sanitized_title: String,
            completed: bool,
        }

        impl Drop for YtDlpCleanupGuard {
            fn drop(&mut self) {
                if !self.completed {
                    let prefix = self.sanitized_title.clone();
                    let output_dir = self.output_dir.clone();
                    if let Ok(entries) = std::fs::read_dir(&output_dir) {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
                                if file_name.contains(&prefix) {
                                    let _ = std::fs::remove_file(&path);
                                }
                            }
                        }
                    }
                }
            }
        }

        let mut cleanup_guard = YtDlpCleanupGuard {
            output_dir: self.output_dir.clone(),
            sanitized_title: sanitized_title.clone(),
            completed: false,
        };

        let audio_format_arg = match self.format {
            AudioFormat::Best => "best",
            fmt => fmt.extension(),
        };

        let cmd_name = find_ytdlp_cmd()
            .await
            .ok_or(YoutubeAudioError::YtDlpNotFound)?;
        let mut cmd = Command::new(cmd_name);
        cmd.kill_on_drop(true);
        cmd.args([
            "-x",
            "--audio-format",
            audio_format_arg,
            "--audio-quality",
            self.quality.bitrate_kbps(),
            "-N",
            "4",
            "-o",
            output_template.to_str().unwrap_or_default(),
            "--no-playlist",
            "--no-warnings",
        ]);

        if Command::new("node").arg("--version").output().await.is_ok() {
            cmd.args(["--js-runtimes", "node"]);
        }

        let home = std::env::var("HOME").unwrap_or_default();
        let cookies_path = format!("{home}/.config/vortex-dl/cookies.txt");
        if std::path::Path::new("./cookies.txt").exists() {
            cmd.args(["--cookies", "./cookies.txt"]);
        } else if !home.is_empty() && std::path::Path::new(&cookies_path).exists() {
            cmd.args(["--cookies", &cookies_path]);
        } else {
            if std::path::Path::new(&format!("{home}/.mozilla/firefox")).exists()
                || std::path::Path::new(&format!("{home}/.snap/firefox")).exists()
            {
                cmd.args(["--cookies-from-browser", "firefox"]);
            } else if std::path::Path::new(&format!("{home}/.config/google-chrome")).exists() {
                cmd.args(["--cookies-from-browser", "chrome"]);
            } else if std::path::Path::new(&format!("{home}/.config/chromium")).exists() {
                cmd.args(["--cookies-from-browser", "chromium"]);
            }
        }

        if self.embed_metadata {
            cmd.arg("--add-metadata");
        }

        cmd.arg(url_or_id);

        self.emit_progress(ProgressEvent::Downloading {
            bytes_downloaded: 0,
            total_bytes: None,
            percentage: None,
        });

        let output = cmd.output().await.map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                YoutubeAudioError::YtDlpNotFound
            } else {
                YoutubeAudioError::Io(e)
            }
        })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(YoutubeAudioError::YtDlpFailed {
                status: output.status.code(),
                stderr,
            });
        }

        let expected_final_path =
            self.output_dir
                .join(format!("{}.{}", sanitized_title, self.format.extension()));

        let final_path = if expected_final_path.exists() {
            expected_final_path
        } else {
            let mut found = None;
            if let Ok(mut entries) = tokio::fs::read_dir(&self.output_dir).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let path = entry.path();
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if name.starts_with(&sanitized_title)
                            && !name.ends_with(".part")
                            && !name.ends_with(".ytdl")
                        {
                            found = Some(path);
                            break;
                        }
                    }
                }
            }
            found.ok_or_else(|| {
                YoutubeAudioError::DownloadFailed("Downloaded file not found".into())
            })?
        };

        cleanup_guard.completed = true;

        let file_size = tokio::fs::metadata(&final_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);

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
}

pub async fn find_ytdlp_cmd() -> Option<String> {
    let home = std::env::var("HOME").unwrap_or_default();
    if !home.is_empty() {
        let user_bin = format!("{home}/.local/bin/yt-dlp");
        if std::path::Path::new(&user_bin).exists() {
            return Some(user_bin);
        }
    }
    if std::path::Path::new("./yt-dlp").exists() {
        return Some("./yt-dlp".to_string());
    }
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(parent) = exe_path.parent() {
            let local_exe = parent.join("yt-dlp");
            if local_exe.exists() {
                return Some(local_exe.to_string_lossy().to_string());
            }
        }
    }
    if Command::new("yt-dlp")
        .arg("--version")
        .output()
        .await
        .is_ok()
    {
        return Some("yt-dlp".to_string());
    }
    None
}

pub async fn is_ytdlp_installed() -> bool {
    find_ytdlp_cmd().await.is_some()
}
