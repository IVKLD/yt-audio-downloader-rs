use futures_util::StreamExt;
use yt_audio_downloader::{
    AudioFormat, AudioQuality, YoutubeAudioDownloader, download_audio, extractor::extract_video_id,
    get_stream_info, stream_audio,
};

const TEST_URL: &str = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
const TEST_VIDEO_ID: &str = "dQw4w9WgXcQ";

#[test]
fn test_video_id_extraction() {
    let id1 = extract_video_id(TEST_URL).unwrap();
    assert_eq!(id1, TEST_VIDEO_ID);

    let id2 = extract_video_id(TEST_VIDEO_ID).unwrap();
    assert_eq!(id2, TEST_VIDEO_ID);

    let id3 = extract_video_id("https://youtu.be/dQw4w9WgXcQ?si=xyz").unwrap();
    assert_eq!(id3, TEST_VIDEO_ID);

    assert!(extract_video_id("invalid_url_string").is_err());
}

#[tokio::test]
async fn test_fetch_metadata_and_stream_info() {
    let response = get_stream_info(TEST_URL).await.unwrap();

    assert_eq!(response.metadata.id, TEST_VIDEO_ID);
    assert!(!response.metadata.title.is_empty());
    assert!(!response.metadata.author.is_empty());
    assert!(!response.stream_url.is_empty());
}

#[tokio::test]
async fn test_audio_streaming_bytes() {
    let (metadata, mut stream) = stream_audio(TEST_URL).await.unwrap();

    assert_eq!(metadata.id, TEST_VIDEO_ID);

    let mut total_bytes = 0;
    let mut chunks_count = 0;

    while let Some(chunk_res) = stream.next().await {
        let chunk = chunk_res.unwrap();
        total_bytes += chunk.len();
        chunks_count += 1;

        if chunks_count >= 5 {
            break;
        }
    }

    assert!(total_bytes > 0);
    assert!(chunks_count > 0);
}

#[tokio::test]
async fn test_full_audio_download() {
    let temp_dir = "target/test_downloads";
    let downloaded = download_audio(TEST_URL, temp_dir).await.unwrap();

    assert!(downloaded.file_path.exists());
    assert!(downloaded.file_size_bytes > 0);
    assert_eq!(downloaded.format, AudioFormat::Mp3);

    let _ = std::fs::remove_file(&downloaded.file_path);
}

#[tokio::test]
async fn test_downloader_custom_options() {
    let temp_dir = "target/test_custom_downloads";
    let downloader = YoutubeAudioDownloader::new()
        .output_dir(temp_dir)
        .format(AudioFormat::M4a)
        .quality(AudioQuality::High);

    let result = downloader.download(TEST_URL).await.unwrap();

    assert!(result.file_path.exists());
    assert!(result.file_size_bytes > 0);
    assert_eq!(result.format, AudioFormat::M4a);

    let _ = std::fs::remove_file(&result.file_path);
}

#[tokio::test]
async fn test_search_youtube() {
    let results = yt_audio_downloader::search_youtube("Never Gonna Give You Up", 5)
        .await
        .unwrap();
    assert!(!results.is_empty());
}

#[tokio::test]
async fn test_bench_streams() {
    let client = yt_audio_downloader::Client::new().await.unwrap();
    let video_ids = [
        "dQw4w9WgXcQ",
        "kJQP7kiw5Fk",
        "9bZkp7q19f0",
        "JGwWNGJdvx8",
    ];

    for vid in video_ids {
        let start = std::time::Instant::now();
        let res = client.resolve_stream(vid).await;
        let elapsed = start.elapsed();
        match res {
            Ok(info) => {
                assert!(!info.stream_url.is_empty());
                assert!(elapsed < std::time::Duration::from_millis(1000));
            }
            Err(e) => {
                panic!("Video {vid}: FAILED in {:?} with error: {e}", elapsed);
            }
        }
    }
}

#[tokio::test]
async fn test_client_builder_and_operations() {
    use std::sync::{Arc, Mutex};

    use yt_audio_downloader::{ClientBuilder, ProxyUrlProvider};

    let proxy_state = Arc::new(Mutex::new(None::<String>));
    let proxy_state_clone = proxy_state.clone();
    let provider = ProxyUrlProvider(Arc::new(move || proxy_state_clone.lock().unwrap().clone()));

    let client = ClientBuilder::new()
        .with_max_retries(2)
        .with_proxy_provider(provider)
        .build()
        .await
        .unwrap();

    let meta = client.fetch_metadata(TEST_URL).await.unwrap();
    assert_eq!(meta.id, TEST_VIDEO_ID);
    assert!(!meta.title.is_empty());

    let stream_res = client.resolve_stream(TEST_URL).await.unwrap();
    assert_eq!(stream_res.metadata.id, TEST_VIDEO_ID);
    assert!(!stream_res.stream_url.is_empty());

    let (results, _) = client
        .search_page("Never Gonna Give You Up", None)
        .await
        .unwrap();
    assert!(!results.is_empty());
}
