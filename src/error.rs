use thiserror::Error;

#[derive(Error, Debug)]
pub enum YoutubeAudioError {
    #[error("Invalid YouTube URL: {0}")]
    InvalidUrl(String),

    #[error("Failed to parse video ID: {0}")]
    VideoIdNotFound(String),

    #[error("HTTP error: {0}")]
    HttpRequest(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    JsonParse(#[from] serde_json::Error),

    #[error("No audio stream found")]
    NoAudioStreamFound,

    #[error("yt-dlp error: {}", clean_ytdlp_stderr(.stderr))]
    YtDlpFailed { status: Option<i32>, stderr: String },

    #[error("yt-dlp is not installed")]
    YtDlpNotFound,

    #[error("FFmpeg error: {0}")]
    FFmpegError(String),

    #[error("FFmpeg is not installed")]
    FFmpegNotFound,

    #[error("Download failed: {0}")]
    DownloadFailed(String),
}

pub type Result<T> = std::result::Result<T, YoutubeAudioError>;

fn clean_ytdlp_stderr(stderr: &str) -> String {
    if stderr.contains("Sign in to confirm you’re not a bot")
        || stderr.contains("Sign in to confirm")
    {
        "YouTube bot verification triggered for this track. Configure proxy or cookies.txt in settings".to_string()
    } else if let Some(err_line) = stderr.lines().find(|l| l.contains("ERROR:")) {
        err_line.trim().to_string()
    } else {
        stderr.trim().to_string()
    }
}
