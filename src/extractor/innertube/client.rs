use reqwest::Client;
use serde_json::Value;

use crate::error::Result;

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
            client_obj["useragent"] = serde_json::json!(
                "com.google.ios.youtube/19.29.1 (iPhone16,2; U; CPU iOS 17_5_1 like Mac OS X; en_US)"
            );
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

        let json: Value = self
            .http
            .post(url)
            .header("User-Agent", user_agent)
            .json(&payload)
            .send()
            .await?
            .json()
            .await?;

        Ok(json)
    }

    pub async fn fetch_browse_json(&self, browse_id: &str) -> Result<Value> {
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
            .http
            .post(url)
            .json(&payload)
            .send()
            .await?
            .json()
            .await?;

        Ok(json)
    }

    pub async fn fetch_search_json(&self, query: &str) -> Result<Value> {
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
            .http
            .post(url)
            .json(&payload)
            .send()
            .await?
            .json()
            .await?;

        Ok(json)
    }
}
