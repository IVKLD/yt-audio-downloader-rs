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
            "yt-dlp".to_string(),
            format!("{home}/.local/bin/yt-dlp"),
            "./yt-dlp".to_string(),
        ];
        for candidate in candidates {
            if let Ok(output) = Command::new(&candidate).arg("--version").output().await
                && output.status.success()
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
            "--socket-timeout",
            "15",
            "--extractor-args",
            "youtube:player_client=web_embedded,web;skip=hls,dash",
        ]);

        if let Some(ref proxy) = self.proxy
            && !proxy.is_empty()
        {
            cmd.args(["--proxy", proxy]);
        }

        if std::path::Path::new("./cookies.txt").exists() {
            cmd.args(["--cookies", "./cookies.txt"]);
        } else if let Some(cookie_path) = crate::cookies::ensure_youtube_cookies_available().await {
            cmd.args(["--cookies", cookie_path.to_str().unwrap_or_default()]);
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
            if is_auth_or_bot_error(&stderr) {
                tracing::info!(
                    "YouTube requested sign-in / verification. Attempting automatic cookie refresh from browser..."
                );
                if crate::cookies::auto_import_cookies(true).await.is_ok() {
                    let mut retry_cmd = self.build_command().await?;
                    retry_cmd.args([
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
                        retry_cmd.arg("--add-metadata");
                    }

                    retry_cmd.arg(target);
                    let retry_output = retry_cmd.output().await?;
                    if retry_output.status.success() {
                        return Ok(());
                    }
                    let retry_stderr = String::from_utf8_lossy(&retry_output.stderr).to_string();
                    return Err(YoutubeAudioError::YtDlpFailed {
                        status: retry_output.status.code(),
                        stderr: retry_stderr,
                    });
                }
            }
            return Err(YoutubeAudioError::YtDlpFailed {
                status: output.status.code(),
                stderr,
            });
        }
        Ok(())
    }
}

pub(crate) fn is_auth_or_bot_error(stderr: &str) -> bool {
    stderr.contains("Sign in to confirm your age")
        || stderr.contains("Sign in to confirm you’re not a bot")
        || stderr.contains("Sign in to confirm you're not a bot")
        || stderr.contains("Sign in to confirm")
        || stderr.contains("This video is only available to registered users")
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
            let output = if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                if is_auth_or_bot_error(&stderr) {
                    tracing::info!(
                        "YouTube requested sign-in / verification. Attempting automatic cookie refresh from browser..."
                    );
                    if crate::cookies::auto_import_cookies(true).await.is_ok() {
                        let mut retry_cmd = self.build_command().await?;
                        retry_cmd.args([
                            "-j",
                            "-f",
                            "bestaudio/best",
                            "--no-playlist",
                            "--no-warnings",
                        ]);
                        retry_cmd.arg(target);
                        retry_cmd.output().await?
                    } else {
                        return Err(YoutubeAudioError::YtDlpFailed {
                            status: output.status.code(),
                            stderr,
                        });
                    }
                } else {
                    return Err(YoutubeAudioError::YtDlpFailed {
                        status: output.status.code(),
                        stderr,
                    });
                }
            } else {
                output
            };

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
                .and_then(|v| v.as_u64())
                .or_else(|| json.get("filesize_approx").and_then(|v| v.as_u64()))
                .or_else(|| {
                    url::Url::parse(&stream_url).ok().and_then(|u| {
                        u.query_pairs()
                            .find(|(k, _)| k == "clen")
                            .map(|(_, v)| v.into_owned())
                            .and_then(|s| s.parse::<u64>().ok())
                    })
                });

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
