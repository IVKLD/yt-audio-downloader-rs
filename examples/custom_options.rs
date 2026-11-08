use std::sync::Arc;

use yt_audio_downloader::{AudioFormat, Client, ProgressCallback};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";

    let client = Client::new()?;

    let metadata = client.fetch_metadata(url).await?;
    println!("Video Title: {}", metadata.title);
    println!("Duration: {} seconds", metadata.duration_seconds);

    let progress_cb: ProgressCallback = Arc::new(|downloaded, total: Option<u64>| {
        if let Some(t) = total {
            print!("\r[>] Downloading: {:.1}% ({downloaded}/{t} bytes)", (downloaded as f64 / t as f64) * 100.0);
        } else {
            print!("\r[>] Downloading: {downloaded} bytes");
        }
    });

    let output_file = std::path::Path::new("audio_output/song.flac");
    let result = client
        .download(url, output_file, AudioFormat::Flac, Some(&progress_cb))
        .await?;
    println!("\nAudio file successfully saved at: {:?}", result.file_path);

    Ok(())
}
