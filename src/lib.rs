pub mod error;
pub mod format;
pub mod traits;

#[cfg(any(feature = "jpeg", feature = "png", feature = "pdf", feature = "office", feature = "video", feature = "webp", feature = "mp3", feature = "gif", feature = "heic"))]
pub mod remove;

pub use error::{Error, Result};
pub use format::detect_format;
pub use traits::{FileFormat, MetadataRemover, RemovalOptions};

/// Remove all metadata from a file's bytes using default-safe options (keeps ICC profiles).
pub fn strip_metadata(input: &[u8]) -> Result<Vec<u8>> {
    strip_metadata_with(input, &RemovalOptions::default())
}

/// Remove metadata with custom options.
pub fn strip_metadata_with(input: &[u8], options: &RemovalOptions) -> Result<Vec<u8>> {
    let format = detect_format(input)?;
    let remover = get_remover(format);
    remover.remove_metadata(input, options)
}

fn get_remover(format: FileFormat) -> Box<dyn MetadataRemover> {
    match format {
        #[cfg(feature = "jpeg")]
        FileFormat::Jpeg => Box::new(remove::jpeg::JpegRemover),
        #[cfg(feature = "png")]
        FileFormat::Png => Box::new(remove::png::PngRemover),
        #[cfg(feature = "pdf")]
        FileFormat::Pdf => Box::new(remove::pdf::PdfRemover),
        FileFormat::Docx | FileFormat::Xlsx | FileFormat::Pptx => {
            #[cfg(feature = "office")]
            {
                Box::new(remove::office::OfficeRemover)
            }
            #[cfg(not(feature = "office"))]
            {
                panic!("office feature not enabled")
            }
        }
        #[cfg(feature = "video")]
        FileFormat::Mp4 => Box::new(remove::video::VideoRemover),
        #[cfg(feature = "webp")]
        FileFormat::Webp => Box::new(remove::webp::WebpRemover),
        #[cfg(feature = "mp3")]
        FileFormat::Mp3 => Box::new(remove::mp3::Mp3Remover),
        #[cfg(feature = "gif")]
        FileFormat::Gif => Box::new(remove::gif::GifRemover),
        #[allow(unreachable_patterns)]
        _ => panic!("format not supported in this build configuration"),
    }
}

// --- UniFFI exports ---

#[uniffi::export]
pub fn strip_metadata_owned(input: Vec<u8>) -> Result<Vec<u8>> {
    strip_metadata(&input)
}

#[uniffi::export]
pub fn strip_metadata_with_owned(input: Vec<u8>, options: RemovalOptions) -> Result<Vec<u8>> {
    strip_metadata_with(&input, &options)
}

#[uniffi::export]
pub fn detect_format_owned(bytes: Vec<u8>) -> Result<FileFormat> {
    detect_format(&bytes)
}

#[uniffi::export]
pub fn removal_options_default() -> RemovalOptions {
    RemovalOptions::default()
}

#[uniffi::export]
pub fn removal_options_all() -> RemovalOptions {
    RemovalOptions::all()
}

uniffi::setup_scaffolding!();