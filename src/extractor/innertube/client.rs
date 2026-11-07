use std::sync::LazyLock;

use regex::Regex;
use reqwest::{Client, header::COOKIE};
use serde_json::{Value, json};
use tokio::sync::RwLock;

use crate::{
    error::{Result, YoutubeAudioError},
    http::YOUTUBE_COOKIE_HEADER,
};

static VISITOR_DATA: LazyLock<RwLock<Option<String>>> = LazyLock::new(|| RwLock::new(None));
static VISITOR_DATA_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#""VISITOR_DATA"\s*:\s*"([^"]+)""#).expect("valid regex"));

pub async fn get_cached_visitor_data(http: &Client) -> Option<String> {
    if let Some(ref vd) = *VISITOR_DATA.read().await {
        return Some(vd.clone());
    }

    let mut write_guard = VISITOR_DATA.write().await;
    if let Some(ref vd) = *write_guard {
        return Some(vd.clone());
    }

    if let Ok(resp) = http
        .get("https://www.youtube.com")
        .header(
            "User-Agent",
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36",
        )
        .send()
        .await
        && let Ok(text) = resp.text().await
        && let Some(caps) = VISITOR_DATA_RE.captures(&text)
        && let Some(vd) = caps.get(1)
    {
        let data = vd.as_str().to_string();
        *write_guard = Some(data.clone());
        return Some(data);
    }

    None
}

pub struct InnertubeClient<'a> {
    pub http: &'a Client,
}

impl<'a> InnertubeClient<'a> {
    pub fn new(http: &'a Client) -> Self {
        Self { http }
    }

    pub async fn fetch_player_json(
        &self,
        video_id: &str,
        client_name: &str,
        client_version: &str,
    ) -> Result<Value> {
        let url = "https://www.youtube.com/youtubei/v1/player";
        let visitor_id = get_cached_visitor_data(self.http).await;

        let mut client_obj = json!({
            "clientName": client_name,
            "clientVersion": client_version,
            "hl": "en",
            "gl": "US"
        });

        if let Some(ref vd) = visitor_id {
            client_obj["visitorData"] = json!(vd);
        }

        if client_name == "ANDROID_VR" {
            client_obj["clientVersion"] = json!("1.65.10");
            client_obj["deviceMake"] = json!("Oculus");
            client_obj["deviceModel"] = json!("Quest 3");
            client_obj["androidSdkVersion"] = json!(32);
            client_obj["userAgent"] = json!(
                "com.google.android.apps.youtube.vr.oculus/1.65.10 (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip"
            );
            client_obj["osName"] = json!("Android");
            client_obj["osVersion"] = json!("12L");
            client_obj["timeZone"] = json!("UTC");
            client_obj["utcOffsetMinutes"] = json!(0);
        } else if client_name.starts_with("ANDROID") {
            client_obj["androidSdkVersion"] = json!(34);
        } else if client_name == "IOS" {
            client_obj["deviceModel"] = json!("iPhone16,2");
            client_obj["useragent"] = json!(
                "com.google.ios.youtube/19.29.1 (iPhone16,2; U; CPU iOS 17_5_1 like Mac OS X; en_US)"
            );
            client_obj["osName"] = json!("iOS");
            client_obj["osVersion"] = json!("17.5.1.21F90");
        }

        let mut payload = json!({
            "videoId": video_id,
            "context": {
                "client": client_obj
            },
            "contentCheckOk": true,
            "racyCheckOk": true,
            "playbackContext": {
                "contentPlaybackContext": {
                    "html5Preference": "HTML5_PREF_WANTS",
                    "signatureTimestamp": 20760
                }
            }
        });

        if client_name == "WEB_EMBEDDED_PLAYER" {
            payload["thirdParty"] = json!({
                "embedUrl": format!("https://www.youtube.com/embed/{video_id}")
            });
        }

        let user_agent = match client_name {
            "ANDROID_VR" => {
                "com.google.android.apps.youtube.vr.oculus/1.65.10 (Linux; U; Android 12L; eureka-user Build/SQ3A.220605.009.A1) gzip"
            }
            "ANDROID_MUSIC" => {
                "com.google.android.apps.youtube.music/6.42.52 (Linux; U; Android 14; en_US)"
            }
            "ANDROID" => "com.google.android.youtube/19.11.38 (Linux; U; Android 14; en_US)",
            "IOS" => {
                "com.google.ios.youtube/19.29.1 (iPhone16,2; U; CPU iOS 17_5_1 like Mac OS X; en_US)"
            }
            "MWEB" => {
                "Mozilla/5.0 (iPhone; CPU iPhone OS 17_5_1 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/17.5 Mobile/15E148 Safari/604.1"
            }
            _ => {
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36"
            }
        };

        let mut req = self
            .http
            .post(url)
            .header("User-Agent", user_agent)
            .header("Origin", "https://www.youtube.com");

        if client_name == "ANDROID_VR" {
            req = req
                .header("X-YouTube-Client-Name", "28")
                .header("X-YouTube-Client-Version", "1.65.10");
        }

        if let Some(ref vd) = visitor_id {
            req = req.header("X-Goog-Visitor-Id", vd);
        }

        // Only attach desktop cookies to web clients. Sending desktop cookies to mobile/VR clients
        // causes YouTube Innertube to return HTTP 400 (FAILED_PRECONDITION).
        if client_name.starts_with("WEB") || client_name == "MWEB" {
            if let Some(ref cookies) = *YOUTUBE_COOKIE_HEADER {
                req = req.header(COOKIE, cookies);
            }
        }

        let resp = req.json(&payload).send().await?;
        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let message = resp.text().await.unwrap_or_default();
            return Err(YoutubeAudioError::HttpStatus { status, message });
        }

