use yt_downloader_rs::{AudioFormat, AudioQuality, ProgressEvent, YoutubeAudioDownloader};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";

    let downloader = YoutubeAudioDownloader::new()
        .output_dir("audio_output")
        .format(AudioFormat::Flac)
        .quality(AudioQuality::Best)
        .embed_metadata(true)
        .on_progress(|event| match event {
            ProgressEvent::Initializing { video_id } => {
                println!("[+] Initializing download for Video ID: {}", video_id);
            }
            ProgressEvent::MetadataFetched { title, author } => {
                println!("[+] Found: '{}' by '{}'", title, author);
            }
            ProgressEvent::Downloading {
                bytes_downloaded,
                percentage,
                ..
            } => {
                if let Some(pct) = percentage {
                    print!(
                        "\r[>] Downloading: {:.1}% ({} bytes)",
                        pct, bytes_downloaded
                    );
                } else {
                    print!("\r[>] Downloading: {} bytes", bytes_downloaded);
                }
            }
            ProgressEvent::Converting { target_format } => {
                println!("\n[*] Converting audio to {} format...", target_format);
            }
            ProgressEvent::Finished {
                output_path,
                total_bytes,
            } => {
                println!(
                    "\n[✓] Successfully downloaded to {:?} ({} bytes)",
                    output_path, total_bytes
                );
            }
            ProgressEvent::Error { message } => {
                eprintln!("\n[!] Error: {}", message);
            }
        });

    let metadata = downloader.fetch_metadata(url).await?;
    println!("Video Title: {}", metadata.title);
    println!("Duration: {} seconds", metadata.duration_seconds);

    let result = downloader.download(url).await?;
    println!("\nAudio file successfully saved at: {:?}", result.file_path);

    Ok(())
}
