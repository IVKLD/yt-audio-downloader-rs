use std::path::{Path, PathBuf};

use reqwest::Client;

use crate::{
    converter::AudioConverter,
    error::Result,
    extractor::{MediaExtractor, YoutubeExtractor, YtDlpExtractor, extract_video_id},
    fs::FileService,
    http::create_http_client,
    models::{AudioFormat, AudioQuality, DownloadedAudio, VideoMetadata},
    progress::ProgressCallback,
    streamer::AudioStreamer,
};

pub(crate) struct YoutubeAudioDownloader {
    pub client: Client,
    pub proxy: Option<String>,
    pub output_dir: PathBuf,
    pub output_file: Option<PathBuf>,
    pub format: AudioFormat,
    pub quality: AudioQuality,
    pub progress: Option<ProgressCallback>,
    pub prefer_yt_dlp: bool,
    pub embed_metadata: bool,
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
            progress: None,
            prefer_yt_dlp: false,
            embed_metadata: true,
        }
    }
}

impl YoutubeAudioDownloader {
    pub async fn download(&self, url_or_id: &str) -> Result<DownloadedAudio> {
        let video_id = extract_video_id(url_or_id)?;

        FileService::ensure_dir(&self.output_dir).await?;

        if self.prefer_yt_dlp {
            return self.download_fallback(url_or_id, None).await;
        }

        let extractor = YoutubeExtractor::with_proxy(self.client.clone(), self.proxy.clone());
        match extractor.extract_media(&video_id).await {
            Ok(media) => {
                let Some(best_stream) = media.best_stream().cloned() else {
                    return self.download_fallback(url_or_id, Some(media.metadata)).await;
                };
                let metadata = media.metadata;

                let temp_path = FileService::temp_file_path(
                    &self.output_dir,
                    &video_id,
                    &best_stream.container,
                );

                if let Err(err) = self
                    .download_stream_to_file(&best_stream.url, best_stream.content_length, &temp_path)
                    .await
                {
                    let _ = FileService::remove_file_if_exists(&temp_path).await;
                    if YtDlpExtractor::is_available().await {
                        return self.download_fallback(url_or_id, Some(metadata)).await;
                    }
                    return Err(err);
                }

                let target_path = self.target_path(&metadata);
                let conv_res = AudioConverter::convert(
                    &temp_path,
                    &target_path,
                    self.format,
                    self.quality,
                    self.embed_metadata.then_some(&metadata),
                )
                .await;
                let _ = FileService::remove_file_if_exists(&temp_path).await;

                let final_path = match conv_res {
                    Ok(p) => p,
                    Err(_err) if YtDlpExtractor::is_available().await => {
                        return self.download_fallback(url_or_id, Some(metadata)).await;
                    }
                    Err(err) => return Err(err),
                };

                let file_size = FileService::file_size(&final_path).await?;

                Ok(DownloadedAudio {
                    file_path: final_path,
                    metadata,
                    format: self.format,
                    file_size_bytes: file_size,
                })
            }
            Err(_) => self.download_fallback(url_or_id, None).await,
        }
    }

    async fn download_stream_to_file(
        &self,
        url: &str,
        total_size: Option<u64>,
        output_path: &Path,
    ) -> Result<()> {
        let streamer = AudioStreamer::with_proxy(self.client.clone(), self.proxy.clone());
        let stream = streamer.stream_from_url(url, total_size, 0, None).await?;
        let cb = self.progress.clone();

        FileService::write_stream_to_file(output_path, stream, total_size, move |downloaded, total| {
            if let Some(ref cb) = cb {
                cb(downloaded, total);
            }
        })
        .await?;

        Ok(())
    }

    async fn download_fallback(
        &self,
        url_or_id: &str,
        cached_meta: Option<VideoMetadata>,
    ) -> Result<DownloadedAudio> {
        let metadata = match cached_meta {
            Some(m) => m,
            None => {
                let ytdlp = YtDlpExtractor::with_proxy(self.proxy.clone());
                ytdlp.extract(url_or_id).await?.metadata
            }
        };

        let target_path = self.target_path(&metadata);
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
            FileService::find_fallback_file(target_dir, target_stem).await?
        };

        let file_size = FileService::file_size(&final_path).await?;

        Ok(DownloadedAudio {
            file_path: final_path,
            metadata,
            format: self.format,
            file_size_bytes: file_size,
        })
    }

    fn target_path(&self, metadata: &VideoMetadata) -> PathBuf {
        FileService::resolve_target_path(
            &self.output_dir,
            self.output_file.as_deref(),
            metadata,
            self.format,
        )
    }
}
