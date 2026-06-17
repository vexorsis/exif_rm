use crate::error::Error;
use crate::remove::isobmff::read_box_header;
use crate::traits::{FileFormat, MetadataRemover, RemovalOptions};
use std::collections::HashSet;
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
            && input.get(16..ftyp_size).is_none_or(|brands| !brands.chunks_exact(4).any(|b| b == b"heic"))
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
                    let meta_content = &input[box_start + header_size..box_end];
                    let original_meta_size = total_size;

                    let processed = process_meta_box(meta_content, original_meta_size, header_size, options)?;
                    // Write meta box header with correct size for the (possibly smaller) content
                    let new_size = (header_size + processed.len()) as u32;
                    output.extend_from_slice(&new_size.to_be_bytes());
                    output.extend_from_slice(b"meta");
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
/// `original_meta_total` is the original meta box total size (header + content).
/// `meta_header_size` is the box header size (8 or 16 for extended size).
fn process_meta_box(
    meta_data: &[u8],
    original_meta_total: usize,
    meta_header_size: usize,
    options: &RemovalOptions,
) -> crate::Result<Vec<u8>> {
    // meta is a fullbox: first 4 bytes are version(1) + flags(3)
    if meta_data.len() < 4 {
        return Err(Error::InvalidData("HEIC: meta box too short".into()));
    }

    let version_flags = &meta_data[0..4];

    // First pass: find metadata item IDs from iinf
    let mut removed_ids: HashSet<u16> = HashSet::new();
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
    if removed_ids.is_empty() && !options.icc_profile {
        return Ok(meta_data.to_vec());
    }

    // Second pass: rebuild meta box contents, filtering iinf and iloc
    let mut result = version_flags.to_vec();
    let mut iloc_result_range: Option<(usize, usize)> = None; // track where iloc ends up in result
    cursor.set_position(0);
    while let Some((total_size, header_size, box_type)) = read_box_header(&mut cursor) {
        let inner_start = cursor.position() as usize - header_size;
        let inner_end = inner_start + total_size;

        if inner_end > meta_data.len() - 4 {
            break;
        }

        let box_content = &meta_data[4 + inner_start + header_size..4 + inner_end];

        match &box_type {
            b"iinf" => {
                let rebuilt = rebuild_iinf(box_content, &removed_ids)?;
                // Write box header with correct size
                let new_size = (header_size + rebuilt.len()) as u32;
                result.extend_from_slice(&new_size.to_be_bytes());
                result.extend_from_slice(&box_type);
                result.extend_from_slice(&rebuilt);
            }
            b"iloc" => {
                let rebuilt = rebuild_iloc(box_content, &removed_ids)?;
                let new_size = (header_size + rebuilt.len()) as u32;
                let iloc_start = result.len();
                result.extend_from_slice(&new_size.to_be_bytes());
                result.extend_from_slice(&box_type);
                result.extend_from_slice(&rebuilt);
                let iloc_end = result.len();
                iloc_result_range = Some((iloc_start, iloc_end));
            }
            b"iref" => {
                let rebuilt = rebuild_iref(box_content, &removed_ids)?;
                if rebuilt.is_empty() {
                    // All references were removed; omit the entire iref box
                } else {
                    let new_size = (header_size + rebuilt.len()) as u32;
                    result.extend_from_slice(&new_size.to_be_bytes());
                    result.extend_from_slice(&box_type);
                    result.extend_from_slice(&rebuilt);
                }
            }
            b"iprp" => {
                if options.icc_profile {
                    let cleaned = process_iprp(box_content)?;
                    let new_size = (header_size + cleaned.len()) as u32;
                    result.extend_from_slice(&new_size.to_be_bytes());
                    result.extend_from_slice(&box_type);
                    result.extend_from_slice(&cleaned);
                } else {
                    result.extend_from_slice(&meta_data[4 + inner_start..4 + inner_end]);
                }
            }
            _ => {
                result.extend_from_slice(&meta_data[4 + inner_start..4 + inner_end]);
            }
        }

        cursor.set_position(inner_end as u64);
    }

    // Adjust iloc extent offsets for construction_method=0 items.
    // When the meta box shrinks, the mdat box moves earlier, making all absolute
    // file offsets in iloc stale. We need to shift them by the delta.
    let new_meta_total = meta_header_size + result.len();
    if new_meta_total != original_meta_total {
        let offset_delta = original_meta_total as i64 - new_meta_total as i64;
        if let Some((iloc_start, iloc_end)) = iloc_result_range {
            adjust_iloc_offsets(&mut result[iloc_start..iloc_end], offset_delta)?;
        }
    }

    Ok(result)
}

/// Find metadata item IDs from iinf box content (after the box header, i.e., fullbox content).
/// The content starts with version(1) + flags(3) + entry_count.
/// item_id width in infe entries depends on iinf version: 2 bytes for v0, 4 bytes for v1+.
fn find_metadata_item_ids(iinf_data: &[u8], options: &RemovalOptions) -> HashSet<u16> {
    let mut ids = HashSet::new();

    if iinf_data.len() < 6 {
        return ids;
    }

    let iinf_version = iinf_data[0];
    let entry_count = if iinf_version == 0 {
        u16::from_be_bytes([iinf_data[4], iinf_data[5]]) as usize
    } else {
        if iinf_data.len() < 8 {
            return ids;
        }
        u32::from_be_bytes([iinf_data[4], iinf_data[5], iinf_data[6], iinf_data[7]]) as usize
    };

    let item_id_size = if iinf_version == 0 { 2usize } else { 4 };

    let mut offset = if iinf_version == 0 { 6 } else { 8 };

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
        let _infe_version = infe_data[8];

        // item_id size depends on iinf_version, not infe_version
        // After fullbox header (12 bytes): item_id(item_id_size) + protection_index(2) + item_type(4)
        let item_type_offset = 12 + item_id_size + 2;
        if item_type_offset + 4 > infe_data.len() {
            offset += infe_size;
            continue;
        }

        let item_id = if item_id_size == 2 {
            u16::from_be_bytes([infe_data[12], infe_data[13]])
        } else {
            u32::from_be_bytes([infe_data[12], infe_data[13], infe_data[14], infe_data[15]]) as u16
        };

        let item_type = &infe_data[item_type_offset..item_type_offset + 4];

        let should_remove = (options.exif && item_type == b"Exif")
            || (options.xmp && item_type == b"mime");

        if should_remove {
            ids.insert(item_id);
        }

        offset += infe_size;
    }

    ids
}

