use std::{env, fs, path::PathBuf, sync::LazyLock, time::Duration};

use reqwest::{Client, Proxy};

pub static YOUTUBE_COOKIE_HEADER: LazyLock<Option<String>> =
    LazyLock::new(load_youtube_cookie_header);

pub fn load_youtube_cookie_header() -> Option<String> {
    let home = env::var("HOME").unwrap_or_default();
    let candidates = [
        PathBuf::from("./cookies.txt"),
        PathBuf::from(format!("{home}/.config/vortex-dl/cookies.txt")),
    ];

    for path in &candidates {
        if path.exists()
            && let Ok(content) = fs::read_to_string(path)
        {
            let mut cookies = Vec::new();
            for line in content.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with('#') || trimmed.is_empty() {
                    continue;
                }
                let parts: Vec<&str> = trimmed.split('\t').collect();
                if parts.len() >= 7 && parts[0].contains("youtube.com") {
                    cookies.push(format!("{}={}", parts[5], parts[6]));
                }
            }
            if !cookies.is_empty() {
                return Some(cookies.join("; "));
            }
        }
    }
    None
}

pub fn create_http_client() -> Client {
    create_http_client_with_proxy(None)
}

pub fn create_http_client_with_proxy(proxy_url: Option<&str>) -> Client {
    let mut builder = Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36")
        .tcp_nodelay(true)
        .pool_max_idle_per_host(20)
        .pool_idle_timeout(Duration::from_secs(120))
        .connect_timeout(Duration::from_secs(8))
        .read_timeout(Duration::from_secs(30));

    if let Some(proxy_str) = proxy_url
        && !proxy_str.is_empty()
        && let Ok(proxy) = Proxy::all(proxy_str)
    {
        builder = builder.proxy(proxy);
    }

    builder.build().unwrap_or_else(|_| Client::new())
}

pub fn select_user_agent_for_url(url: &str) -> &'static str {
    if url.contains("c=ANDROID_VR") {
        "com.google.android.apps.youtube.vr/1.56.21 (Linux; U; Android 12; en_US)"
    } else if url.contains("c=ANDROID_MUSIC") {
        "com.google.android.apps.youtube.music/6.42.52 (Linux; U; Android 14; en_US)"
    } else if url.contains("c=ANDROID") {
        "com.google.android.youtube/19.11.38 (Linux; U; Android 14; en_US)"
    } else if url.contains("c=IOS") {
        "com.google.ios.youtube/19.29.1 (iPhone16,2; U; CPU iOS 17_5_1 like Mac OS X; en_US)"
    } else {
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/123.0.0.0 Safari/537.36"
    }
}
