use std::{
    env, future::Future, path::Path, pin::Pin, process::Command as StdCommand, sync::OnceLock,
};

use tokio::process::Command;

use super::{id::extract_video_id, strategy::MediaExtractor};
use crate::{
    error::{Result, YoutubeAudioError},
    models::{AudioFormat, AudioQuality, AudioStreamInfo, ExtractedMedia, VideoMetadata},
};

static CACHED_CMD: OnceLock<Option<String>> = OnceLock::new();
static CACHED_JS_RUNTIME: OnceLock<Option<&'static str>> = OnceLock::new();

#[derive(Default, Clone)]
pub struct YtDlpExtractor {
    pub proxy: Option<String>,
}

impl YtDlpExtractor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_proxy(proxy: Option<String>) -> Self {
        Self { proxy }
    }

    pub async fn is_available() -> bool {
        Self::find_cmd().await.is_ok()
    }

    pub async fn find_cmd() -> Result<String> {
        if let Some(cached) = CACHED_CMD.get() {
            return cached.clone().ok_or(YoutubeAudioError::YtDlpNotFound);
        }

        let home = env::var("HOME").unwrap_or_default();
        let candidates = [
            format!("{home}/.local/bin/yt-dlp"),
            "./yt-dlp".to_string(),
            "yt-dlp".to_string(),
        ];
        for candidate in candidates {
            if Path::new(&candidate).exists()
                || Command::new(&candidate)
                    .arg("--version")
                    .output()
                    .await
                    .is_ok()
            {
                let _ = CACHED_CMD.set(Some(candidate.clone()));
                return Ok(candidate);
            }
        }
        let _ = CACHED_CMD.set(None);
        Err(YoutubeAudioError::YtDlpNotFound)
    }

    pub async fn build_command(&self) -> Result<Command> {
        let cmd_path = Self::find_cmd().await?;
        let mut cmd = Command::new(cmd_path);
        cmd.kill_on_drop(true);

        let js_runtime = *CACHED_JS_RUNTIME.get_or_init(|| {
            if StdCommand::new("deno").arg("--version").output().is_ok() {
                Some("deno")
            } else if StdCommand::new("node").arg("--version").output().is_ok() {
                Some("node")
            } else {
                None
            }
        });

        if let Some(runtime) = js_runtime {
            cmd.args(["--js-runtimes", runtime]);
        }

        cmd.args([
            "--no-config",
            "--no-check-certificates",
            "--no-call-home",
            "--socket-timeout",
            "5",
            "--extractor-args",
            "youtube:player_client=web_embedded,android_vr;skip=hls,dash",
        ]);

        if let Some(ref proxy) = self.proxy
            && !proxy.is_empty()
        {
            cmd.args(["--proxy", proxy]);
        }

        let home = std::env::var("HOME").unwrap_or_default();
        if std::path::Path::new("./cookies.txt").exists() {
            cmd.args(["--cookies", "./cookies.txt"]);
        } else if !home.is_empty()
            && std::path::Path::new(&format!("{home}/.config/vortex-dl/cookies.txt")).exists()
        {
            cmd.args([
                "--cookies",
                &format!("{home}/.config/vortex-dl/cookies.txt"),
            ]);
        }

        Ok(cmd)
    }

    pub async fn download_audio(
        &self,
        target: &str,
        output_template: &Path,
        format: AudioFormat,
        quality: AudioQuality,
        embed_metadata: bool,
    ) -> Result<()> {
        let mut cmd = self.build_command().await?;
        let format_arg = match format {
            AudioFormat::Best => "best",
            fmt => fmt.extension(),
        };
        cmd.args([
            "-x",
            "--audio-format",
            format_arg,
            "--audio-quality",
            quality.bitrate_kbps(),
            "-o",
            output_template.to_str().unwrap_or_default(),
            "--no-playlist",
            "--no-warnings",
        ]);

        if embed_metadata {
            cmd.arg("--add-metadata");
        }

        cmd.arg(target);
        let output = cmd.output().await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(YoutubeAudioError::YtDlpFailed {
                status: output.status.code(),
                stderr,
            });
        }
        Ok(())
    }
}

impl MediaExtractor for YtDlpExtractor {
    fn extract<'a>(
        &'a self,
        target: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<ExtractedMedia>> + Send + 'a>> {
        Box::pin(async move {
            let mut cmd = self.build_command().await?;
            cmd.args([
                "-j",
                "-f",
                "bestaudio/best",
                "--no-playlist",
                "--no-warnings",
            ]);
            cmd.arg(target);

            let output = cmd.output().await?;
            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                return Err(YoutubeAudioError::YtDlpFailed {
                    status: output.status.code(),
                    stderr,
                });
            }

            let json: serde_json::Value = serde_json::from_slice(&output.stdout)?;
            let video_id = json
                .get("id")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(|| extract_video_id(target).unwrap_or_default());

            let raw_title = json
                .get("title")
                .and_then(|v| v.as_str())
                .unwrap_or("YouTube Audio");

            let uploader = json
                .get("uploader")
                .or_else(|| json.get("channel"))
                .and_then(|v| v.as_str())
                .unwrap_or("YouTube");

            let (author, title) = if let Some((artist, song)) = raw_title.split_once(" - ") {
                (artist.trim().to_string(), song.trim().to_string())
            } else {
                (uploader.to_string(), raw_title.to_string())
            };

            let stream_url = json
                .get("url")
                .and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_default();

            if stream_url.is_empty() {
                return Err(YoutubeAudioError::NoAudioStreamFound);
            }

            let metadata = VideoMetadata {
                id: video_id,
                title,
                author,
                duration_seconds: json.get("duration").and_then(|v| v.as_u64()).unwrap_or(0),
                view_count: json.get("view_count").and_then(|v| v.as_u64()).unwrap_or(0),
                thumbnail_url: json
                    .get("thumbnail")
                    .and_then(|v| v.as_str())
                    .map(String::from),
                description: json
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(String::from),
            };

            let content_length = json
                .get("filesize")
                .or_else(|| json.get("filesize_approx"))
                .and_then(|v| v.as_u64());

            let ext = json
                .get("ext")
                .and_then(|v| v.as_str())
                .unwrap_or("webm")
                .to_string();
            let streams = vec![AudioStreamInfo {
                url: stream_url,
                mime_type: format!("audio/{ext}"),
                bitrate: json.get("abr").and_then(|v| v.as_u64()).unwrap_or(128) as u32,
                sample_rate: json.get("asr").and_then(|v| v.as_u64()).map(|v| v as u32),
                content_length,
                container: ext,
                audio_codec: json
                    .get("acodec")
                    .and_then(|v| v.as_str())
                    .unwrap_or("opus")
                    .to_string(),
            }];

            Ok(ExtractedMedia { metadata, streams })
        })
    }
}
