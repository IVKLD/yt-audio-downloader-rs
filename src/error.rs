use thiserror::Error;

#[derive(Error, Debug)]
pub enum YoutubeAudioError {
    #[error("Invalid YouTube URL: {0}")]
    InvalidUrl(String),

    #[error("Failed to parse video ID: {0}")]
    VideoIdNotFound(String),

    #[error("HTTP request failed: {0}")]
    HttpRequest(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    JsonParse(#[from] serde_json::Error),

    #[error("Task join failed: {0}")]
    Join(#[from] tokio::task::JoinError),

    #[error("URL parsing failed: {0}")]
    UrlParse(#[from] url::ParseError),

    #[error("HTTP {status}: {message}")]
    HttpStatus { status: u16, message: String },

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

    #[error("{0}")]
    Other(String),
}

pub type Error = YoutubeAudioError;
pub type Result<T> = std::result::Result<T, YoutubeAudioError>;

impl YoutubeAudioError {
    pub fn new(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }

    pub fn with_status(status: u16, msg: impl Into<String>) -> Self {
        Self::HttpStatus {
            status,
            message: msg.into(),
        }
    }

    pub fn is_status(&self, code: u16) -> bool {
        match self {
            Self::HttpStatus { status, .. } => *status == code,
            Self::HttpRequest(err) => err.status().map_or(false, |s| s.as_u16() == code),
            _ => false,
        }
    }

    pub fn status_code(&self) -> Option<u16> {
        match self {
            Self::HttpStatus { status, .. } => Some(*status),
            Self::HttpRequest(err) => err.status().map(|s| s.as_u16()),
            _ => None,
        }
    }
}

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
