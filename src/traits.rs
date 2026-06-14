/// File format variants supported by this library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, uniffi::Enum)]
pub enum FileFormat {
    #[default]
    Jpeg,
    Png,
    Pdf,
    Docx,
    Xlsx,
    Pptx,
    Mp4,
    Webp,
}

/// Granular control over what metadata categories to remove.
#[derive(Debug, Clone, uniffi::Record)]
pub struct RemovalOptions {
    pub exif: bool,
    pub xmp: bool,
    pub iptc: bool,
    pub icc_profile: bool,
    pub document_properties: bool,
    pub comments: bool,
    pub timestamps: bool,
}

impl Default for RemovalOptions {
    fn default() -> Self {
        Self {
            exif: true,
            xmp: true,
            iptc: true,
            icc_profile: false,
            document_properties: true,
            comments: true,
            timestamps: true,
        }
    }
}

impl RemovalOptions {
    pub fn all() -> Self {
        Self {
            exif: true,
            xmp: true,
            iptc: true,
            icc_profile: true,
            document_properties: true,
            comments: true,
            timestamps: true,
        }
    }
}

/// Core trait for metadata removal from a specific file format.
pub trait MetadataRemover: Send + Sync {
    fn format(&self) -> FileFormat;
    fn remove_metadata(&self, input: &[u8], options: &RemovalOptions) -> crate::Result<Vec<u8>>;
}
