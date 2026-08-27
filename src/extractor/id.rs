use regex::Regex;

use crate::error::{Result, YoutubeAudioError};

pub fn is_youtube_url(url_or_str: &str) -> bool {
    let trimmed = url_or_str.trim();
    if trimmed.contains("youtube.com") || trimmed.contains("youtu.be") {
        return true;
    }
    extract_video_id(trimmed).is_ok() || extract_playlist_id(trimmed).is_some()
}

pub fn youtube_id_to_i64(yt_id: &str) -> i64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in yt_id.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    let val = (hash & 0x001F_FFFF_FFFF_FFFF) as i64;
    if val == 0 { 1 } else { val }
}

pub fn extract_video_id(url_or_id: &str) -> Result<String> {
    let trimmed = url_or_id.trim();

    if trimmed.len() == 11
        && trimmed
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Ok(trimmed.to_string());
    }

    let re = Regex::new(
        r"(?:youtube\.com/(?:[^/]+/.+/|(?:v|e(?:mbed)?|shorts)/|.*[?&]v=)|youtu\.be/)([^?&/]{11})",
    )
    .map_err(|e| YoutubeAudioError::InvalidUrl(e.to_string()))?;

    re.captures(trimmed)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .ok_or_else(|| YoutubeAudioError::VideoIdNotFound(url_or_id.to_string()))
}

pub fn extract_playlist_id(url_or_id: &str) -> Option<String> {
    let trimmed = url_or_id.trim();

    if (trimmed.starts_with("PL") || trimmed.starts_with("OLAK5uy_") || trimmed.starts_with("VL"))
        && trimmed
            .chars()
            .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
    {
        return Some(trimmed.to_string());
    }

    if let Ok(parsed_url) = url::Url::parse(trimmed) {
        for (key, val) in parsed_url.query_pairs() {
            if key == "list" && !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }

    let re = Regex::new(r"[?&]list=([a-zA-Z0-9_-]+)").ok()?;
    re.captures(trimmed)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}
