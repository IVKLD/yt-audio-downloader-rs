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
    println!("Search results count: {}", results.len());
    for r in &results {
        println!(" - {} ({}) [{}]", r.title, r.author, r.id);
    }
    assert!(!results.is_empty());
}
