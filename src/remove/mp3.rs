use crate::error::Error;
use crate::traits::{FileFormat, MetadataRemover, RemovalOptions};

pub struct Mp3Remover;

impl MetadataRemover for Mp3Remover {
    fn format(&self) -> FileFormat {
        FileFormat::Mp3
    }

    fn remove_metadata(&self, input: &[u8], _options: &RemovalOptions) -> crate::Result<Vec<u8>> {
        let mut start = 0usize;
        let mut end = input.len();

        // Strip ID3v2 tags from the front (may be multiple consecutive tags)
        loop {
            if !input[start..].starts_with(b"ID3") {
                break;
            }
            if input.len() - start < 10 {
                return Err(Error::InvalidData("Truncated ID3v2 header".into()));
            }
            let tag_size = decode_syncsafe(&input[start + 6..start + 10]);
            let total_tag_size = 10 + tag_size;
            // Footer flag (bit 4) only exists in ID3v2.4+
            let major_version = input[start + 3];
            let has_footer = major_version >= 4 && (input[start + 5] & 0x10) != 0;
            let skip = total_tag_size + if has_footer { 10 } else { 0 };
            if start + skip > input.len() {
                return Err(Error::InvalidData("ID3v2 tag size exceeds file length".into()));
            }
            start += skip;
        }

        // Strip ID3v1 tag from the end
        if end - start >= 128 && &input[end - 128..end - 125] == b"TAG" {
            end -= 128;
        }

        // Trim null padding between ID3v2 tag and first audio frame
        while start < end && input[start] == 0x00 {
            start += 1;
        }

        if start >= end {
            return Err(Error::InvalidData("MP3 file contains no audio data".into()));
        }

        Ok(input[start..end].to_vec())
    }
}

