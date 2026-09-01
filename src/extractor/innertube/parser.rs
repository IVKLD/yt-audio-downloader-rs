use serde_json::Value;

use crate::{
    error::{Result, YoutubeAudioError},
    models::{AudioStreamInfo, ExtractedMedia, VideoMetadata},
};

pub fn parse_duration_str(s: &str) -> u64 {
    let parts: Vec<&str> = s.split(':').collect();
    let mut secs: u64 = 0;
    for part in parts {
        if let Ok(num) = part.parse::<u64>() {
            secs = secs * 60 + num;
        }
    }
    secs
}

pub fn parse_player_response(video_id: &str, json: &Value) -> Result<ExtractedMedia> {
    let details = json
        .get("videoDetails")
        .ok_or_else(|| YoutubeAudioError::DownloadFailed("Missing videoDetails".into()))?;

    let metadata = VideoMetadata {
        id: video_id.to_string(),
        title: details
            .get("title")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string(),
        author: details
            .get("author")
            .and_then(|v| v.as_str())
            .unwrap_or("Unknown")
            .to_string(),
        duration_seconds: details
            .get("lengthSeconds")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        view_count: details
            .get("viewCount")
            .and_then(|v| v.as_str())
            .and_then(|s| s.parse().ok())
            .unwrap_or(0),
        thumbnail_url: details
            .get("thumbnail")
            .and_then(|t| t.get("thumbnails"))
            .and_then(|arr| arr.as_array())
            .and_then(|arr| arr.last())
            .and_then(|item| item.get("url"))
            .and_then(|u| u.as_str())
            .map(|s| s.to_string()),
        description: details
            .get("shortDescription")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string()),
    };

    let mut streams = Vec::new();
    if let Some(formats) = json
        .get("streamingData")
        .and_then(|s| s.get("adaptiveFormats"))
        .and_then(|v| v.as_array())
    {
        for fmt in formats {
            let mime = fmt.get("mimeType").and_then(|v| v.as_str()).unwrap_or("");
            if mime.starts_with("audio/") {
                let url_opt = fmt
                    .get("url")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| {
                        fmt.get("signatureCipher")
                            .or_else(|| fmt.get("cipher"))
                            .and_then(|v| v.as_str())
                            .and_then(|cipher_str| {
                                url::form_urlencoded::parse(cipher_str.as_bytes())
                                    .find(|(k, _)| k == "url")
                                    .map(|(_, v)| v.into_owned())
                            })
                    });

                if let Some(direct_url) = url_opt {
                    let container = if mime.contains("webm") {
                        "webm"
                    } else if mime.contains("mp4") {
                        "m4a"
                    } else {
                        "audio"
                    };

                    let codec = if mime.contains("opus") {
                        "opus"
                    } else if mime.contains("mp4a") {
                        "aac"
                    } else {
                        "unknown"
                    };

                    streams.push(AudioStreamInfo {
                        url: direct_url,
                        mime_type: mime.to_string(),
                        bitrate: fmt.get("bitrate").and_then(|v| v.as_u64()).unwrap_or(0) as u32,
                        sample_rate: fmt
                            .get("audioSampleRate")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse().ok()),
                        content_length: fmt
                            .get("contentLength")
                            .and_then(|v| v.as_str())
                            .and_then(|s| s.parse().ok()),
                        container: container.to_string(),
                        audio_codec: codec.to_string(),
                    });
                }
            }
        }
    }

    streams.sort_by(|a, b| b.bitrate.cmp(&a.bitrate));

    if streams.is_empty() {
        return Err(YoutubeAudioError::NoAudioStreamFound);
    }

    Ok(ExtractedMedia { metadata, streams })
}

pub fn parse_playlist_contents(json: &Value) -> Vec<VideoMetadata> {
    let mut tracks = Vec::new();
    let contents = json
        .pointer("/contents/twoColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/itemSectionRenderer/contents/0/playlistVideoListRenderer/contents")
        .or_else(|| json.pointer("/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/itemSectionRenderer/contents/0/playlistVideoListRenderer/contents"))
        .and_then(|v| v.as_array());

    if let Some(contents) = contents {
        for item in contents {
            if let Some(video) = item.get("playlistVideoRenderer") {
                let vid = video.get("videoId").and_then(|v| v.as_str()).unwrap_or("");
                if vid.is_empty() {
                    continue;
                }
                let title = video
                    .pointer("/title/runs/0/text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown")
                    .to_string();
                let author = video
                    .pointer("/shortBylineText/runs/0/text")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Unknown")
                    .to_string();
                let duration_seconds = video
                    .get("lengthSeconds")
                    .and_then(|v| v.as_str())
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);
                let thumbnail_url = video
                    .pointer("/thumbnail/thumbnails")
                    .and_then(|arr| arr.as_array())
                    .and_then(|arr| arr.last())
                    .and_then(|t| t.get("url"))
                    .and_then(|u| u.as_str())
                    .map(|s| s.to_string());

                tracks.push(VideoMetadata {
                    id: vid.to_string(),
                    title,
                    author,
                    duration_seconds,
                    view_count: 0,
                    thumbnail_url,
                    description: None,
                });
            }
        }
    }

    tracks
}

pub fn parse_search_results(json: &Value, limit: usize) -> Vec<VideoMetadata> {
    let mut tracks = Vec::new();
    let sections = json
        .pointer("/contents/twoColumnSearchResultsRenderer/primaryContents/sectionListRenderer/contents")
        .or_else(|| json.pointer("/contents/sectionListRenderer/contents"))
        .and_then(|v| v.as_array());

    if let Some(sections) = sections {
        for section in sections {
            if let Some(contents) = section
                .pointer("/itemSectionRenderer/contents")
                .and_then(|v| v.as_array())
            {
                for item in contents {
                    if let Some(video) = item.get("videoRenderer") {
                        let vid = video.get("videoId").and_then(|v| v.as_str()).unwrap_or("");
                        if !vid.is_empty() {
                            let title = video
                                .pointer("/title/runs/0/text")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown")
                                .to_string();
                            let author = video
                                .pointer("/ownerText/runs/0/text")
                                .or_else(|| video.pointer("/longBylineText/runs/0/text"))
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown")
                                .to_string();
                            let duration_seconds = video
                                .get("lengthText")
                                .and_then(|v| v.get("simpleText"))
                                .or_else(|| video.pointer("/lengthText/runs/0/text"))
                                .and_then(|v| v.as_str())
                                .map(parse_duration_str)
                                .unwrap_or(0);

                            let thumbnail_url = video
                                .pointer("/thumbnail/thumbnails")
                                .and_then(|arr| arr.as_array())
                                .and_then(|arr| arr.last())
                                .and_then(|t| t.get("url"))
                                .and_then(|u| u.as_str())
                                .map(|s| s.to_string());

                            tracks.push(VideoMetadata {
                                id: vid.to_string(),
                                title,
                                author,
                                duration_seconds,
                                view_count: 0,
                                thumbnail_url,
                                description: None,
                            });

                            if tracks.len() >= limit {
                                return tracks;
                            }
                        }
                    }
                }
            }
        }
    }

    tracks
}
