use futures_util::StreamExt;
use yt_downloader_rs::{get_stream_info, stream_audio};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";

    let info = get_stream_info(url).await?;
    println!("Title: {}", info.metadata.title);
    println!("Author: {}", info.metadata.author);
    println!("Stream URL for web player: {}", info.stream_url);

    let (metadata, mut stream) = stream_audio(url).await?;
    println!("Streaming audio bytes for '{}'...", metadata.title);

    let mut total_bytes = 0;
    while let Some(chunk) = stream.next().await {
        let bytes = chunk?;
        total_bytes += bytes.len();
        print!("\rStreamed {} bytes", total_bytes);
    }
    println!("\nStreaming finished successfully.");

    Ok(())
}
