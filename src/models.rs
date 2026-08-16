use std::{fmt, path::PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AudioFormat {
    #[default]
    Mp3,
    M4a,
    Opus,
    Wav,
    Flac,
    Best,
}

impl AudioFormat {
    pub fn extension(&self) -> &'static str {
        match self {
            AudioFormat::Mp3 => "mp3",
            AudioFormat::M4a => "m4a",
            AudioFormat::Opus => "opus",
            AudioFormat::Wav => "wav",
            AudioFormat::Flac => "flac",
            AudioFormat::Best => "m4a",
        }
    }
}

impl fmt::Display for AudioFormat {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.extension())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AudioQuality {
    #[default]
    Best,
    High,
    Medium,
    Low,
}

impl AudioQuality {
    pub fn bitrate_kbps(&self) -> &'static str {
        match self {
            AudioQuality::Best => "0",
            AudioQuality::High => "320k",
            AudioQuality::Medium => "192k",
            AudioQuality::Low => "128k",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoMetadata {
    pub id: String,
    pub title: String,
    pub author: String,
    pub duration_seconds: u64,
    pub view_count: u64,
    pub thumbnail_url: Option<String>,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioStreamInfo {
    pub url: String,
    pub mime_type: String,
    pub bitrate: u32,
    pub sample_rate: Option<u32>,
    pub content_length: Option<u64>,
    pub container: String,
    pub audio_codec: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtractedMedia {
    pub metadata: VideoMetadata,
    pub streams: Vec<AudioStreamInfo>,
}

impl ExtractedMedia {
    pub fn best_stream(&self) -> Option<&AudioStreamInfo> {
        self.streams.first()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadedAudio {
    pub file_path: PathBuf,
    pub metadata: VideoMetadata,
    pub format: AudioFormat,
    pub file_size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioStreamResponse {
    pub metadata: VideoMetadata,
    pub stream_info: AudioStreamInfo,
    pub stream_url: String,
    pub mime_type: String,
    pub content_length: Option<u64>,
}
