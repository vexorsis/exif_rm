use crate::error::Error;
use crate::traits::{FileFormat, MetadataRemover, RemovalOptions};

pub struct WebpRemover;

impl MetadataRemover for WebpRemover {
    fn format(&self) -> FileFormat {
        FileFormat::Webp
    }

    fn remove_metadata(&self, input: &[u8], options: &RemovalOptions) -> crate::Result<Vec<u8>> {
        if input.len() < 12 || !input.starts_with(b"RIFF") || &input[8..12] != b"WEBP" {
            return Err(Error::InvalidData("WebP".into()));
        }

        let riff_size = u32::from_le_bytes([input[4], input[5], input[6], input[7]]) as usize;
        if riff_size + 8 != input.len() {
            return Err(Error::InvalidData("WebP RIFF size mismatch".into()));
        }

        let mut output = Vec::with_capacity(input.len());
        output.extend_from_slice(b"RIFF");
        output.extend_from_slice(&[0, 0, 0, 0]); // size placeholder
        output.extend_from_slice(b"WEBP");

        let mut pos = 12;
        let mut has_image = false;

        while pos + 8 <= input.len() {
            let fourcc = &input[pos..pos + 4];
            let chunk_size =
                u32::from_le_bytes([input[pos + 4], input[pos + 5], input[pos + 6], input[pos + 7]])
                    as usize;

            if pos + 8 + chunk_size > input.len() {
                return Err(Error::InvalidData("WebP truncated chunk".into()));
            }

            let payload_end = pos + 8 + chunk_size;
            let padded_end = payload_end + (chunk_size % 2);

            if should_strip_chunk(fourcc, options) {
                pos = padded_end;
                continue;
            }

            if fourcc == b"VP8 " || fourcc == b"VP8L" {
                has_image = true;
            }

            output.extend_from_slice(fourcc);
            output.extend_from_slice(&(chunk_size as u32).to_le_bytes());
            output.extend_from_slice(&input[pos + 8..payload_end]);
            if chunk_size % 2 != 0 && padded_end <= input.len() {
                output.push(input[payload_end]);
            }

            pos = padded_end;
        }

        if !has_image {
            return Err(Error::InvalidData("WebP missing image data chunk".into()));
        }

        let payload_size = (output.len() - 8) as u32;
        output[4..8].copy_from_slice(&payload_size.to_le_bytes());

        Ok(output)
    }
}

fn should_strip_chunk(fourcc: &[u8], options: &RemovalOptions) -> bool {
    match fourcc {
        b"EXIF" => options.exif,
        b"XMP " => options.xmp,
        b"ICCP" => options.icc_profile,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_webp_with_chunks(chunks: &[(&[u8], &[u8])]) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(b"WEBP");
        for &(fourcc, data) in chunks {
            payload.extend_from_slice(fourcc);
            let size = data.len() as u32;
            payload.extend_from_slice(&size.to_le_bytes());
            payload.extend_from_slice(data);
            if data.len() % 2 != 0 {
                payload.push(0);
            }
        }
        let mut webp = Vec::new();
        webp.extend_from_slice(b"RIFF");
        webp.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        webp.extend_from_slice(&payload);
        webp
    }

    fn minimal_vp8_payload() -> &'static [u8] {
        &[0x9D, 0x01, 0x2A, 0x00, 0x01, 0x00, 0x01, 0x00, 0x02, 0x00, 0x02,
          0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00]
    }

    #[test]
    fn test_strip_removes_exif_and_xmp() {
        let input = create_webp_with_chunks(&[
            (b"VP8 ", minimal_vp8_payload()),
            (b"EXIF", b"fake exif data"),
            (b"XMP ", b"fake xmp data"),
            (b"ICCP", b"fake icc data"),
        ]);
        let remover = WebpRemover;
        let output = remover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        assert!(!window_contains(&output, b"EXIF"));
        assert!(!window_contains(&output, b"XMP "));
        assert!(window_contains(&output, b"ICCP"));
        assert!(window_contains(&output, b"VP8 "));
        assert!(output.starts_with(b"RIFF"));
        assert!(&output[8..12] == b"WEBP");
    }

    #[test]
    fn test_strip_icc_when_option_set() {
        let input = create_webp_with_chunks(&[
            (b"VP8 ", minimal_vp8_payload()),
            (b"ICCP", b"fake icc data"),
        ]);
        let options = RemovalOptions { icc_profile: true, ..RemovalOptions::default() };
        let remover = WebpRemover;
        let output = remover.remove_metadata(&input, &options).unwrap();
        assert!(!window_contains(&output, b"ICCP"));
    }

    #[test]
    fn test_no_metadata_output_identical_except_size() {
        let input = create_webp_with_chunks(&[
            (b"VP8 ", minimal_vp8_payload()),
        ]);
        let remover = WebpRemover;
        let output = remover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        assert!(output.starts_with(b"RIFF"));
        assert!(&output[8..12] == b"WEBP");
        assert!(window_contains(&output, b"VP8 "));
    }

    #[test]
    fn test_missing_vp8_returns_error() {
        let input = create_webp_with_chunks(&[
            (b"EXIF", b"fake exif data"),
        ]);
        let remover = WebpRemover;
        let result = remover.remove_metadata(&input, &RemovalOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_riff_size_mismatch_returns_error() {
        let mut webp = Vec::new();
        webp.extend_from_slice(b"RIFF");
        webp.extend_from_slice(&99u32.to_le_bytes());
        webp.extend_from_slice(b"WEBP");
        webp.extend_from_slice(b"VP8 ");
        webp.extend_from_slice(&50u32.to_le_bytes());
        let remover = WebpRemover;
        let result = remover.remove_metadata(&webp, &RemovalOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_truncated_chunk_returns_error() {
        let vp8_payload: &[u8] = &[0x9D, 0x01, 0x2A, 0x00, 0x01, 0x00, 0x01];
        let mut payload = Vec::new();
        payload.extend_from_slice(b"WEBP");
        payload.extend_from_slice(b"VP8 ");
        payload.extend_from_slice(&100u32.to_le_bytes());
        payload.extend_from_slice(vp8_payload);
        let mut webp = Vec::new();
        webp.extend_from_slice(b"RIFF");
        webp.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        webp.extend_from_slice(&payload);
        let remover = WebpRemover;
        let result = remover.remove_metadata(&webp, &RemovalOptions::default());
        assert!(result.is_err());
    }

    fn window_contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }
}