/// Decode a 4-byte syncsafe integer (each byte uses only 7 bits).
fn decode_syncsafe(bytes: &[u8]) -> usize {
    ((bytes[0] as usize & 0x7F) << 21)
        | ((bytes[1] as usize & 0x7F) << 14)
        | ((bytes[2] as usize & 0x7F) << 7)
        | (bytes[3] as usize & 0x7F)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_syncsafe() {
        assert_eq!(decode_syncsafe(&[0x00, 0x00, 0x00, 0x00]), 0);
        assert_eq!(decode_syncsafe(&[0x00, 0x00, 0x00, 0x01]), 1);
        assert_eq!(decode_syncsafe(&[0x00, 0x00, 0x01, 0x00]), 128);
        assert_eq!(decode_syncsafe(&[0x7F, 0x7F, 0x7F, 0x7F]), 268435455);
    }

    #[test]
    fn test_strip_id3v2_only() {
        let body = b"TIT2\x00\x00\x00\x04\x00\x00\x00Test";
        let mut input = Vec::new();
        input.extend_from_slice(b"ID3");
        input.extend_from_slice(&[0x03, 0x00, 0x00]); // version + flags
        // syncsafe size = 15 bytes
        input.extend_from_slice(&[0x00, 0x00, 0x00, 0x0F]);
        input.extend_from_slice(body);
        input.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x00]); // fake frame
        let output = Mp3Remover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        assert!(!output.starts_with(b"ID3"));
        assert!(output.starts_with(&[0xFF, 0xFB]));
    }

    #[test]
    fn test_strip_id3v1_only() {
        let mut input = vec![0xFF, 0xFB, 0x90, 0x00]; // fake frame
        input.extend_from_slice(b"TAG");
        input.extend_from_slice(&[0u8; 125]);
        let output = Mp3Remover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        assert!(!output.windows(3).any(|w| w == b"TAG"));
    }

    #[test]
    fn test_truncated_id3v2_header() {
        let input = b"ID3\x03\x00".as_slice(); // only 5 bytes
        let result = Mp3Remover.remove_metadata(input, &RemovalOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_no_audio_data() {
        let mut input = Vec::new();
        input.extend_from_slice(b"ID3");
        input.extend_from_slice(&[0x03, 0x00, 0x00]);
        input.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // size = 0
        let result = Mp3Remover.remove_metadata(&input, &RemovalOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_strip_id3v2_with_footer() {
        let body = b"TIT2\x00\x00\x00\x04\x00\x00\x00Test";
        let body_size = body.len() as u32;
        // ID3v2.4 header with footer flag set
        let mut header = Vec::new();
        header.extend_from_slice(b"ID3");
        header.extend_from_slice(&[0x04, 0x00, 0x10]); // v2.4, footer flag set
        header.push(((body_size >> 21) & 0x7F) as u8);
        header.push(((body_size >> 14) & 0x7F) as u8);
        header.push(((body_size >> 7) & 0x7F) as u8);
        header.push((body_size & 0x7F) as u8);
        let mut input = Vec::new();
        input.extend_from_slice(&header);
        input.extend_from_slice(body);
        // ID3v2 footer: "3DI" + version + flags + same size
        input.extend_from_slice(b"3DI");
        input.extend_from_slice(&[0x04, 0x00, 0x10]);
        input.extend_from_slice(&header[6..10]);
        input.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x00]); // fake frame
        let output = Mp3Remover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        assert!(!output.starts_with(b"ID3"));
        assert!(!output.windows(3).any(|w| w == b"3DI"));
        assert!(output.starts_with(&[0xFF, 0xFB]));
    }

    #[test]
    fn test_strip_null_padding() {
        let mut input = Vec::new();
        input.extend_from_slice(b"ID3");
        input.extend_from_slice(&[0x03, 0x00, 0x00]);
        input.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // size = 0
        // Null padding between tag and audio
        input.extend_from_slice(&[0x00, 0x00, 0x00]);
        input.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x00]); // fake frame
        let output = Mp3Remover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        assert!(output.starts_with(&[0xFF, 0xFB]));
    }

    #[test]
    fn test_v3_experimental_flag_not_treated_as_footer() {
        let body = b"TIT2\x00\x00\x00\x04\x00\x00\x00Test";
        let body_size = body.len() as u32;
        // ID3v2.3 header with bit 4 set (experimental flag, NOT footer)
        let mut header = Vec::new();
        header.extend_from_slice(b"ID3");
        header.extend_from_slice(&[0x03, 0x00, 0x10]); // v2.3, experimental flag set
        header.push(((body_size >> 21) & 0x7F) as u8);
        header.push(((body_size >> 14) & 0x7F) as u8);
        header.push(((body_size >> 7) & 0x7F) as u8);
        header.push((body_size & 0x7F) as u8);
        let mut input = Vec::new();
        input.extend_from_slice(&header);
        input.extend_from_slice(body);
        input.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x00]); // fake frame
        let output = Mp3Remover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        assert!(output.starts_with(&[0xFF, 0xFB]),
            "audio should start immediately after tag body, no extra 10-byte skip");
    }

    #[test]
    fn test_multiple_id3v2_tags() {
        let body1 = b"TIT2\x00\x00\x00\x04\x00\x00\x00Test";
        let body2 = b"TPE1\x00\x00\x00\x05\x00\x00\x00Hello";
        let mut input = Vec::new();
        // First ID3v2 tag
        input.extend_from_slice(b"ID3");
        input.extend_from_slice(&[0x03, 0x00, 0x00]);
        let sz1 = body1.len() as u32;
        input.push(((sz1 >> 21) & 0x7F) as u8);
        input.push(((sz1 >> 14) & 0x7F) as u8);
        input.push(((sz1 >> 7) & 0x7F) as u8);
        input.push((sz1 & 0x7F) as u8);
        input.extend_from_slice(body1);
        // Second ID3v2 tag
        input.extend_from_slice(b"ID3");
        input.extend_from_slice(&[0x03, 0x00, 0x00]);
        let sz2 = body2.len() as u32;
        input.push(((sz2 >> 21) & 0x7F) as u8);
        input.push(((sz2 >> 14) & 0x7F) as u8);
        input.push(((sz2 >> 7) & 0x7F) as u8);
        input.push((sz2 & 0x7F) as u8);
        input.extend_from_slice(body2);
        // Audio data
        input.extend_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
        let output = Mp3Remover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        assert!(!output.windows(3).any(|w| w == b"ID3"));
        assert!(output.starts_with(&[0xFF, 0xFB]));
    }
}
