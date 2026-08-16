# yt-audio-downloader

A high-performance, asynchronous Rust library designed for downloading and streaming audio from YouTube in various formats (`MP3`, `M4A`, `OPUS`, `FLAC`, `WAV`).

## 🚀 Features

- MP3, M4A, OPUS, FLAC, WAV audio formats
- Asynchronous streaming (HTTP byte stream & stream URL for frontend `<audio>` elements)
- Metadata extraction (title, author, duration, view count, thumbnails)
- FFmpeg conversion & ID3 tagging
- Progress event reporting
- Hybrid extraction (YouTube Innertube API + `yt-dlp` fallback)

---

## 📦 Installation

Add to `Cargo.toml`:

```toml
[dependencies]
yt-audio-downloader = { path = "." }
tokio = { version = "1.38", features = ["full"] }
```

---

## 🛠 Quick Start

### 1. Simple Download

```rust
use yt_audio_downloader::download_audio;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let downloaded = download_audio("https://www.youtube.com/watch?v=dQw4w9WgXcQ", "music").await?;

    println!("File saved: {:?}", downloaded.file_path);
    println!("Title: {}", downloaded.metadata.title);
    println!("Author: {}", downloaded.metadata.author);
    Ok(())
}
```

---

### 2. Audio Streaming

```rust
use futures_util::StreamExt;
use yt_audio_downloader::stream_audio;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (metadata, mut stream) = stream_audio("https://www.youtube.com/watch?v=dQw4w9WgXcQ").await?;

    println!("Streaming audio: {}", metadata.title);
    while let Some(chunk) = stream.next().await {
        let bytes = chunk?;
    }

    Ok(())
}
```

---

## 📁 Project Structure

```text
.
├── Cargo.toml
├── shell.nix
├── src/
│   ├── lib.rs
│   ├── downloader.rs
│   ├── streamer.rs
│   ├── converter.rs
│   ├── models.rs
│   ├── progress.rs
│   ├── error.rs
│   └── extractor/
│       ├── mod.rs
│       ├── id.rs
│       ├── innertube.rs
│       └── ytdlp.rs
└── examples/
    ├── simple_download.rs
    ├── custom_options.rs
    └── stream_audio.rs
```

---

## 🧪 Running Examples

```bash
cargo run --example simple_download
cargo run --example custom_options
cargo run --example stream_audio
```