/// Rebuild iinf box content (after the box header), excluding removed items.
/// The content starts with version(1) + flags(3) + entry_count.
fn rebuild_iinf(iinf_data: &[u8], removed_ids: &HashSet<u16>) -> crate::Result<Vec<u8>> {
    if iinf_data.len() < 6 {
        return Err(Error::InvalidData("HEIC: iinf box too short".into()));
    }

    let version = iinf_data[0];
    let flags = &iinf_data[1..4];
    let item_id_size = if version == 0 { 2usize } else { 4 };

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

        if !is_infe_removed(infe_data, removed_ids, item_id_size) {
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
/// item_id_size depends on the parent iinf version: 2 for v0, 4 for v1+.
fn is_infe_removed(infe_data: &[u8], removed_ids: &HashSet<u16>, item_id_size: usize) -> bool {
    if infe_data.len() < 12 + item_id_size {
        return false;
    }

    let item_id = if item_id_size == 2 {
        u16::from_be_bytes([infe_data[12], infe_data[13]])
    } else {
        u32::from_be_bytes([infe_data[12], infe_data[13], infe_data[14], infe_data[15]]) as u16
    };

    removed_ids.contains(&item_id)
}

/// Rebuild iloc box content (after the box header), excluding removed items.
/// The content starts with version(1) + flags(3) + size fields + item_count.
/// After rebuilding, `adjust_iloc_offsets` must be called to shift construction_method=0
/// extent offsets by the meta box shrinkage delta.
fn rebuild_iloc(iloc_data: &[u8], removed_ids: &HashSet<u16>) -> crate::Result<Vec<u8>> {
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
    let item_count_offset = 6;

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

/// Adjust iloc extent offsets in-place for construction_method=0 items.
/// `iloc_box` is the full iloc box including the box header (size + type).
/// When the meta box shrinks, the mdat box shifts earlier in the file, so all
/// absolute file offsets (construction_method=0) become stale and must be shifted
/// by `offset_delta` (positive = offsets need to decrease).
fn adjust_iloc_offsets(iloc_box: &mut [u8], offset_delta: i64) -> crate::Result<()> {
    if offset_delta == 0 {
        return Ok(());
    }

    // iloc box: [size(4)][type(4)][version(1)][flags(3)][offset_size|length_size(1)][base_offset_size|index_size(1)][item_count...]
    if iloc_box.len() < 14 {
        return Ok(());
    }

    let version = iloc_box[8];
    if version > 1 {
        return Ok(());
    }

    let offset_size = (iloc_box[12] >> 4) as usize;
    let length_size = (iloc_box[12] & 0x0F) as usize;
    let base_offset_size = (iloc_box[13] >> 4) as usize;
    let index_size = if version == 1 { (iloc_box[13] & 0x0F) as usize } else { 0 };

    let item_count_size = if version < 2 { 2 } else { 4 };
    let mut pos = 14 + item_count_size; // after box header + fullbox header + size fields + item_count

    if pos > iloc_box.len() {
        return Ok(());
    }

    let item_count = if version < 2 {
        u16::from_be_bytes([iloc_box[14], iloc_box[15]]) as usize
    } else {
        u32::from_be_bytes([iloc_box[14], iloc_box[15], iloc_box[16], iloc_box[17]]) as usize
    };

    for _ in 0..item_count {
        if pos + 2 > iloc_box.len() {
            break;
        }

        let construction_method = if version >= 1 {
            if pos + 4 > iloc_box.len() {
                break;
            }
            let cm = u16::from_be_bytes([iloc_box[pos + 2], iloc_box[pos + 3]]) & 0xF;
            pos += 4; // item_id(2) + construction_method(2)
            cm
        } else {
            pos += 2; // item_id(2)
            0
        };

        if pos + 2 > iloc_box.len() {
            break;
        }
        pos += 2; // data_reference_index

        // Adjust base_offset if present
        if base_offset_size > 0 {
            adjust_offset_field(iloc_box, pos, base_offset_size, offset_delta, construction_method == 0)?;
            pos += base_offset_size;
        }

        if pos + 2 > iloc_box.len() {
            break;
        }
        let extent_count = u16::from_be_bytes([iloc_box[pos], iloc_box[pos + 1]]) as usize;
        pos += 2; // extent_count

        for _ in 0..extent_count {
            if version >= 1 && index_size > 0 {
                pos += index_size; // extent_index
            }

            // Adjust extent_offset for construction_method=0 (file offsets)
            if offset_size > 0 {
                adjust_offset_field(iloc_box, pos, offset_size, offset_delta, construction_method == 0)?;
                pos += offset_size;
            }

            pos += length_size; // extent_length (not adjusted)
        }
    }

    Ok(())
}

/// Adjust a multi-byte offset field in-place. Only applies the adjustment if `should_adjust` is true
/// (i.e., construction_method == 0, meaning the offset is an absolute file position).
fn adjust_offset_field(buf: &mut [u8], pos: usize, field_size: usize, delta: i64, should_adjust: bool) -> crate::Result<()> {
    if !should_adjust || delta == 0 {
        return Ok(());
    }

    match field_size {
        4 => {
            if pos + 4 > buf.len() {
                return Ok(());
            }
            let old_val = u32::from_be_bytes([buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]]) as i64;
            let new_val = old_val - delta;
            if new_val < 0 {
                return Err(Error::InvalidData("HEIC: iloc offset underflow after adjustment".into()));
            }
            let bytes = (new_val as u32).to_be_bytes();
            buf[pos..pos + 4].copy_from_slice(&bytes);
        }
        8 => {
            if pos + 8 > buf.len() {
                return Ok(());
            }
            let old_val = u64::from_be_bytes([
                buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3],
                buf[pos + 4], buf[pos + 5], buf[pos + 6], buf[pos + 7],
            ]) as i64;
            let new_val = old_val - delta;
            if new_val < 0 {
                return Err(Error::InvalidData("HEIC: iloc offset underflow after adjustment".into()));
            }
            let bytes = (new_val as u64).to_be_bytes();
            buf[pos..pos + 8].copy_from_slice(&bytes);
        }
        _ => {}
    }

    Ok(())
}

/// Rebuild iref box content (after the box header), removing entries whose
/// from_item_id is in removed_ids. The content is a fullbox: version(1) + flags(3),
/// then a sequence of reference entries, each being a box with:
///   size(4) + reference_type(4) + from_item_id(2 for v0, 4 for v1+) +
///   reference_count(2) + to_item_ids(2 or 4 each).
/// Returns empty Vec if all references are removed (caller should omit the box).
fn rebuild_iref(iref_data: &[u8], removed_ids: &HashSet<u16>) -> crate::Result<Vec<u8>> {
    if iref_data.len() < 4 {
        return Err(Error::InvalidData("HEIC: iref box too short".into()));
    }

    let version = iref_data[0];
    let flags = &iref_data[1..4];
    let from_id_size: usize = if version == 0 { 2 } else { 4 };
    let _to_id_size: usize = if version == 0 { 2 } else { 4 };

    let mut result = vec![version];
    result.extend_from_slice(flags);

    let mut offset = 4;
    let mut any_kept = false;

    while offset + 8 <= iref_data.len() {
        let entry_size = u32::from_be_bytes(iref_data[offset..offset + 4].try_into().unwrap()) as usize;
        let _ref_type = &iref_data[offset + 4..offset + 8];

        if entry_size < 8 + from_id_size + 2 || offset + entry_size > iref_data.len() {
            break;
        }

        let from_id_pos = offset + 8;
        let from_id = if from_id_size == 2 {
            u16::from_be_bytes([iref_data[from_id_pos], iref_data[from_id_pos + 1]])
        } else {
            u32::from_be_bytes([
                iref_data[from_id_pos],
                iref_data[from_id_pos + 1],
                iref_data[from_id_pos + 2],
                iref_data[from_id_pos + 3],
            ]) as u16
        };

        if removed_ids.contains(&from_id) {
            offset += entry_size;
            continue;
        }

        // Keep this reference entry
        result.extend_from_slice(&iref_data[offset..offset + entry_size]);
        any_kept = true;
        offset += entry_size;
    }

    if any_kept {
        Ok(result)
    } else {
        Ok(Vec::new())
    }
}

fn process_iprp(iprp_data: &[u8]) -> crate::Result<Vec<u8>> {
    let mut cursor = Cursor::new(iprp_data);
    let mut colr_property_index: Option<u8> = None;

    // First pass: find colr box index in ipco
    while let Some((total_size, header_size, box_type)) = read_box_header(&mut cursor) {
        let data_start = cursor.position() as usize;
        let box_end = data_start + total_size - header_size;

        if box_end > iprp_data.len() {
            break;
        }

        if &box_type == b"ipco" {
            let ipco_data = &iprp_data[data_start..box_end];
            let mut ipco_cursor = Cursor::new(ipco_data);
            let mut idx: u8 = 1;
            while let Some((ipco_total, ipco_header, ipco_type)) = read_box_header(&mut ipco_cursor) {
                if &ipco_type == b"colr" {
                    colr_property_index = Some(idx);
                    break;
                }
                let ipco_end = ipco_cursor.position() as usize + ipco_total - ipco_header;
                if ipco_end > ipco_data.len() {
                    break;
                }
                idx += 1;
                ipco_cursor.set_position(ipco_end as u64);
            }
        }

        cursor.set_position(box_end as u64);
    }

    let colr_idx = match colr_property_index {
        Some(i) => i,
        None => return Ok(iprp_data.to_vec()),
    };

    // Second pass: rebuild iprp
    let mut result = Vec::with_capacity(iprp_data.len());
    cursor.set_position(0);

    while let Some((total_size, header_size, box_type)) = read_box_header(&mut cursor) {
        let box_start = cursor.position() as usize - header_size;
        let data_start = cursor.position() as usize;
        let box_end = box_start + total_size;

        if box_end > iprp_data.len() {
            break;
        }

        match &box_type {
            b"ipco" => {
                let ipco_data = &iprp_data[data_start..box_end];
                let cleaned = rebuild_ipco(ipco_data, colr_idx)?;
                let new_size = (header_size + cleaned.len()) as u32;
                result.extend_from_slice(&new_size.to_be_bytes());
                result.extend_from_slice(b"ipco");
                result.extend_from_slice(&cleaned);
            }
            b"ipma" => {
                let ipma_data = &iprp_data[data_start..box_end];
                let cleaned = rebuild_ipma(ipma_data, colr_idx)?;
                let new_size = (header_size + cleaned.len()) as u32;
                result.extend_from_slice(&new_size.to_be_bytes());
                result.extend_from_slice(b"ipma");
                result.extend_from_slice(&cleaned);
            }
            _ => {
                result.extend_from_slice(&iprp_data[box_start..box_end]);
            }
        }

        cursor.set_position(box_end as u64);
    }

    Ok(result)
}

fn rebuild_ipco(ipco_data: &[u8], colr_index: u8) -> crate::Result<Vec<u8>> {
    let mut result = Vec::with_capacity(ipco_data.len());
    let mut cursor = Cursor::new(ipco_data);
    let mut idx: u8 = 1;

    while let Some((total_size, header_size, _box_type)) = read_box_header(&mut cursor) {
        let box_start = cursor.position() as usize - header_size;
        let box_end = box_start + total_size;

        if box_end > ipco_data.len() {
            break;
        }

        if idx != colr_index {
            result.extend_from_slice(&ipco_data[box_start..box_end]);
        }
        idx += 1;
        cursor.set_position(box_end as u64);
    }

    Ok(result)
}

fn rebuild_ipma(ipma_data: &[u8], colr_index: u8) -> crate::Result<Vec<u8>> {
    if ipma_data.len() < 8 {
        return Ok(ipma_data.to_vec());
    }
    let version = ipma_data[0];
    let flags = u32::from_be_bytes([0, ipma_data[1], ipma_data[2], ipma_data[3]]);
    let entry_count = u32::from_be_bytes(ipma_data[4..8].try_into().unwrap()) as usize;

    let mut result = Vec::with_capacity(ipma_data.len());
    let vf = ((version as u32) << 24) | flags;
    result.extend_from_slice(&vf.to_be_bytes());
    result.extend_from_slice(&entry_count.to_be_bytes());

    let mut pos = 8;
    for _ in 0..entry_count {
        let item_id_size = if version < 1 { 2usize } else { 4 };
        if pos + item_id_size > ipma_data.len() {
            break;
        }
        result.extend_from_slice(&ipma_data[pos..pos + item_id_size]);
        pos += item_id_size;

        if pos + 1 > ipma_data.len() {
            break;
        }
        let association_count = ipma_data[pos] as usize;
        result.push(ipma_data[pos]);
        pos += 1;

        for _ in 0..association_count {
            let assoc_size = if version < 1 { 1 } else { 2 };
            if pos + assoc_size > ipma_data.len() {
                break;
            }
            let property_index: u16 = if version < 1 {
                (ipma_data[pos] & 0x7F) as u16
            } else {
                u16::from_be_bytes([ipma_data[pos], ipma_data[pos + 1]]) & 0x7FFF
            };

            if property_index != colr_index as u16 {
                result.extend_from_slice(&ipma_data[pos..pos + assoc_size]);
            }
            pos += assoc_size;
        }
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

    fn create_heic_with_icc() -> Vec<u8> {
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

        let mut iinf_entries = Vec::new();
        let mut infe1 = Vec::new();
        infe1.extend_from_slice(&1u16.to_be_bytes());
        infe1.extend_from_slice(&0u16.to_be_bytes());
        infe1.extend_from_slice(b"hvc1");
        infe1.push(0);
        iinf_entries.extend_from_slice(&make_fullbox(b"infe", 0, 0, &infe1));

        let mut iinf_content = Vec::new();
        iinf_content.extend_from_slice(&1u16.to_be_bytes());
        iinf_content.extend_from_slice(&iinf_entries);
        let iinf = make_fullbox(b"iinf", 0, 0, &iinf_content);

        let mut iloc_content = Vec::new();
        iloc_content.push(0x44);
        iloc_content.push(0x00);
        iloc_content.extend_from_slice(&1u16.to_be_bytes());
        iloc_content.extend_from_slice(&1u16.to_be_bytes());
        iloc_content.extend_from_slice(&0u16.to_be_bytes());
        iloc_content.extend_from_slice(&1u16.to_be_bytes());
        iloc_content.extend_from_slice(&100u32.to_be_bytes());
        iloc_content.extend_from_slice(&20u32.to_be_bytes());
        let iloc = make_fullbox(b"iloc", 0, 0, &iloc_content);

        // iprp with ipco (ispe + colr) and ipma
        let ispe = make_box(b"ispe", &[0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01]);
        let colr = make_box(b"colr", b"nclx\x01\x01\x01\x00");
        let mut ipco_content = Vec::new();
        ipco_content.extend_from_slice(&ispe);
        ipco_content.extend_from_slice(&colr);
        let ipco = make_box(b"ipco", &ipco_content);

        // ipma (v0): 1 entry, item_id=1, 2 associations [ispe(1), colr(2)]
        let mut ipma_content = Vec::new();
        ipma_content.extend_from_slice(&0u32.to_be_bytes()); // version=0, flags=0
        ipma_content.extend_from_slice(&1u32.to_be_bytes()); // entry_count
        ipma_content.extend_from_slice(&1u16.to_be_bytes()); // item_id
        ipma_content.push(2u8); // association_count
        ipma_content.push(0x81); // essential=1, property_index=1 (ispe)
        ipma_content.push(0x82); // essential=1, property_index=2 (colr)
        let ipma = make_box(b"ipma", &ipma_content);

        let mut iprp_content = Vec::new();
        iprp_content.extend_from_slice(&ipco);
        iprp_content.extend_from_slice(&ipma);
        let iprp = make_box(b"iprp", &iprp_content);

        let mut meta_inner = Vec::new();
        meta_inner.extend_from_slice(&hdlr);
        meta_inner.extend_from_slice(&iinf);
        meta_inner.extend_from_slice(&iloc);
        meta_inner.extend_from_slice(&iprp);
        let meta = make_fullbox(b"meta", 0, 0, &meta_inner);
        heic.extend_from_slice(&meta);

        heic.extend_from_slice(&make_box(b"mdat", b"fake image data"));
        heic
    }

    #[test]
    fn test_heic_strip_icc() {
        let input = create_heic_with_icc();
        let options = RemovalOptions { icc_profile: true, ..RemovalOptions::default() };
        let output = HeicRemover.remove_metadata(&input, &options).unwrap();
        assert!(!output.windows(4).any(|w| w == b"colr"), "colr should be removed when icc_profile option is set");
        assert!(output.windows(4).any(|w| w == b"ispe"), "ispe should be preserved");
        assert_eq!(&output[4..8], b"ftyp");
        assert!(output.windows(4).any(|w| w == b"mdat"));
    }

    #[test]
    fn test_heic_keep_icc_by_default() {
        let input = create_heic_with_icc();
        let output = HeicRemover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        assert!(output.windows(4).any(|w| w == b"colr"), "colr should be preserved by default");
    }

    #[test]
    fn test_heic_invalid_header() {
        let input = b"not a heic file at all".to_vec();
        let result = HeicRemover.remove_metadata(&input, &RemovalOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_heic_missing_meta_box() {
        let mut heic = Vec::new();
        let mut ftyp_content = b"heic".to_vec();
        ftyp_content.extend_from_slice(&0u32.to_be_bytes());
        heic.extend_from_slice(&make_box(b"ftyp", &ftyp_content));
        heic.extend_from_slice(&make_box(b"mdat", b"fake image data"));
        let result = HeicRemover.remove_metadata(&heic, &RemovalOptions::default());
        assert!(result.is_err(), "HEIC without meta box should error");
    }

    #[test]
    fn test_heic_truncated_data() {
        let mut heic = Vec::new();
        let mut ftyp_content = b"heic".to_vec();
        ftyp_content.extend_from_slice(&0u32.to_be_bytes());
        heic.extend_from_slice(&make_box(b"ftyp", &ftyp_content));
        heic.extend_from_slice(&50u32.to_be_bytes());
        heic.extend_from_slice(b"meta");
        let result = HeicRemover.remove_metadata(&heic, &RemovalOptions::default());
        assert!(result.is_err(), "truncated HEIC should error");
    }

    #[test]
    fn test_heic_strip_iref_orphaned_references() {
        // Build a HEIC with Exif item (id=1), hvc1 item (id=2), and an iref with
        // cdsc from Exif(1)->hvc1(2). After stripping, the cdsc reference should be removed.
        let mut heic = Vec::new();

        // ftyp
        let mut ftyp_content = b"heic".to_vec();
        ftyp_content.extend_from_slice(&0u32.to_be_bytes());
        heic.extend_from_slice(&make_box(b"ftyp", &ftyp_content));

        let mut meta_inner = Vec::new();

        // hdlr
        let mut hdlr_content = Vec::new();
        hdlr_content.extend_from_slice(&0u32.to_be_bytes());
        hdlr_content.extend_from_slice(b"pict");
        hdlr_content.extend_from_slice(&[0u8; 12]);
        hdlr_content.push(0);
        meta_inner.extend_from_slice(&make_fullbox(b"hdlr", 0, 0, &hdlr_content));

        // iinf: Exif(id=1) + hvc1(id=2)
        let mut iinf_entries = Vec::new();
        let mut infe1_content = Vec::new();
        infe1_content.extend_from_slice(&1u16.to_be_bytes());
        infe1_content.extend_from_slice(&0u16.to_be_bytes());
        infe1_content.extend_from_slice(b"Exif");
        infe1_content.push(0);
        iinf_entries.extend_from_slice(&make_fullbox(b"infe", 0, 0, &infe1_content));

        let mut infe2_content = Vec::new();
        infe2_content.extend_from_slice(&2u16.to_be_bytes());
        infe2_content.extend_from_slice(&0u16.to_be_bytes());
        infe2_content.extend_from_slice(b"hvc1");
        infe2_content.push(0);
        iinf_entries.extend_from_slice(&make_fullbox(b"infe", 0, 0, &infe2_content));

        let mut iinf_content = (2u16).to_be_bytes().to_vec();
        iinf_content.extend_from_slice(&iinf_entries);
        meta_inner.extend_from_slice(&make_fullbox(b"iinf", 0, 0, &iinf_content));

        // iloc
        let mut iloc_content = Vec::new();
        iloc_content.push(0x00);
        iloc_content.push(0x00);
        iloc_content.extend_from_slice(&2u16.to_be_bytes());
        iloc_content.extend_from_slice(&1u16.to_be_bytes());
        iloc_content.extend_from_slice(&0u16.to_be_bytes());
        iloc_content.extend_from_slice(&0u16.to_be_bytes());
        iloc_content.extend_from_slice(&2u16.to_be_bytes());
        iloc_content.extend_from_slice(&0u16.to_be_bytes());
        iloc_content.extend_from_slice(&0u16.to_be_bytes());
        meta_inner.extend_from_slice(&make_fullbox(b"iloc", 0, 0, &iloc_content));

        // iref: cdsc from Exif(1) -> hvc1(2)
        let mut iref_entry = Vec::new();
        iref_entry.extend_from_slice(b"cdsc"); // reference_type
        iref_entry.extend_from_slice(&1u16.to_be_bytes()); // from_item_id
        iref_entry.extend_from_slice(&1u16.to_be_bytes()); // reference_count
        iref_entry.extend_from_slice(&2u16.to_be_bytes()); // to_item_id
        let iref_entry_size = (8 + iref_entry.len()) as u32;
        let mut iref_entry_box = iref_entry_size.to_be_bytes().to_vec();
        iref_entry_box.extend_from_slice(&iref_entry);
        meta_inner.extend_from_slice(&make_fullbox(b"iref", 0, 0, &iref_entry_box));

        heic.extend_from_slice(&make_fullbox(b"meta", 0, 0, &meta_inner));
        heic.extend_from_slice(&make_box(b"mdat", b"fake image data"));

        let output = HeicRemover.remove_metadata(&heic, &RemovalOptions::default()).unwrap();

        // The iref box should be completely absent (all references were from removed items)
        // or have no cdsc entries. Check that "cdsc" doesn't appear in output.
        assert!(!output.windows(4).any(|w| w == b"cdsc"),
            "cdsc reference from removed Exif item should be gone");
        // The hvc1 item should still be present
        assert!(output.windows(4).any(|w| w == b"hvc1"),
            "hvc1 item should still be present");
    }

    /// Create a HEIC with iloc v1, construction_method=0, real offset/length sizes (4/4).
    /// Contains: hvc1 (id=1) + Exif (id=2) + hvc1 (id=3).
    /// The iloc offsets point into an mdat box that follows the meta box.
    fn create_heic_with_iloc_v1_offsets() -> Vec<u8> {
        let mut heic = Vec::new();

        // ftyp box (36 bytes: 8 header + 28 content)
        let mut ftyp_content = b"heic".to_vec();
        ftyp_content.extend_from_slice(&0u32.to_be_bytes());
        ftyp_content.extend_from_slice(b"mif1");
        heic.extend_from_slice(&make_box(b"ftyp", &ftyp_content));

        // Build meta inner boxes
        let mut meta_inner = Vec::new();

        // hdlr
        let mut hdlr_content = Vec::new();
        hdlr_content.extend_from_slice(&0u32.to_be_bytes());
        hdlr_content.extend_from_slice(b"pict");
        hdlr_content.extend_from_slice(&[0u8; 12]);
        hdlr_content.push(0);
        meta_inner.extend_from_slice(&make_fullbox(b"hdlr", 0, 0, &hdlr_content));

        // iinf with 3 items: hvc1(1), Exif(2), hvc1(3)
        let mut iinf_entries = Vec::new();
        // infe for hvc1 id=1
        let mut infe1 = Vec::new();
        infe1.extend_from_slice(&1u16.to_be_bytes());
        infe1.extend_from_slice(&0u16.to_be_bytes());
        infe1.extend_from_slice(b"hvc1");
        infe1.push(0);
        iinf_entries.extend_from_slice(&make_fullbox(b"infe", 0, 0, &infe1));
        // infe for Exif id=2
        let mut infe2 = Vec::new();
        infe2.extend_from_slice(&2u16.to_be_bytes());
        infe2.extend_from_slice(&0u16.to_be_bytes());
        infe2.extend_from_slice(b"Exif");
        infe2.push(0);
        iinf_entries.extend_from_slice(&make_fullbox(b"infe", 0, 0, &infe2));
        // infe for hvc1 id=3
        let mut infe3 = Vec::new();
        infe3.extend_from_slice(&3u16.to_be_bytes());
        infe3.extend_from_slice(&0u16.to_be_bytes());
        infe3.extend_from_slice(b"hvc1");
        infe3.push(0);
        iinf_entries.extend_from_slice(&make_fullbox(b"infe", 0, 0, &infe3));

        let mut iinf_content = (3u16).to_be_bytes().to_vec();
        iinf_content.extend_from_slice(&iinf_entries);
        meta_inner.extend_from_slice(&make_fullbox(b"iinf", 0, 0, &iinf_content));

        // iloc v1 with offset_size=4, length_size=4, base_offset_size=0, index_size=0
        // 3 items, all construction_method=0 (file offsets)
        // We'll use placeholder offsets; the test will verify they're adjusted correctly.
        let mut iloc_content = Vec::new();
        iloc_content.push(0x44); // offset_size=4, length_size=4
        iloc_content.push(0x00); // base_offset_size=0, index_size=0
        iloc_content.extend_from_slice(&3u16.to_be_bytes()); // item_count=3

        // item 1 (hvc1): item_id=1, construction_method=0, data_ref=0, 1 extent
        iloc_content.extend_from_slice(&1u16.to_be_bytes()); // item_id
        iloc_content.extend_from_slice(&0u16.to_be_bytes()); // construction_method=0
        iloc_content.extend_from_slice(&0u16.to_be_bytes()); // data_reference_index
        iloc_content.extend_from_slice(&1u16.to_be_bytes()); // extent_count=1
        iloc_content.extend_from_slice(&999u32.to_be_bytes()); // extent_offset (placeholder)
        iloc_content.extend_from_slice(&100u32.to_be_bytes()); // extent_length

        // item 2 (Exif): item_id=2, construction_method=0, data_ref=0, 1 extent
        iloc_content.extend_from_slice(&2u16.to_be_bytes()); // item_id
        iloc_content.extend_from_slice(&0u16.to_be_bytes()); // construction_method=0
        iloc_content.extend_from_slice(&0u16.to_be_bytes()); // data_reference_index
        iloc_content.extend_from_slice(&1u16.to_be_bytes()); // extent_count=1
        iloc_content.extend_from_slice(&1200u32.to_be_bytes()); // extent_offset (placeholder)
        iloc_content.extend_from_slice(&50u32.to_be_bytes()); // extent_length

        // item 3 (hvc1): item_id=3, construction_method=0, data_ref=0, 1 extent
        iloc_content.extend_from_slice(&3u16.to_be_bytes()); // item_id
        iloc_content.extend_from_slice(&0u16.to_be_bytes()); // construction_method=0
        iloc_content.extend_from_slice(&0u16.to_be_bytes()); // data_reference_index
        iloc_content.extend_from_slice(&1u16.to_be_bytes()); // extent_count=1
        iloc_content.extend_from_slice(&1300u32.to_be_bytes()); // extent_offset (placeholder)
        iloc_content.extend_from_slice(&200u32.to_be_bytes()); // extent_length

        meta_inner.extend_from_slice(&make_fullbox(b"iloc", 1, 0, &iloc_content));

        // meta fullbox
        heic.extend_from_slice(&make_fullbox(b"meta", 0, 0, &meta_inner));

        // mdat box
        heic.extend_from_slice(&make_box(b"mdat", b"fake image data that is long enough"));

        heic
    }

    /// Extract extent offsets from an iloc v1 box in the output.
    /// Returns vec of (item_id, construction_method, vec of (extent_offset, extent_length))
    fn parse_iloc_offsets(data: &[u8]) -> Vec<(u16, u16, Vec<(u32, u32)>)> {
        // Find iloc box
        let pos = data.windows(4).position(|w| w == b"iloc").expect("iloc not found");
        let iloc_start = pos - 4;
        let iloc_size = u32::from_be_bytes(data[iloc_start..iloc_start + 4].try_into().unwrap()) as usize;
        let _iloc_end = iloc_start + iloc_size;

        // Skip box header (8) + fullbox header (4)
        let version = data[iloc_start + 8];
        let offset_size = (data[iloc_start + 12] >> 4) as usize;
        let length_size = (data[iloc_start + 12] & 0x0F) as usize;
        let base_offset_size = (data[iloc_start + 13] >> 4) as usize;
        let index_size = if version >= 1 { (data[iloc_start + 13] & 0x0F) as usize } else { 0 };

        let item_count_off = iloc_start + 14;
        let item_count = u16::from_be_bytes(data[item_count_off..item_count_off + 2].try_into().unwrap()) as usize;

        let mut pos = item_count_off + 2;
        let mut items = Vec::new();

        for _ in 0..item_count {
            let item_id = u16::from_be_bytes(data[pos..pos + 2].try_into().unwrap());
            pos += 2;
            let construction_method = if version >= 1 {
                let cm = u16::from_be_bytes(data[pos..pos + 2].try_into().unwrap()) & 0xF;
                pos += 2;
                cm
            } else {
                0
            };
            pos += 2; // data_reference_index
            pos += base_offset_size; // base_offset
            let extent_count = u16::from_be_bytes(data[pos..pos + 2].try_into().unwrap()) as usize;
            pos += 2;
            let mut extents = Vec::new();
            for _ in 0..extent_count {
                pos += index_size;
                let ext_offset = if offset_size == 4 {
                    let v = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap());
                    pos += 4;
                    v
                } else {
                    pos += offset_size;
                    0
                };
                let ext_length = if length_size == 4 {
                    let v = u32::from_be_bytes(data[pos..pos + 4].try_into().unwrap());
                    pos += 4;
                    v
                } else {
                    pos += length_size;
                    0
                };
                extents.push((ext_offset, ext_length));
            }
            items.push((item_id, construction_method, extents));
        }
        items
    }

    #[test]
    fn test_heic_iloc_v1_offset_adjustment() {
        let input = create_heic_with_iloc_v1_offsets();

        // Parse original offsets
        let orig_items = parse_iloc_offsets(&input);
        let orig_item1_offset = orig_items.iter().find(|(id, _, _)| *id == 1).unwrap().2[0].0;
        let orig_item3_offset = orig_items.iter().find(|(id, _, _)| *id == 3).unwrap().2[0].0;

        // Strip EXIF (item 2 will be removed, meta box shrinks)
        let output = HeicRemover.remove_metadata(&input, &RemovalOptions::default()).unwrap();

        // Parse stripped offsets
        let strip_items = parse_iloc_offsets(&output);

        // Item 2 (Exif) should be gone
        assert!(strip_items.iter().all(|(id, _, _)| *id != 2), "Exif item should be removed");

        // Items 1 and 3 should still be present
        let strip_item1 = strip_items.iter().find(|(id, _, _)| *id == 1).unwrap();
        let strip_item3 = strip_items.iter().find(|(id, _, _)| *id == 3).unwrap();

        // Both should be construction_method=0
        assert_eq!(strip_item1.1, 0);
        assert_eq!(strip_item3.1, 0);

        // Offsets should be adjusted by exactly the meta box shrinkage
        // Find original and stripped meta box sizes
        let orig_meta_pos = input.windows(4).position(|w| w == b"meta").unwrap() - 4;
        let orig_meta_size = u32::from_be_bytes(input[orig_meta_pos..orig_meta_pos + 4].try_into().unwrap()) as usize;
        let strip_meta_pos = output.windows(4).position(|w| w == b"meta").unwrap() - 4;
        let strip_meta_size = u32::from_be_bytes(output[strip_meta_pos..strip_meta_pos + 4].try_into().unwrap()) as usize;

        let expected_delta = orig_meta_size - strip_meta_size;
        assert!(expected_delta > 0, "meta should have shrunk");

        let strip_item1_offset = strip_item1.2[0].0;
        let strip_item3_offset = strip_item3.2[0].0;

        assert_eq!(strip_item1_offset, orig_item1_offset - expected_delta as u32,
            "item 1 offset should be adjusted by meta shrinkage");
        assert_eq!(strip_item3_offset, orig_item3_offset - expected_delta as u32,
            "item 3 offset should be adjusted by meta shrinkage");

        // Lengths should be unchanged
        let orig_item1_length = orig_items.iter().find(|(id, _, _)| *id == 1).unwrap().2[0].1;
        let orig_item3_length = orig_items.iter().find(|(id, _, _)| *id == 3).unwrap().2[0].1;
        assert_eq!(strip_item1.2[0].1, orig_item1_length, "item 1 length should be unchanged");
        assert_eq!(strip_item3.2[0].1, orig_item3_length, "item 3 length should be unchanged");
    }

    /// Create a HEIC with iloc v1 containing both construction_method=0 (file offset)
    /// and construction_method=1 (idat offset) items.
    /// Items: hvc1 id=1 (cm=0), Exif id=2 (cm=0), hvc1 id=3 (cm=1).
    fn create_heic_with_mixed_construction_methods() -> Vec<u8> {
        let mut heic = Vec::new();

        // ftyp
        let mut ftyp_content = b"heic".to_vec();
        ftyp_content.extend_from_slice(&0u32.to_be_bytes());
        ftyp_content.extend_from_slice(b"mif1");
        heic.extend_from_slice(&make_box(b"ftyp", &ftyp_content));

        let mut meta_inner = Vec::new();

        // hdlr
        let mut hdlr_content = Vec::new();
        hdlr_content.extend_from_slice(&0u32.to_be_bytes());
        hdlr_content.extend_from_slice(b"pict");
        hdlr_content.extend_from_slice(&[0u8; 12]);
        hdlr_content.push(0);
        meta_inner.extend_from_slice(&make_fullbox(b"hdlr", 0, 0, &hdlr_content));

        // iinf: hvc1(1), Exif(2), hvc1(3)
        let mut iinf_entries = Vec::new();
        for (id, item_type) in [(1u16, b"hvc1"), (2u16, b"Exif"), (3u16, b"hvc1")] {
            let mut infe = Vec::new();
            infe.extend_from_slice(&id.to_be_bytes());
            infe.extend_from_slice(&0u16.to_be_bytes());
            infe.extend_from_slice(item_type);
            infe.push(0);
            iinf_entries.extend_from_slice(&make_fullbox(b"infe", 0, 0, &infe));
        }
        let mut iinf_content = (3u16).to_be_bytes().to_vec();
        iinf_content.extend_from_slice(&iinf_entries);
        meta_inner.extend_from_slice(&make_fullbox(b"iinf", 0, 0, &iinf_content));

        // iloc v1: offset_size=4, length_size=4, base_offset_size=0, index_size=0
        let mut iloc_content = Vec::new();
        iloc_content.push(0x44); // offset_size=4, length_size=4
        iloc_content.push(0x00); // base_offset_size=0, index_size=0
        iloc_content.extend_from_slice(&3u16.to_be_bytes()); // item_count

        // item 1: hvc1, cm=0 (file offset)
        iloc_content.extend_from_slice(&1u16.to_be_bytes()); // item_id
        iloc_content.extend_from_slice(&0u16.to_be_bytes()); // construction_method=0
        iloc_content.extend_from_slice(&0u16.to_be_bytes()); // data_reference_index
        iloc_content.extend_from_slice(&1u16.to_be_bytes()); // extent_count
        iloc_content.extend_from_slice(&500u32.to_be_bytes()); // extent_offset (file offset)
        iloc_content.extend_from_slice(&100u32.to_be_bytes()); // extent_length

        // item 2: Exif, cm=0 (file offset) — will be removed
        iloc_content.extend_from_slice(&2u16.to_be_bytes()); // item_id
        iloc_content.extend_from_slice(&0u16.to_be_bytes()); // construction_method=0
        iloc_content.extend_from_slice(&0u16.to_be_bytes()); // data_reference_index
        iloc_content.extend_from_slice(&1u16.to_be_bytes()); // extent_count
        iloc_content.extend_from_slice(&700u32.to_be_bytes()); // extent_offset
        iloc_content.extend_from_slice(&50u32.to_be_bytes()); // extent_length

        // item 3: hvc1, cm=1 (idat offset) — should NOT be adjusted
        iloc_content.extend_from_slice(&3u16.to_be_bytes()); // item_id
        iloc_content.extend_from_slice(&1u16.to_be_bytes()); // construction_method=1
        iloc_content.extend_from_slice(&0u16.to_be_bytes()); // data_reference_index
        iloc_content.extend_from_slice(&1u16.to_be_bytes()); // extent_count
        iloc_content.extend_from_slice(&42u32.to_be_bytes()); // extent_offset (idat-relative)
        iloc_content.extend_from_slice(&200u32.to_be_bytes()); // extent_length

        meta_inner.extend_from_slice(&make_fullbox(b"iloc", 1, 0, &iloc_content));

        // idat box (for construction_method=1 items)
        let idat_content = vec![0xAAu8; 200];
        meta_inner.extend_from_slice(&make_box(b"idat", &idat_content));

        heic.extend_from_slice(&make_fullbox(b"meta", 0, 0, &meta_inner));
        heic.extend_from_slice(&make_box(b"mdat", b"fake image data"));
        heic
    }

    #[test]
    fn test_heic_iloc_cm0_adjusted_cm1_unchanged() {
        let input = create_heic_with_mixed_construction_methods();
        let orig_items = parse_iloc_offsets(&input);

        let orig_item1_offset = orig_items.iter().find(|(id, _, _)| *id == 1).unwrap().2[0].0;
        let orig_item3_offset = orig_items.iter().find(|(id, _, _)| *id == 3).unwrap().2[0].0;

        let output = HeicRemover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        let strip_items = parse_iloc_offsets(&output);

        // Item 2 (Exif) removed
        assert!(strip_items.iter().all(|(id, _, _)| *id != 2));

        let strip_item1 = strip_items.iter().find(|(id, _, _)| *id == 1).unwrap();
        let strip_item3 = strip_items.iter().find(|(id, _, _)| *id == 3).unwrap();

        // Item 1 (cm=0) offset should be adjusted
        assert_eq!(strip_item1.1, 0);
        let orig_meta_pos = input.windows(4).position(|w| w == b"meta").unwrap() - 4;
        let orig_meta_size = u32::from_be_bytes(input[orig_meta_pos..orig_meta_pos + 4].try_into().unwrap()) as usize;
        let strip_meta_pos = output.windows(4).position(|w| w == b"meta").unwrap() - 4;
        let strip_meta_size = u32::from_be_bytes(output[strip_meta_pos..strip_meta_pos + 4].try_into().unwrap()) as usize;
        let expected_delta = orig_meta_size - strip_meta_size;

        assert!(expected_delta > 0);
        assert_eq!(strip_item1.2[0].0, orig_item1_offset - expected_delta as u32,
            "cm=0 item offset should be adjusted by meta shrinkage");

        // Item 3 (cm=1) offset should NOT be adjusted (idat-relative)
        assert_eq!(strip_item3.1, 1);
        assert_eq!(strip_item3.2[0].0, orig_item3_offset,
            "cm=1 item offset should be unchanged (idat-relative)");
    }

    /// Verify that all box sizes in the output are internally consistent.
    /// This catches the bug where rebuilt boxes keep stale size headers.
    fn verify_box_sizes(data: &[u8]) {
        let mut cursor = Cursor::new(data);
        while let Some((total_size, header_size, box_type)) = read_box_header(&mut cursor) {
            let declared_end = cursor.position() as usize - header_size + total_size;
            assert!(declared_end <= data.len(),
                "box {:?} declares size {} but data is only {} bytes",
                std::str::from_utf8(&box_type).unwrap_or("????"),
                declared_end, data.len());
            cursor.set_position((cursor.position() as usize - header_size + total_size) as u64);
        }
    }

    #[test]
    fn test_heic_strip_exif_box_sizes_consistent() {
        let input = create_heic_with_exif();
        let output = HeicRemover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        verify_box_sizes(&output);
    }

    #[test]
    fn test_heic_strip_xmp_box_sizes_consistent() {
        let input = create_heic_with_xmp();
        let output = HeicRemover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        verify_box_sizes(&output);
    }

    #[test]
    fn test_heic_strip_icc_box_sizes_consistent() {
        let input = create_heic_with_icc();
        let options = RemovalOptions { icc_profile: true, ..RemovalOptions::default() };
        let output = HeicRemover.remove_metadata(&input, &options).unwrap();
        verify_box_sizes(&output);
    }
}
