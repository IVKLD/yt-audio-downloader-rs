use reqwest::{Client, header::COOKIE};
use serde_json::{Value, json};

use crate::{error::Result, http::YOUTUBE_COOKIE_HEADER};

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
        let mut client_obj = json!({
            "clientName": client_name,
            "clientVersion": client_version,
            "hl": "en",
            "gl": "US"
        });

        if client_name.starts_with("ANDROID") {
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
            }
        });

        if client_name == "WEB_EMBEDDED_PLAYER" {
            payload["thirdParty"] = json!({
                "embedUrl": format!("https://www.youtube.com/embed/{video_id}")
            });
        }

        let user_agent = match client_name {
            "ANDROID_MUSIC" => {
                "com.google.android.apps.youtube.music/6.42.52 (Linux; U; Android 14; en_US)"
            }
            "ANDROID" => "com.google.android.youtube/19.11.38 (Linux; U; Android 14; en_US)",
            "ANDROID_VR" => {
                "com.google.android.apps.youtube.vr/1.56.21 (Linux; U; Android 12; en_US)"
            }
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
            .json(&payload);

        if let Some(ref cookies) = *YOUTUBE_COOKIE_HEADER {
            req = req.header(COOKIE, cookies);
        }

        let json: Value = req.send().await?.json().await?;

        Ok(json)
    }

    pub async fn fetch_browse_json(&self, browse_id: &str) -> Result<Value> {
        let url = "https://www.youtube.com/youtubei/v1/browse";
        let payload = json!({
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

        let mut req = self.http.post(url).json(&payload);
        if let Some(ref cookies) = *YOUTUBE_COOKIE_HEADER {
            req = req.header(COOKIE, cookies);
        }

        let json: Value = req.send().await?.json().await?;

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

        let json: Value = req.send().await?.json().await?;

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

        let json: Value = req.send().await?.json().await?;

        Ok(json)
    }
}
