use crate::error::Error;
use crate::remove::isobmff::{read_box_header, write_box};
use crate::traits::{FileFormat, MetadataRemover, RemovalOptions};
use std::io::Cursor;

pub struct VideoRemover;

impl MetadataRemover for VideoRemover {
    fn format(&self) -> FileFormat {
        FileFormat::Mp4
    }

    fn remove_metadata(&self, input: &[u8], _options: &RemovalOptions) -> crate::Result<Vec<u8>> {
        if input.len() < 8 || &input[4..8] != b"ftyp" {
            return Err(Error::InvalidData("MP4".into()));
        }

        let mut output = Vec::with_capacity(input.len());
        let mut cursor = Cursor::new(input);

        while let Some((total_size, header_size, box_type)) = read_box_header(&mut cursor) {
            let box_start = cursor.position() as usize - header_size;
            let box_end = box_start + total_size;

            if box_end > input.len() {
                break;
            }

            match &box_type {
                b"moov" => {
                    let moov_data = &input[box_start + header_size..box_end];
                    let cleaned_moov = process_moov_box(moov_data)?;
                    if !cleaned_moov.is_empty() {
                        write_box(&mut output, b"moov", &cleaned_moov)?;
                    }
                }
                b"meta" | b"uuid" => {
                    // Skip top-level metadata boxes (some MOV files have meta/uuid outside moov)
                }
                _ => {
                    output.extend_from_slice(&input[box_start..box_end]);
                }
            }

            cursor.set_position(box_end as u64);
        }

        if output.is_empty() {
            return Err(Error::InvalidData("MP4: no boxes processed".into()));
        }

        Ok(output)
    }
}

// --- Box walking helpers ---

/// Process a moov box, removing metadata sub-boxes (udta, meta, uuid)
fn process_moov_box(moov_data: &[u8]) -> crate::Result<Vec<u8>> {
    let mut output = Vec::with_capacity(moov_data.len());
    let mut cursor = Cursor::new(moov_data);

    while let Some((total_size, header_size, box_type)) = read_box_header(&mut cursor) {
        let box_start = cursor.position() as usize - header_size;
        let box_end = box_start + total_size;

        if box_end > moov_data.len() {
            break;
        }

        match &box_type {
            b"udta" | b"meta" | b"uuid" => {
                // Skip metadata boxes entirely
            }
            b"trak" => {
                let trak_data = &moov_data[box_start + header_size..box_end];
                let cleaned_trak = process_trak_box(trak_data)?;
                if !cleaned_trak.is_empty() {
                    write_box(&mut output, b"trak", &cleaned_trak)?;
                }
            }
            _ => {
                output.extend_from_slice(&moov_data[box_start..box_end]);
            }
        }

        cursor.set_position(box_end as u64);
    }

    Ok(output)
}

/// Check if a trak contains a metadata handler
fn is_metadata_track(trak_data: &[u8]) -> bool {
    let mut cursor = Cursor::new(trak_data);

    while let Some((total_size, header_size, box_type)) = read_box_header(&mut cursor) {
        let data_start = cursor.position() as usize;
        let box_end = data_start + total_size - header_size;

        if box_end > trak_data.len() {
            break;
        }

        if &box_type == b"mdia" {
            let mdia_data = &trak_data[data_start..box_end];
            if is_metadata_handler(mdia_data) {
                return true;
            }
        }

        cursor.set_position(box_end as u64);
    }

    false
}

/// Check if mdia contains a metadata handler type
fn is_metadata_handler(mdia_data: &[u8]) -> bool {
    let mut cursor = Cursor::new(mdia_data);

    while let Some((total_size, header_size, box_type)) = read_box_header(&mut cursor) {
        let data_start = cursor.position() as usize;
        let box_end = data_start + total_size - header_size;

        if box_end > mdia_data.len() {
            break;
        }

        if &box_type == b"hdlr" {
            let handler_offset = data_start + 8;
            if let Some(handler_type) = mdia_data.get(handler_offset..handler_offset + 4) {
                if handler_type == b"meta" || handler_type == b"subt" || handler_type == b"gpmd" {
                    return true;
                }
            }
        }

        cursor.set_position(box_end as u64);
    }

    false
}

/// Process a trak box, checking for metadata tracks
fn process_trak_box(trak_data: &[u8]) -> crate::Result<Vec<u8>> {
    if is_metadata_track(trak_data) {
        return Ok(Vec::new());
    }

    let mut output = Vec::with_capacity(trak_data.len());
    let mut cursor = Cursor::new(trak_data);

    while let Some((total_size, header_size, box_type)) = read_box_header(&mut cursor) {
        let box_start = cursor.position() as usize - header_size;
        let box_end = box_start + total_size;

        if box_end > trak_data.len() {
            break;
        }

        match &box_type {
            b"udta" | b"meta" | b"uuid" => {
                // Skip metadata boxes in track
            }
            _ => {
                output.extend_from_slice(&trak_data[box_start..box_end]);
            }
        }

        cursor.set_position(box_end as u64);
    }

    Ok(output)
}