use std::sync::Arc;

/// Callback for reporting download progress: `(downloaded_bytes, total_bytes)`.
pub type ProgressCallback = Arc<dyn Fn(u64, Option<u64>) + Send + Sync>;
