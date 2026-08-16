use std::path::{Path, PathBuf};

use tokio::process::Command;

use crate::{
    error::{Result, YoutubeAudioError},
    models::{AudioFormat, AudioQuality, VideoMetadata},
};

pub struct AudioConverter;

impl AudioConverter {
    pub async fn is_ffmpeg_installed() -> bool {
        Command::new("ffmpeg")
            .arg("-version")
            .output()
            .await
            .map(|out| out.status.success())
            .unwrap_or(false)
    }

    pub async fn convert(
        input_path: &Path,
        output_dir: &Path,
        base_name: &str,
        format: AudioFormat,
        quality: AudioQuality,
        metadata: Option<&VideoMetadata>,
    ) -> Result<PathBuf> {
        let ext = format.extension();
        let file_name = format!("{}.{}", sanitize_filename(base_name), ext);
        let output_path = output_dir.join(file_name);

        if let Some(input_ext) = input_path.extension().and_then(|e| e.to_str()) {
            if input_ext.eq_ignore_ascii_case(ext) && format != AudioFormat::Mp3 {
                tokio::fs::copy(input_path, &output_path).await?;
                return Ok(output_path);
            }
        }

        if !Self::is_ffmpeg_installed().await {
            return Err(YoutubeAudioError::FFmpegNotFound);
        }

        let mut cmd = Command::new("ffmpeg");
        cmd.kill_on_drop(true);
        cmd.args(["-y", "-i"]).arg(input_path);

        match format {
            AudioFormat::Mp3 => {
                cmd.args(["-codec:a", "libmp3lame"]);
                if quality == AudioQuality::Best {
                    cmd.args(["-qscale:a", "0"]);
                } else {
                    cmd.args(["-b:a", quality.bitrate_kbps()]);
                }
            }
            AudioFormat::M4a => {
                let bitrate = match quality {
                    AudioQuality::Best | AudioQuality::High => "256k",
                    AudioQuality::Medium => "192k",
                    AudioQuality::Low => "128k",
                };
                cmd.args(["-codec:a", "aac", "-b:a", bitrate]);
            }
            AudioFormat::Opus => {
                cmd.args(["-codec:a", "libopus", "-b:a", quality.bitrate_kbps()]);
            }
            AudioFormat::Flac => {
                cmd.args(["-codec:a", "flac"]);
            }
            AudioFormat::Wav => {
                cmd.args(["-codec:a", "pcm_s16le"]);
            }
            AudioFormat::Best => {
                cmd.args(["-codec:a", "copy"]);
            }
        }

        if let Some(meta) = metadata {
            cmd.args(["-metadata", &format!("title={}", meta.title)]);
            cmd.args(["-metadata", &format!("artist={}", meta.author)]);
            cmd.args([
                "-metadata",
                &format!("comment=Downloaded from YouTube (ID: {})", meta.id),
            ]);
        }

        cmd.arg(&output_path);

        let output = cmd.output().await?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(YoutubeAudioError::FFmpegError(stderr));
        }

        Ok(output_path)
    }
}

pub fn sanitize_filename(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | '?' | '%' | '*' | ':' | '|' | '"' | '<' | '>' => '_',
            _ => c,
        })
        .collect::<String>()
        .trim()
        .to_string()
}
