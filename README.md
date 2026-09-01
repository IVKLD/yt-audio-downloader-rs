<p align="center">
  <h2 align="center">yt-audio-downloader-rs</h2>
</p>

<p align="center">
  Asynchronous Rust library for unthrottled YouTube audio extraction, streaming, and format conversion.
  <br>
  <img src="https://img.shields.io/badge/License-MIT-blue.svg?style=flat-square" alt="License">
  <img src="https://img.shields.io/badge/Rust-2024_Edition-orange?style=flat-square&logo=rust" alt="Rust Edition">
  <img src="https://img.shields.io/badge/Async-Tokio_Reqwest-blueviolet?style=flat-square" alt="Async">
</p>

---

## Overview

`yt-audio-downloader` is an asynchronous Rust library designed to extract, stream, and download YouTube audio tracks. By utilizing direct YouTube Innertube client emulation combined with parallel chunked byte-range pipelines, it bypasses Google Video CDN bandwidth throttling, delivering downloads at maximum available network capacity.

## Features

- **Unthrottled Multi-Chunk Downloading**: Splits audio streams into parallel segmented ranges (`buffer_unordered`), bypassing YouTube CDN rate limits.
- **Native Innertube API Emulation**: Directly interfaces with YouTube Innertube clients (`ANDROID_VR`, `ANDROID`) to retrieve Opus and AAC streams without browser automation.
- **Dynamic User-Agent Matching**: Matches the cryptographic client User-Agent with stream URL signatures to prevent `403 Forbidden` responses.
- **Fallback Pipeline**: Falls back to `yt-dlp` if bot verification or cipher changes are encountered.
- **Format Conversion**: Transcodes raw audio to `MP3`, `M4A`, `OPUS`, `FLAC`, and `WAV` via multithreaded FFmpeg (`-threads 0`) with ID3 metadata embedding.
- **Async Byte Streaming**: Generates non-blocking byte streams for web players and backend proxy endpoints.
- **Progress Reporting**: Event-driven progress notifications for initialization, metadata resolution, download progress, and transcoding status.

---

## Installation

Add to `Cargo.toml`:

```toml
[dependencies]
yt-audio-downloader = { path = "libs/yt-audio-downloader-rs" }
tokio = { version = "1.53", features = ["full"] }
```

---

## Usage

### 1. Basic Download

```rust
use yt_audio_downloader::download_audio;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let downloaded = download_audio("https://www.youtube.com/watch?v=dQw4w9WgXcQ", "downloads").await?;

    println!("Saved File: {:?}", downloaded.file_path);
    println!("Title: {}", downloaded.metadata.title);
    println!("Artist: {}", downloaded.metadata.author);
    println!("File Size: {} bytes", downloaded.file_size_bytes);
    Ok(())
}
```

### 2. Custom Options and Progress Handling

```rust
use yt_audio_downloader::{AudioFormat, AudioQuality, ProgressEvent, YoutubeAudioDownloader};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let downloader = YoutubeAudioDownloader::new()
        .output_dir("music")
        .format(AudioFormat::Flac)
        .quality(AudioQuality::Best)
        .embed_metadata(true)
        .on_progress(|event| match event {
            ProgressEvent::Initializing { video_id } => {
                println!("[Init] Video ID: {video_id}");
            }
            ProgressEvent::MetadataFetched { title, author } => {
                println!("[Metadata] '{title}' by '{author}'");
            }
            ProgressEvent::Downloading { bytes_downloaded, percentage, .. } => {
                if let Some(pct) = percentage {
                    print!("\r[Downloading] {pct:.1}% ({bytes_downloaded} bytes)");
                }
            }
            ProgressEvent::Converting { target_format } => {
                println!("\n[Transcoding] Target format: {target_format}");
            }
            ProgressEvent::Finished { output_path, total_bytes } => {
                println!("\n[Done] {:?} ({total_bytes} bytes)", output_path);
            }
            ProgressEvent::Error { message } => {
                eprintln!("\n[Error] {message}");
            }
        });

    let result = downloader.download("https://www.youtube.com/watch?v=dQw4w9WgXcQ").await?;
    println!("File saved: {:?}", result.file_path);
    Ok(())
}
```

### 3. Audio Byte Streaming

```rust
use futures_util::StreamExt;
use yt_audio_downloader::stream_audio;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (metadata, mut stream) = stream_audio("https://www.youtube.com/watch?v=dQw4w9WgXcQ").await?;

    println!("Streaming track: {}", metadata.title);
    while let Some(chunk) = stream.next().await {
        let bytes = chunk?;
        println!("Received chunk: {} bytes", bytes.len());
    }

    Ok(())
}
```

### 4. Search and Playlist Resolution

```rust
use yt_audio_downloader::{fetch_playlist, search_youtube};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tracks = search_youtube("Lofi beats", 10).await?;
    for track in &tracks {
        println!("{}: {} ({}s)", track.id, track.title, track.duration_seconds);
    }

    let playlist_tracks = fetch_playlist("PLxxxxxxxxxxxxxxxxxxxx").await?;
    println!("Playlist track count: {}", playlist_tracks.len());

    Ok(())
}
```

---

## Architecture

```text
libs/yt-audio-downloader-rs/
├── src/
│   ├── lib.rs              # Public exports and convenience functions
│   ├── downloader.rs       # Parallel chunked downloader
│   ├── streamer.rs         # Async audio stream provider
│   ├── converter.rs        # Multithreaded FFmpeg transcoding
│   ├── http.rs             # Connection pooling & User-Agent resolver
│   ├── models.rs           # Core types, formats, metadata schemas
│   ├── progress.rs         # Event-driven progress callback handlers
│   ├── error.rs            # Type-safe errors
│   └── extractor/
│       ├── mod.rs          # Extraction coordinator
│       ├── id.rs           # URL / Video ID / Playlist ID utilities
│       ├── innertube/      # Native Innertube API client & parser
│       │   ├── client.rs   # Raw HTTP endpoints
│       │   ├── parser.rs   # JSON response parser
│       │   └── mod.rs      # InnertubeExtractor implementation
│       ├── strategy.rs     # MediaExtractor trait
│       └── ytdlp.rs        # yt-dlp execution engine
└── examples/
    ├── simple_download.rs  # Minimal download example
    ├── custom_options.rs   # Builder and progress example
    └── stream_audio.rs     # Stream consumption example
```

---

## Testing

```bash
cargo test
cargo run --example simple_download
cargo run --example custom_options
cargo run --example stream_audio
```

---

## License

MIT License.