        let json: Value = resp.json().await?;
        Ok(json)
    }

    pub async fn fetch_browse_json(&self, browse_id: &str) -> Result<Value> {
        let url = "https://www.youtube.com/youtubei/v1/browse";
        let payload = json!({
            "browseId": browse_id,
            "context": {
                "client": {
                    "clientName": "WEB",
                    "clientVersion": "2.20231201.00.00",
                    "hl": "en",
                    "gl": "US"
                }
            }
        });

        let mut req = self.http.post(url).json(&payload);
        if let Some(ref cookies) = *YOUTUBE_COOKIE_HEADER {
            req = req.header(COOKIE, cookies);
        }

        let resp = req.send().await?;
        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let message = resp.text().await.unwrap_or_default();
            return Err(YoutubeAudioError::HttpStatus { status, message });
        }

        let json: Value = resp.json().await?;
        Ok(json)
    }

    pub async fn fetch_search_json(&self, query: &str) -> Result<Value> {
        let url = "https://www.youtube.com/youtubei/v1/search";
        let payload = json!({
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

        let mut req = self.http.post(url).json(&payload);
        if let Some(ref cookies) = *YOUTUBE_COOKIE_HEADER {
            req = req.header(COOKIE, cookies);
        }

        let resp = req.send().await?;
        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let message = resp.text().await.unwrap_or_default();
            return Err(YoutubeAudioError::HttpStatus { status, message });
        }

        let json: Value = resp.json().await?;
        Ok(json)
    }

    pub async fn fetch_search_continuation_json(&self, continuation: &str) -> Result<Value> {
        let url = "https://www.youtube.com/youtubei/v1/search";
        let payload = json!({
            "continuation": continuation,
            "context": {
                "client": {
                    "clientName": "WEB",
                    "clientVersion": "2.20231201.00.00",
                    "hl": "en",
                    "gl": "US"
                }
            }
        });

        let mut req = self.http.post(url).json(&payload);
        if let Some(ref cookies) = *YOUTUBE_COOKIE_HEADER {
            req = req.header(COOKIE, cookies);
        }

        let resp = req.send().await?;
        let status = resp.status().as_u16();
        if !resp.status().is_success() {
            let message = resp.text().await.unwrap_or_default();
            return Err(YoutubeAudioError::HttpStatus { status, message });
        }

        let json: Value = resp.json().await?;
        Ok(json)
    }
}
