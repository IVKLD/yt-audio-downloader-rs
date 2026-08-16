use std::{path::PathBuf, sync::Arc};

#[derive(Debug, Clone)]
pub enum ProgressEvent {
    Initializing {
        video_id: String,
    },
    MetadataFetched {
        title: String,
        author: String,
    },
    Downloading {
        bytes_downloaded: u64,
        total_bytes: Option<u64>,
        percentage: Option<f32>,
    },
    Converting {
        target_format: String,
    },
    Finished {
        output_path: PathBuf,
        total_bytes: u64,
    },
    Error {
        message: String,
    },
}

pub type ProgressHandler = Arc<dyn Fn(ProgressEvent) + Send + Sync + 'static>;
