use yt_downloader_rs::download_audio;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";

    println!("Downloading audio from YouTube video: {}", url);

    let downloaded = download_audio(url, "music").await?;

    println!("\n=== Download Complete! ===");
    println!("Title: {}", downloaded.metadata.title);
    println!("Author: {}", downloaded.metadata.author);
    println!("Saved File Path: {:?}", downloaded.file_path);
    println!("File Size: {} bytes", downloaded.file_size_bytes);
    println!("Audio Format: {}", downloaded.format);

    Ok(())
}
