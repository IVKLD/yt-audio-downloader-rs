use reqwest::{Client, Proxy};

pub fn create_http_client() -> Client {
    create_http_client_with_proxy(None)
}

pub fn create_http_client_with_proxy(proxy_url: Option<&str>) -> Client {
    let mut builder = Client::builder()
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/122.0.0.0 Safari/537.36");

    if let Some(proxy_str) = proxy_url {
        if !proxy_str.is_empty() {
            if let Ok(proxy) = Proxy::all(proxy_str) {
                builder = builder.proxy(proxy);
            }
        }
    }

    builder.build().unwrap_or_else(|_| Client::new())
}
