use std::{future::Future, pin::Pin};

use reqwest::Client;
use serde_json::Value;

use super::{
    id::{extract_playlist_id, extract_video_id},
    strategy::MediaExtractor,
};
use crate::{
    error::{Result, YoutubeAudioError},
    models::{AudioStreamInfo, ExtractedMedia, VideoMetadata},
};

pub struct InnertubeExtractor {
    client: Client,
}

impl InnertubeExtractor {
    pub fn new(client: Client) -> Self {
        Self { client }
    }

    async fn fetch_player_json(
        &self,
        video_id: &str,
        client_name: &str,
        client_version: &str,
    ) -> Result<Value> {
        let url = "https://www.youtube.com/youtubei/v1/player";
        let mut client_obj = serde_json::json!({
            "clientName": client_name,
            "clientVersion": client_version,
            "hl": "en",
            "gl": "US"
        });
        if client_name.starts_with("ANDROID") {
            client_obj["androidSdkVersion"] = serde_json::json!(34);
        } else if client_name == "IOS" {
            client_obj["deviceModel"] = serde_json::json!("iPhone16,2");
            client_obj["useragent"] = serde_json::json!("com.google.ios.youtube/19.29.1 (iPhone16,2; U; CPU iOS 17_5_1 like Mac OS X; en_US)");
            client_obj["osName"] = serde_json::json!("iOS");
            client_obj["osVersion"] = serde_json::json!("17.5.1.21F90");
        }
        let mut payload = serde_json::json!({
            "videoId": video_id,
            "context": {
                "client": client_obj
            }
        });

        if client_name == "WEB_EMBEDDED_PLAYER" {
            payload["thirdParty"] = serde_json::json!({
                "embedUrl": format!("https://www.youtube.com/embed/{video_id}")
            });
        }

        let user_agent = match client_name {
            "ANDROID_MUSIC" => "com.google.android.apps.youtube.music/6.42.52 (Linux; U; Android 14; en_US)",
            "ANDROID" => "com.google.android.youtube/19.11.38 (Linux; U; Android 14; en_US)",
            "ANDROID_VR" => "com.google.android.apps.youtube.vr/1.56.21 (Linux; U; Android 12; en_US)",
            "IOS" => "com.google.ios.youtube/19.29.1 (iPhone16,2; U; CPU iOS 17_5_1 like Mac OS X; en_US)",
            "MWEB" => "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1",
            _ => "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36",
        };

        let json: Value = self
            .client
            .post(url)
            .header("User-Agent", user_agent)
            .json(&payload)
            .send()
            .await?
            .json()
            .await?;

        Ok(json)
    }

    pub async fn fetch_playlist(&self, target: &str) -> Result<Vec<VideoMetadata>> {
        let playlist_id = extract_playlist_id(target).ok_or_else(|| {
            YoutubeAudioError::InvalidUrl("Not a valid playlist URL or ID".into())
        })?;

        let browse_id = if playlist_id.starts_with("VL") {
            playlist_id
        } else {
            format!("VL{}", playlist_id)
        };

        let url = "https://www.youtube.com/youtubei/v1/browse";
        let payload = serde_json::json!({
            "browseId": browse_id,
            "context": {
                "client": {
                    "clientName": "ANDROID_VR",
                    "clientVersion": "1.56.21",
                    "hl": "en",
                    "gl": "US"
                }
            }
        });

        let json: Value = self
            .client
            .post(url)
            .json(&payload)
            .send()
            .await?
            .json()
            .await?;

        let mut tracks = Vec::new();
        if let Some(contents) = json.pointer("/contents/twoColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/itemSectionRenderer/contents/0/playlistVideoListRenderer/contents")
            .or_else(|| json.pointer("/contents/singleColumnBrowseResultsRenderer/tabs/0/tabRenderer/content/sectionListRenderer/contents/0/itemSectionRenderer/contents/0/playlistVideoListRenderer/contents"))
            .and_then(|v| v.as_array())
        {
            for item in contents {
                if let Some(video) = item.get("playlistVideoRenderer") {
                    let vid = video.get("videoId").and_then(|v| v.as_str()).unwrap_or("");
                    if vid.is_empty() { continue; }
                    let title = video.pointer("/title/runs/0/text").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string();
                    let author = video.pointer("/shortBylineText/runs/0/text").and_then(|v| v.as_str()).unwrap_or("Unknown").to_string();
                    let duration_seconds = video.get("lengthSeconds").and_then(|v| v.as_str()).and_then(|s| s.parse().ok()).unwrap_or(0);
                    let thumbnail_url = video.pointer("/thumbnail/thumbnails").and_then(|arr| arr.as_array()).and_then(|arr| arr.last()).and_then(|t| t.get("url")).and_then(|u| u.as_str()).map(|s| s.to_string());

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

        Ok(tracks)
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<VideoMetadata>> {
        let url = "https://www.youtube.com/youtubei/v1/search";
        let payload = serde_json::json!({
            "query": query,
            "context": {
                "client": {
                    "clientName": "WEB",
                    "clientVersion": "2.20231201.00.00",
                    "hl": "en",
                    "gl": "US"
                }
            }
        });

        let json: Value = self
            .client
            .post(url)
            .json(&payload)
            .send()
            .await?
            .json()
            .await?;

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
                                    return Ok(tracks);
                                }
                            }
                        }
                    }
                }
            }
        }

        Ok(tracks)
    }
}

fn parse_duration_str(s: &str) -> u64 {
    let parts: Vec<&str> = s.split(':').collect();
    let mut secs: u64 = 0;
    for part in parts {
        if let Ok(num) = part.parse::<u64>() {
            secs = secs * 60 + num;
        }
    }
    secs
}

fn parse_player_response(video_id: &str, json: &Value) -> Result<ExtractedMedia> {
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
                        url: direct_url.to_string(),
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

impl MediaExtractor for InnertubeExtractor {
    fn extract<'a>(
        &'a self,
        target: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<ExtractedMedia>> + Send + 'a>> {
        Box::pin(async move {
            let video_id = extract_video_id(target)?;
            let client_configs = [("ANDROID_VR", "1.56.21"), ("ANDROID", "19.11.38")];

            let futures: Vec<_> = client_configs
                .iter()
                .map(|(c_name, c_ver)| {
                    let v_id = video_id.clone();
                    Box::pin(async move {
                        let json = self.fetch_player_json(&v_id, c_name, c_ver).await?;
                        match parse_player_response(&v_id, &json) {
                            Ok(media) => {
                                if media.streams.is_empty() {
                                    Err(YoutubeAudioError::NoAudioStreamFound)
                                } else {
                                    Ok(media)
                                }
                            }
                            Err(err) => Err(err),
                        }
                    })
                })
                .collect();

            if let Ok((media, _)) = futures_util::future::select_ok(futures).await {
                return Ok(media);
            }

            Err(YoutubeAudioError::NoAudioStreamFound)
        })
    }
}
