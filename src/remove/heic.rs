use crate::error::Error;
use crate::remove::isobmff::read_box_header;
use crate::traits::{FileFormat, MetadataRemover, RemovalOptions};
use std::io::Cursor;

pub struct HeicRemover;

impl MetadataRemover for HeicRemover {
    fn format(&self) -> FileFormat {
        FileFormat::Heic
    }

    fn remove_metadata(&self, input: &[u8], options: &RemovalOptions) -> crate::Result<Vec<u8>> {
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
                    // meta box: 8-byte box header + content
                    // content starts with version+flags (4 bytes for fullbox), then inner boxes
                    let meta_header = &input[box_start..box_start + header_size];
                    let meta_content = &input[box_start + header_size..box_end];

                    let processed = process_meta_box(meta_content, options)?;
                    // Rebuild meta box: original header + processed content
                    output.extend_from_slice(meta_header);
                    output.extend_from_slice(&processed);
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

/// Process the content inside a meta box (after the box header).
/// The content starts with version+flags (4 bytes for a fullbox), then inner boxes.
fn process_meta_box(meta_data: &[u8], options: &RemovalOptions) -> crate::Result<Vec<u8>> {
    // meta is a fullbox: first 4 bytes are version(1) + flags(3)
    if meta_data.len() < 4 {
        return Err(Error::InvalidData("HEIC: meta box too short".into()));
    }

    let version_flags = &meta_data[0..4];

    // First pass: find metadata item IDs from iinf
    let mut removed_ids: Vec<u16> = Vec::new();
    let mut cursor = Cursor::new(&meta_data[4..]);
    while let Some((total_size, header_size, box_type)) = read_box_header(&mut cursor) {
        let inner_start = cursor.position() as usize - header_size;
        let inner_end = inner_start + total_size;

        if inner_end > meta_data.len() - 4 {
            break;
        }

        if box_type == *b"iinf" {
            // iinf content starts after the box header (which includes version+flags as a fullbox)
            let iinf_content = &meta_data[4 + inner_start + header_size..4 + inner_end];
            removed_ids = find_metadata_item_ids(iinf_content, options);
        }

        cursor.set_position(inner_end as u64);
    }

    // If nothing to remove, return as-is
    if removed_ids.is_empty() {
        return Ok(meta_data.to_vec());
    }

    // Second pass: rebuild meta box contents, filtering iinf and iloc
    let mut result = version_flags.to_vec();
    cursor.set_position(0);
    while let Some((total_size, header_size, box_type)) = read_box_header(&mut cursor) {
        let inner_start = cursor.position() as usize - header_size;
        let inner_end = inner_start + total_size;

        if inner_end > meta_data.len() - 4 {
            break;
        }

        let box_header_bytes = &meta_data[4 + inner_start..4 + inner_start + header_size];
        let box_content = &meta_data[4 + inner_start + header_size..4 + inner_end];

        match &box_type {
            b"iinf" => {
                let rebuilt = rebuild_iinf(box_content, &removed_ids)?;
                result.extend_from_slice(box_header_bytes);
                result.extend_from_slice(&rebuilt);
            }
            b"iloc" => {
                let rebuilt = rebuild_iloc(box_content, &removed_ids)?;
                result.extend_from_slice(box_header_bytes);
                result.extend_from_slice(&rebuilt);
            }
            _ => {
                result.extend_from_slice(&meta_data[4 + inner_start..4 + inner_end]);
            }
        }

        cursor.set_position(inner_end as u64);
    }

    Ok(result)
}

/// Find metadata item IDs from iinf box content (after the box header, i.e., fullbox content).
/// The content starts with version(1) + flags(3) + entry_count.
fn find_metadata_item_ids(iinf_data: &[u8], options: &RemovalOptions) -> Vec<u16> {
    let mut ids = Vec::new();

    if iinf_data.len() < 6 {
        return ids;
    }

    let version = iinf_data[0];
    let entry_count = if version == 0 {
        u16::from_be_bytes([iinf_data[4], iinf_data[5]]) as usize
    } else {
        if iinf_data.len() < 8 {
            return ids;
        }
        u32::from_be_bytes([iinf_data[4], iinf_data[5], iinf_data[6], iinf_data[7]]) as usize
    };

    let mut offset = if version == 0 { 6 } else { 8 };

    for _ in 0..entry_count {
        if offset + 8 > iinf_data.len() {
            break;
        }

        // Each infe is itself a fullbox
        let infe_size = u32::from_be_bytes(
            iinf_data[offset..offset + 4].try_into().unwrap_or([0u8; 4]),
        ) as usize;
        if infe_size < 8 || offset + infe_size > iinf_data.len() {
            break;
        }

        let infe_data = &iinf_data[offset..offset + infe_size];
        let infe_version = infe_data[8]; // version byte of the infe fullbox

        let (item_id, item_type_offset) = if infe_version <= 1 {
            // v0/v1: item_id at offset 12 (2 bytes), item_type at offset 16 (4 bytes)
            if infe_data.len() < 20 {
                offset += infe_size;
                continue;
            }
            let id = u16::from_be_bytes([infe_data[12], infe_data[13]]);
            (id, 16)
        } else {
            // v2+: item_id at offset 12 (4 bytes), item_type at offset 16 (4 bytes)
            if infe_data.len() < 20 {
                offset += infe_size;
                continue;
            }
            let id = u32::from_be_bytes([infe_data[12], infe_data[13], infe_data[14], infe_data[15]]) as u16;
            (id, 16)
        };

        let item_type = &infe_data[item_type_offset..item_type_offset + 4];

        let should_remove = (options.exif && item_type == b"Exif")
            || (options.xmp && item_type == b"mime");

        if should_remove {
            ids.push(item_id);
        }

        offset += infe_size;
    }

    ids
}

/// Rebuild iinf box content (after the box header), excluding removed items.
/// The content starts with version(1) + flags(3) + entry_count.
fn rebuild_iinf(iinf_data: &[u8], removed_ids: &[u16]) -> crate::Result<Vec<u8>> {
    if iinf_data.len() < 6 {
        return Err(Error::InvalidData("HEIC: iinf box too short".into()));
    }

    let version = iinf_data[0];
    let flags = &iinf_data[1..4];

    let mut result = vec![version];
    result.extend_from_slice(flags);

    let mut kept_count: u32 = 0;
    let mut offset = if version == 0 { 6 } else { 8 };

    // Skip the original entry_count for now; we'll write it later
    // First, collect the kept infe entries
    let mut kept_entries: Vec<Vec<u8>> = Vec::new();

    let entry_count = if version == 0 {
        u16::from_be_bytes([iinf_data[4], iinf_data[5]]) as usize
    } else {
        u32::from_be_bytes([iinf_data[4], iinf_data[5], iinf_data[6], iinf_data[7]]) as usize
    };

    for _ in 0..entry_count {
        if offset + 8 > iinf_data.len() {
            break;
        }

        let infe_size = u32::from_be_bytes(
            iinf_data[offset..offset + 4].try_into().unwrap_or([0u8; 4]),
        ) as usize;
        if infe_size < 8 || offset + infe_size > iinf_data.len() {
            break;
        }

        let infe_data = &iinf_data[offset..offset + infe_size];

        if !is_infe_removed(infe_data, removed_ids) {
            kept_entries.push(infe_data.to_vec());
            kept_count += 1;
        }

        offset += infe_size;
    }

    // Write entry_count
    if version == 0 {
        result.extend_from_slice(&(kept_count as u16).to_be_bytes());
    } else {
        result.extend_from_slice(&kept_count.to_be_bytes());
    }

    // Write kept entries
    for entry in &kept_entries {
        result.extend_from_slice(entry);
    }

    Ok(result)
}

/// Check if an infe entry's item_id is in the removed_ids list.
fn is_infe_removed(infe_data: &[u8], removed_ids: &[u16]) -> bool {
    if infe_data.len() < 12 {
        return false;
    }

    let infe_version = infe_data[8]; // version byte of the infe fullbox

    let item_id = if infe_version <= 1 {
        if infe_data.len() < 14 {
            return false;
        }
        u16::from_be_bytes([infe_data[12], infe_data[13]])
    } else {
        if infe_data.len() < 16 {
            return false;
        }
        u32::from_be_bytes([infe_data[12], infe_data[13], infe_data[14], infe_data[15]]) as u16
    };

    removed_ids.contains(&item_id)
}

/// Rebuild iloc box content (after the box header), excluding removed items.
/// The content starts with version(1) + flags(3) + size fields + item_count.
fn rebuild_iloc(iloc_data: &[u8], removed_ids: &[u16]) -> crate::Result<Vec<u8>> {
    if iloc_data.len() < 8 {
        return Err(Error::InvalidData("HEIC: iloc box too short".into()));
    }

    let version = iloc_data[0];
    if version > 1 {
        return Err(Error::InvalidData("HEIC: iloc version > 1 not supported".into()));
    }

    let flags = &iloc_data[1..4];

    // offset_size(4 bits) + length_size(4 bits) at byte 4
    // base_offset_size(4 bits) + [index_size(4 bits) for v1] at byte 5
    let offset_size = (iloc_data[4] >> 4) as usize;
    let length_size = (iloc_data[4] & 0x0F) as usize;
    let base_offset_size = (iloc_data[5] >> 4) as usize;
    let index_size = if version == 1 { (iloc_data[5] & 0x0F) as usize } else { 0 };

    let item_count_size = if version < 2 { 2 } else { 4 };
    let item_count_offset = if version == 1 { 6 } else { 6 };

    if iloc_data.len() < item_count_offset + item_count_size {
        return Err(Error::InvalidData("HEIC: iloc box too short for item count".into()));
    }

    let item_count = if version < 2 {
        u16::from_be_bytes([iloc_data[item_count_offset], iloc_data[item_count_offset + 1]]) as usize
    } else {
        u32::from_be_bytes([
            iloc_data[item_count_offset],
            iloc_data[item_count_offset + 1],
            iloc_data[item_count_offset + 2],
            iloc_data[item_count_offset + 3],
        ]) as usize
    };

    // Build result: version + flags + size fields
    let mut result = vec![version];
    result.extend_from_slice(flags);
    result.push(iloc_data[4]); // offset_size + length_size
    result.push(iloc_data[5]); // base_offset_size + index_size

    // We'll write item_count later, after counting kept items
    let item_count_pos = result.len();
    if version < 2 {
        result.extend_from_slice(&0u16.to_be_bytes()); // placeholder
    } else {
        result.extend_from_slice(&0u32.to_be_bytes()); // placeholder
    }

    let mut offset = item_count_offset + item_count_size;
    let mut kept_count: u32 = 0;

    for _ in 0..item_count {
        if offset + 2 > iloc_data.len() {
            break;
        }

        let item_id = u16::from_be_bytes([iloc_data[offset], iloc_data[offset + 1]]);
        let is_removed = removed_ids.contains(&item_id);

        // Calculate the size of this item entry to skip or copy
        let entry_start = offset;
        offset += 2; // item_id

        if version == 1 {
            offset += 2; // construction_method
        }

        if offset + 2 > iloc_data.len() {
            break;
        }
        offset += 2; // data_reference_index

        offset += base_offset_size; // base_offset

        if offset + 2 > iloc_data.len() {
            break;
        }
        let extent_count = u16::from_be_bytes([iloc_data[offset], iloc_data[offset + 1]]) as usize;
        offset += 2; // extent_count

        if version == 1 && index_size > 0 {
            offset += index_size * extent_count; // extent_index
        }

        offset += (offset_size + length_size) * extent_count; // extents

        if !is_removed {
            // Copy this item entry
            result.extend_from_slice(&iloc_data[entry_start..offset]);
            kept_count += 1;
        }
    }

    // Update item_count in result
    if version < 2 {
        let count_bytes = (kept_count as u16).to_be_bytes();
        result[item_count_pos] = count_bytes[0];
        result[item_count_pos + 1] = count_bytes[1];
    } else {
        let count_bytes = kept_count.to_be_bytes();
        result[item_count_pos] = count_bytes[0];
        result[item_count_pos + 1] = count_bytes[1];
        result[item_count_pos + 2] = count_bytes[2];
        result[item_count_pos + 3] = count_bytes[3];
    }

    Ok(result)
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

    /// Create a minimal HEIC with an EXIF item (id=1) and an hvc1 item (id=2)
    fn create_heic_with_exif() -> Vec<u8> {
        let mut heic = Vec::new();

        // ftyp box
        let mut ftyp_content = b"heic".to_vec();
        ftyp_content.extend_from_slice(&0u32.to_be_bytes());
        heic.extend_from_slice(&make_box(b"ftyp", &ftyp_content));

        // Build meta box contents
        let mut meta_inner = Vec::new();

        // hdlr box
        let mut hdlr_content = Vec::new();
        hdlr_content.extend_from_slice(&0u32.to_be_bytes()); // pre_defined
        hdlr_content.extend_from_slice(b"pict");
        hdlr_content.extend_from_slice(&[0u8; 12]); // reserved
        hdlr_content.push(0); // name null terminator
        meta_inner.extend_from_slice(&make_fullbox(b"hdlr", 0, 0, &hdlr_content));

        // iinf box with 2 items: Exif (id=1) and hvc1 (id=2)
        let mut iinf_entries = Vec::new();

        // infe for Exif item (id=1)
        let mut infe1_content = Vec::new();
        infe1_content.extend_from_slice(&1u16.to_be_bytes()); // item_id
        infe1_content.extend_from_slice(&0u16.to_be_bytes()); // item_protection_index
        infe1_content.extend_from_slice(b"Exif"); // item_type
        infe1_content.push(0); // item_name null terminator
        iinf_entries.extend_from_slice(&make_fullbox(b"infe", 0, 0, &infe1_content));

        // infe for hvc1 item (id=2)
        let mut infe2_content = Vec::new();
        infe2_content.extend_from_slice(&2u16.to_be_bytes()); // item_id
        infe2_content.extend_from_slice(&0u16.to_be_bytes()); // item_protection_index
        infe2_content.extend_from_slice(b"hvc1"); // item_type
        infe2_content.push(0); // item_name null terminator
        iinf_entries.extend_from_slice(&make_fullbox(b"infe", 0, 0, &infe2_content));

        // iinf fullbox: version=0, flags=0, entry_count=2
        let mut iinf_content = (2u16).to_be_bytes().to_vec();
        iinf_content.extend_from_slice(&iinf_entries);
        meta_inner.extend_from_slice(&make_fullbox(b"iinf", 0, 0, &iinf_content));

        // iloc box with 2 items
        // version=0, offset_size=0, length_size=0, base_offset_size=0
        let mut iloc_content = Vec::new();
        iloc_content.push(0x00); // offset_size=0, length_size=0
        iloc_content.push(0x00); // base_offset_size=0
        iloc_content.extend_from_slice(&2u16.to_be_bytes()); // item_count=2

        // item 1 (Exif): item_id=1, data_reference_index=0, extent_count=0
        iloc_content.extend_from_slice(&1u16.to_be_bytes()); // item_id
        iloc_content.extend_from_slice(&0u16.to_be_bytes()); // data_reference_index
        iloc_content.extend_from_slice(&0u16.to_be_bytes()); // extent_count

        // item 2 (hvc1): item_id=2, data_reference_index=0, extent_count=1
        iloc_content.extend_from_slice(&2u16.to_be_bytes()); // item_id
        iloc_content.extend_from_slice(&0u16.to_be_bytes()); // data_reference_index
        iloc_content.extend_from_slice(&1u16.to_be_bytes()); // extent_count
        // extent: offset(0) + length(4) - but offset_size=0, length_size=0, so no extent data
        // Actually with offset_size=0 and length_size=0, extents have 0 bytes per extent
        // That's fine for testing

        meta_inner.extend_from_slice(&make_fullbox(b"iloc", 0, 0, &iloc_content));

        // meta fullbox
        heic.extend_from_slice(&make_fullbox(b"meta", 0, 0, &meta_inner));

        // mdat box
        heic.extend_from_slice(&make_box(b"mdat", b"fake image data"));

        heic
    }

    /// Create a minimal HEIC with an XMP/mime item (id=1) and an hvc1 item (id=2)
    fn create_heic_with_xmp() -> Vec<u8> {
        let mut heic = Vec::new();

        // ftyp box
        let mut ftyp_content = b"heic".to_vec();
        ftyp_content.extend_from_slice(&0u32.to_be_bytes());
        heic.extend_from_slice(&make_box(b"ftyp", &ftyp_content));

        // Build meta box contents
        let mut meta_inner = Vec::new();

        // hdlr box
        let mut hdlr_content = Vec::new();
        hdlr_content.extend_from_slice(&0u32.to_be_bytes());
        hdlr_content.extend_from_slice(b"pict");
        hdlr_content.extend_from_slice(&[0u8; 12]);
        hdlr_content.push(0);
        meta_inner.extend_from_slice(&make_fullbox(b"hdlr", 0, 0, &hdlr_content));

        // iinf box with 2 items: mime/XMP (id=1) and hvc1 (id=2)
        let mut iinf_entries = Vec::new();

        // infe for XMP item (id=1, type="mime")
        let mut infe1_content = Vec::new();
        infe1_content.extend_from_slice(&1u16.to_be_bytes()); // item_id
        infe1_content.extend_from_slice(&0u16.to_be_bytes()); // item_protection_index
        infe1_content.extend_from_slice(b"mime"); // item_type
        infe1_content.push(0); // item_name null terminator
        iinf_entries.extend_from_slice(&make_fullbox(b"infe", 0, 0, &infe1_content));

        // infe for hvc1 item (id=2)
        let mut infe2_content = Vec::new();
        infe2_content.extend_from_slice(&2u16.to_be_bytes()); // item_id
        infe2_content.extend_from_slice(&0u16.to_be_bytes()); // item_protection_index
        infe2_content.extend_from_slice(b"hvc1"); // item_type
        infe2_content.push(0); // item_name null terminator
        iinf_entries.extend_from_slice(&make_fullbox(b"infe", 0, 0, &infe2_content));

        // iinf fullbox: version=0, flags=0, entry_count=2
        let mut iinf_content = (2u16).to_be_bytes().to_vec();
        iinf_content.extend_from_slice(&iinf_entries);
        meta_inner.extend_from_slice(&make_fullbox(b"iinf", 0, 0, &iinf_content));

        // iloc box with 2 items
        let mut iloc_content = Vec::new();
        iloc_content.push(0x00); // offset_size=0, length_size=0
        iloc_content.push(0x00); // base_offset_size=0
        iloc_content.extend_from_slice(&2u16.to_be_bytes()); // item_count=2

        // item 1 (XMP): item_id=1, data_reference_index=0, extent_count=0
        iloc_content.extend_from_slice(&1u16.to_be_bytes());
        iloc_content.extend_from_slice(&0u16.to_be_bytes());
        iloc_content.extend_from_slice(&0u16.to_be_bytes());

        // item 2 (hvc1): item_id=2, data_reference_index=0, extent_count=0
        iloc_content.extend_from_slice(&2u16.to_be_bytes());
        iloc_content.extend_from_slice(&0u16.to_be_bytes());
        iloc_content.extend_from_slice(&0u16.to_be_bytes());

        meta_inner.extend_from_slice(&make_fullbox(b"iloc", 0, 0, &iloc_content));

        // meta fullbox
        heic.extend_from_slice(&make_fullbox(b"meta", 0, 0, &meta_inner));

        // mdat box
        heic.extend_from_slice(&make_box(b"mdat", b"fake image data"));

        heic
    }

    #[test]
    fn test_heic_passthrough_no_metadata() {
        let input = create_minimal_heic();
        let output = HeicRemover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        assert_eq!(input, output, "HEIC with no metadata should pass through unchanged");
    }

    #[test]
    fn test_heic_strip_exif() {
        let input = create_heic_with_exif();
        let output = HeicRemover.remove_metadata(&input, &RemovalOptions::default()).unwrap();

        // Output should be smaller (EXIF item removed)
        assert!(output.len() < input.len(), "Output should be smaller after EXIF removal");

        // ftyp should be preserved
        assert_eq!(&output[4..8], b"ftyp");
        assert_eq!(&output[8..12], b"heic");

        // mdat should be preserved
        let mdat_pos = output.windows(4).position(|w| w == b"mdat").unwrap();
        assert_eq!(&output[mdat_pos..mdat_pos + 4], b"mdat");
    }

    #[test]
    fn test_heic_strip_xmp() {
        let input = create_heic_with_xmp();
        let output = HeicRemover.remove_metadata(&input, &RemovalOptions::default()).unwrap();

        // Output should be smaller (XMP item removed)
        assert!(output.len() < input.len(), "Output should be smaller after XMP removal");

        // ftyp should be preserved
        assert_eq!(&output[4..8], b"ftyp");
        assert_eq!(&output[8..12], b"heic");

        // mdat should be preserved
        let mdat_pos = output.windows(4).position(|w| w == b"mdat").unwrap();
        assert_eq!(&output[mdat_pos..mdat_pos + 4], b"mdat");
    }
}
