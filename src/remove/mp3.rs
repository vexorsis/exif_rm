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

        // Strip ID3v2 tag from the front
        if input.starts_with(b"ID3") {
            if input.len() < 10 {
                return Err(Error::InvalidData("Truncated ID3v2 header".into()));
            }
            let tag_size = decode_syncsafe(&input[6..10]);
            let total_tag_size = 10 + tag_size;
            // Check for footer (flag bit 4)
            let has_footer = (input[5] & 0x10) != 0;
            let skip = total_tag_size + if has_footer { 10 } else { 0 };
            if skip > input.len() {
                return Err(Error::InvalidData("ID3v2 tag size exceeds file length".into()));
            }
            start = skip;
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
}
