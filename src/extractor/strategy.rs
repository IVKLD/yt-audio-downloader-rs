use crate::{error::Result, models::ExtractedMedia};

pub trait MediaExtractor: Send + Sync {
    fn extract<'a>(
        &'a self,
        target: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ExtractedMedia>> + Send + 'a>>;
}
