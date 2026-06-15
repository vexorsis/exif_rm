use crate::error::Error;
use crate::traits::{FileFormat, MetadataRemover, RemovalOptions};

pub struct GifRemover;

impl MetadataRemover for GifRemover {
    fn format(&self) -> FileFormat {
        FileFormat::Gif
    }

    fn remove_metadata(&self, input: &[u8], _options: &RemovalOptions) -> crate::Result<Vec<u8>> {
        if !input.starts_with(b"GIF87a") && !input.starts_with(b"GIF89a") {
            return Err(Error::FormatDetectionFailed);
        }
        Ok(input.to_vec())
    }
}
