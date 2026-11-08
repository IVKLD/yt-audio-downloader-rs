use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use tokio::{
    fs,
    io::{AsyncWriteExt, BufWriter},
};

use crate::{
    converter::sanitize_filename,
    error::{Result, YoutubeAudioError},
    models::{AudioFormat, VideoMetadata},
    streamer::AudioByteStream,
};

/// File system utilities and helpers for managing audio files, directories, and streams.
pub struct FileService;

impl FileService {
    /// Ensure that a directory exists, creating all parent components if necessary.
    pub async fn ensure_dir(path: &Path) -> Result<()> {
        fs::create_dir_all(path).await?;
        Ok(())
    }

    /// Ensure that the parent directory of a file path exists.
    pub async fn ensure_parent_dir(file_path: &Path) -> Result<()> {
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).await?;
        }
        Ok(())
    }

    /// Safely remove a file from disk if it exists, ignoring `NotFound`.
    pub async fn remove_file_if_exists(path: &Path) -> Result<()> {
        match fs::remove_file(path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }

    /// Read the file size in bytes.
    pub async fn file_size(path: &Path) -> Result<u64> {
        let meta = fs::metadata(path).await?;
        Ok(meta.len())
    }

    /// Copy a file from source to destination, creating parent directories if needed.
    pub async fn copy(from: &Path, to: &Path) -> Result<u64> {
        Self::ensure_parent_dir(to).await?;
        let bytes = fs::copy(from, to).await?;
        Ok(bytes)
    }

    /// Build a path for a temporary media stream download.
    pub fn temp_file_path(dir: &Path, video_id: &str, extension: &str) -> PathBuf {
        dir.join(format!("temp_{video_id}.{extension}"))
    }

    /// Resolve the final target path for an audio download, sanitizing title and author.
    pub fn resolve_target_path(
        output_dir: &Path,
        override_file: Option<&Path>,
        metadata: &VideoMetadata,
        format: AudioFormat,
    ) -> PathBuf {
        if let Some(path) = override_file {
            return path.to_path_buf();
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
        let final_stem = if sanitized.is_empty() || sanitized.chars().all(|c| c == '_') {
            &metadata.id
        } else {
            &sanitized
        };

        output_dir.join(format!("{final_stem}.{}", format.extension()))
    }

    /// Writes an incoming asynchronous byte stream into a file on disk, invoking the progress callback on each chunk.
    pub async fn write_stream_to_file<F>(
        output_path: &Path,
        mut stream: AudioByteStream,
        total_size: Option<u64>,
        mut on_progress: F,
    ) -> Result<u64>
    where
        F: FnMut(u64, Option<u64>),
    {
        Self::ensure_parent_dir(output_path).await?;
        let mut writer = BufWriter::new(fs::File::create(output_path).await?);
        let mut downloaded: u64 = 0;

        while let Some(chunk_res) = stream.next().await {
            let chunk = chunk_res?;
            writer.write_all(&chunk).await?;
            downloaded += chunk.len() as u64;
            on_progress(downloaded, total_size);
        }

        writer.flush().await?;
        Ok(downloaded)
    }

    /// Search for a downloaded fallback output file inside a directory matching a specific stem.
    pub async fn find_fallback_file(dir: &Path, stem: &str) -> Result<PathBuf> {
        let mut entries = fs::read_dir(dir).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or_default();
            if name.starts_with(stem) && !name.ends_with(".part") && !name.ends_with(".ytdl") {
                return Ok(path);
            }
        }
        Err(YoutubeAudioError::DownloadFailed(
            "Downloaded file not found".into(),
        ))
    }
}
