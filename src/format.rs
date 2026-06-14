use crate::error::Error;
use crate::traits::FileFormat;

pub fn detect_format(bytes: &[u8]) -> crate::Result<FileFormat> {
    if bytes.len() < 8 {
        return Err(Error::FormatDetectionFailed);
    }

    // JPEG: FF D8 FF
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Ok(FileFormat::Jpeg);
    }

    // PNG: 89 50 4E 47 0D 0A 1A 0A
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Ok(FileFormat::Png);
    }

    // PDF: %PDF
    if bytes.starts_with(b"%PDF") {
        return Ok(FileFormat::Pdf);
    }

    // MP4/MOV: box size (4 bytes) + "ftyp" (4 bytes)
    // Both .mp4 and .mov use the same ISOBMFF container format
    if bytes.len() >= 8 && &bytes[4..8] == b"ftyp" {
        #[cfg(feature = "video")]
        return Ok(FileFormat::Mp4);
        #[cfg(not(feature = "video"))]
        return Err(Error::UnsupportedFormat);
    }

    // WebP: RIFF....WEBP
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        #[cfg(feature = "webp")]
        return Ok(FileFormat::Webp);
        #[cfg(not(feature = "webp"))]
        return Err(Error::UnsupportedFormat);
    }

    // Office Open XML (ZIP-based): PK 03 04
    if bytes.starts_with(&[0x50, 0x4B, 0x03, 0x04]) {
        #[cfg(feature = "office")]
        return detect_office_format(bytes);
        #[cfg(not(feature = "office"))]
        return Err(Error::UnsupportedFormat);
    }

    Err(Error::UnsupportedFormat)
}

#[cfg(feature = "office")]
fn detect_office_format(bytes: &[u8]) -> crate::Result<FileFormat> {
    use std::io::Cursor;
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| Error::InvalidData("ZIP".into()))?;

    for i in 0..archive.len() {
        let file = archive.by_index(i).map_err(|_| Error::InvalidData("ZIP entry".into()))?;
        let name = file.name();
        if name.starts_with("word/") {
            return Ok(FileFormat::Docx);
        }
        if name.starts_with("xl/") {
            return Ok(FileFormat::Xlsx);
        }
        if name.starts_with("ppt/") {
            return Ok(FileFormat::Pptx);
        }
    }

    Err(Error::InvalidData("Cannot determine Office format".into()))
}
