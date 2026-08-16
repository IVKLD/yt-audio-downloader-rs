use std::{future::Future, pin::Pin};

use tokio::process::Command;

use super::{id::extract_video_id, strategy::MediaExtractor};
use crate::{
    error::{Result, YoutubeAudioError},
    models::{AudioStreamInfo, ExtractedMedia, VideoMetadata},
};

#[derive(Default)]
pub struct YtDlpExtractor {
    pub proxy: Option<String>,
}

impl YtDlpExtractor {
    pub fn with_proxy(proxy: Option<String>) -> Self {
        Self { proxy }
    }

    async fn find_cmd() -> Result<String> {
        let home = std::env::var("HOME").unwrap_or_default();
        if !home.is_empty() {
            let user_bin = format!("{home}/.local/bin/yt-dlp");
            if std::path::Path::new(&user_bin).exists() {
                return Ok(user_bin);
            }
        }
        if std::path::Path::new("./yt-dlp").exists() {
            return Ok("./yt-dlp".to_string());
        }
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(parent) = exe_path.parent() {
                let local_exe = parent.join("yt-dlp");
                if local_exe.exists() {
                    return Ok(local_exe.to_string_lossy().to_string());
                }
            }
        }
        if Command::new("yt-dlp")
            .arg("--version")
            .output()
            .await
            .is_ok()
        {
            return Ok("yt-dlp".to_string());
        }
        Err(YoutubeAudioError::YtDlpNotFound)
    }

    async fn find_node_cmd() -> Option<String> {
        let home = std::env::var("HOME").unwrap_or_default();
        let candidates = [
            "node".to_string(),
            "/usr/bin/node".to_string(),
            "/usr/local/bin/node".to_string(),
            format!("{home}/.nvm/versions/node/current/bin/node"),
        ];
        for candidate in candidates {
            if Command::new(&candidate)
                .arg("--version")
                .output()
                .await
                .is_ok()
            {
                return Some(candidate);
            }
        }
        None
    }
}

impl MediaExtractor for YtDlpExtractor {
    fn extract<'a>(
        &'a self,
        target: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<ExtractedMedia>> + Send + 'a>> {
        Box::pin(async move {
            let cmd = Self::find_cmd().await?;
            let mut command = Command::new(&cmd);
            command.kill_on_drop(true);
            command.args([
                "-j",
                "-f",
                "bestaudio/best",
                "--no-playlist",
                "--no-warnings",
            ]);

            if let Some(node_path) = Self::find_node_cmd().await {
                command.args(["--js-runtimes", &format!("node:{node_path}")]);
            } else {
                command.args(["--js-runtimes", "node"]);
            }

            if let Some(ref proxy) = self.proxy {
                if !proxy.is_empty() {
                    command.args(["--proxy", proxy]);
                }
            }

            let home = std::env::var("HOME").unwrap_or_default();
            let mut browser = None;
            if std::path::Path::new(&format!("{home}/.mozilla/firefox")).exists()
                || std::path::Path::new(&format!("{home}/.snap/firefox")).exists()
            {
                browser = Some("firefox");
            } else if std::path::Path::new(&format!("{home}/.config/google-chrome")).exists() {
                browser = Some("chrome");
            } else if std::path::Path::new(&format!("{home}/.config/chromium")).exists() {
                browser = Some("chromium");
            }

            if let Some(b) = browser {
                command.args(["--cookies-from-browser", b]);
            } else if std::path::Path::new("./cookies.txt")
                .metadata()
                .map(|m| m.len() > 0)
                .unwrap_or(false)
            {
                command.args(["--cookies", "./cookies.txt"]);
            } else {
                let config_cookies = format!("{home}/.config/vortex-dl/cookies.txt");
                if !home.is_empty()
                    && std::path::Path::new(&config_cookies)
                        .metadata()
                        .map(|m| m.len() > 0)
                        .unwrap_or(false)
                {
                    command.args(["--cookies", &config_cookies]);
                }
            }

            command.arg(target);
            let output = command.output().await?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                return Err(YoutubeAudioError::YtDlpFailed {
                    status: output.status.code(),
                    stderr,
                });
            }

            if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&output.stdout) {
                let video_id = json
                    .get("id")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_else(|| extract_video_id(target).unwrap_or_default());

                let raw_title = json
                    .get("title")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("YouTube Audio");

                let uploader = json
                    .get("uploader")
                    .or_else(|| json.get("channel"))
                    .or_else(|| json.get("artist"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("YouTube");

                let (author, title) = if let Some((artist, song)) = raw_title.split_once(" - ") {
                    let a = artist.trim();
                    let s = song.trim();
                    if !a.is_empty() && !s.is_empty() {
                        (a.to_string(), s.to_string())
                    } else {
                        (uploader.to_string(), raw_title.to_string())
                    }
                } else {
                    (uploader.to_string(), raw_title.to_string())
                };

                let duration_seconds = json.get("duration").and_then(|v| v.as_u64()).unwrap_or(0);
                let view_count = json.get("view_count").and_then(|v| v.as_u64()).unwrap_or(0);
                let thumbnail_url = json
                    .get("thumbnail")
                    .and_then(|v| v.as_str())
                    .map(String::from);
                let description = json
                    .get("description")
                    .and_then(|v| v.as_str())
                    .map(String::from);

                let stream_url = json
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_default();

                if !stream_url.is_empty() {
                    let metadata = VideoMetadata {
                        id: video_id,
                        title,
                        author,
                        duration_seconds,
                        view_count,
                        thumbnail_url,
                        description,
                    };
                    let ext = json
                        .get("ext")
                        .and_then(|v| v.as_str())
                        .unwrap_or("webm")
                        .to_string();
                    let acodec = json
                        .get("acodec")
                        .and_then(|v| v.as_str())
                        .unwrap_or("opus")
                        .to_string();
                    let abr = json.get("abr").and_then(|v| v.as_u64()).unwrap_or(128) as u32;
                    let asr = json.get("asr").and_then(|v| v.as_u64()).map(|v| v as u32);

                    let streams = vec![AudioStreamInfo {
                        url: stream_url,
                        mime_type: format!("audio/{ext}"),
                        bitrate: abr,
                        sample_rate: asr,
                        content_length: None,
                        container: ext,
                        audio_codec: acodec,
                    }];
                    return Ok(ExtractedMedia { metadata, streams });
                }
            }

            Err(YoutubeAudioError::NoAudioStreamFound)
        })
    }
}
