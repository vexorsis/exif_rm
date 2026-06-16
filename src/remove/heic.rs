use crate::error::Error;
use crate::remove::isobmff::read_box_header;
use crate::traits::{FileFormat, MetadataRemover, RemovalOptions};
use std::io::Cursor;

pub struct HeicRemover;

impl MetadataRemover for HeicRemover {
    fn format(&self) -> FileFormat {
        FileFormat::Heic
    }

    fn remove_metadata(&self, input: &[u8], _options: &RemovalOptions) -> crate::Result<Vec<u8>> {
        if input.len() < 12 || &input[4..8] != b"ftyp" {
            return Err(Error::InvalidData("HEIC".into()));
        }

        let ftyp_size = u32::from_be_bytes(input[0..4].try_into().unwrap()) as usize;
        let major_brand = &input[8..12];
        if major_brand != b"heic"
            && input.get(16..ftyp_size).map_or(true, |brands| !brands.chunks_exact(4).any(|b| b == b"heic"))
        {
            return Err(Error::InvalidData("HEIC".into()));
        }

        let mut output = Vec::with_capacity(input.len());
        let mut cursor = Cursor::new(input);
        let mut found_meta = false;

        while let Some((total_size, header_size, box_type)) = read_box_header(&mut cursor) {
            let box_start = cursor.position() as usize - header_size;
            let box_end = box_start + total_size;

            if box_end > input.len() {
                break;
            }

            match &box_type {
                b"meta" => {
                    found_meta = true;
                    output.extend_from_slice(&input[box_start..box_end]);
                }
                _ => {
                    output.extend_from_slice(&input[box_start..box_end]);
                }
            }

            cursor.set_position(box_end as u64);
        }

        if output.is_empty() {
            return Err(Error::InvalidData("HEIC: no boxes processed".into()));
        }

        if !found_meta {
            return Err(Error::InvalidData("HEIC: no meta box found".into()));
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_box(box_type: &[u8], content: &[u8]) -> Vec<u8> {
        let size = (8 + content.len()) as u32;
        let mut buf = size.to_be_bytes().to_vec();
        buf.extend_from_slice(box_type);
        buf.extend_from_slice(content);
        buf
    }

    fn make_fullbox(box_type: &[u8], version: u8, flags: u32, content: &[u8]) -> Vec<u8> {
        let vf = ((version as u32) << 24) | flags;
        let mut full_content = vf.to_be_bytes().to_vec();
        full_content.extend_from_slice(content);
        make_box(box_type, &full_content)
    }

    fn create_minimal_heic() -> Vec<u8> {
        let mut heic = Vec::new();
        let mut ftyp_content = b"heic".to_vec();
        ftyp_content.extend_from_slice(&0u32.to_be_bytes());
        heic.extend_from_slice(&make_box(b"ftyp", &ftyp_content));

        let mut hdlr_content = Vec::new();
        hdlr_content.extend_from_slice(&0u32.to_be_bytes());
        hdlr_content.extend_from_slice(b"pict");
        hdlr_content.extend_from_slice(&[0u8; 12]);
        hdlr_content.push(0);
        let hdlr = make_fullbox(b"hdlr", 0, 0, &hdlr_content);
        let meta = make_fullbox(b"meta", 0, 0, &hdlr);
        heic.extend_from_slice(&meta);

        heic.extend_from_slice(&make_box(b"mdat", b"fake image data"));
        heic
    }

    #[test]
    fn test_heic_passthrough_no_metadata() {
        let input = create_minimal_heic();
        let output = HeicRemover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        assert_eq!(input, output, "HEIC with no metadata should pass through unchanged");
    }
}
