use std::io::{Cursor, Write};

/// Read a box header and return (total_size, header_size, box_type)
pub fn read_box_header(cursor: &mut Cursor<&[u8]>) -> Option<(usize, usize, [u8; 4])> {
    let pos = cursor.position() as usize;
    let data = cursor.get_ref();

    if pos + 8 > data.len() {
        return None;
    }

    let size = u32::from_be_bytes(data[pos..pos + 4].try_into().ok()?) as usize;
    let box_type: [u8; 4] = data[pos + 4..pos + 8].try_into().ok()?;

    let (total_size, header_size) = if size == 1 {
        if pos + 16 > data.len() {
            return None;
        }
        let ext_size = u64::from_be_bytes(data[pos + 8..pos + 16].try_into().ok()?) as usize;
        (ext_size, 16)
    } else if size == 0 {
        (data.len() - pos, 8)
    } else {
        (size, 8)
    };

    cursor.set_position((pos + header_size) as u64);
    Some((total_size, header_size, box_type))
}

/// Read a full box header and return (total_size, header_size, box_type, version, flags)
pub fn read_fullbox_header(cursor: &mut Cursor<&[u8]>) -> Option<(usize, usize, [u8; 4], u8, u32)> {
    let pos = cursor.position() as usize;
    let data = cursor.get_ref();

    if pos + 12 > data.len() {
        return None;
    }

    let size = u32::from_be_bytes(data[pos..pos + 4].try_into().ok()?) as usize;
    let box_type: [u8; 4] = data[pos + 4..pos + 8].try_into().ok()?;

    if size == 1 {
        if pos + 20 > data.len() {
            return None;
        }
        let ext_size = u64::from_be_bytes(data[pos + 8..pos + 16].try_into().ok()?) as usize;
        let vf = u32::from_be_bytes(data[pos + 16..pos + 20].try_into().ok()?);
        let version = (vf >> 24) as u8;
        let flags = vf & 0x00FFFFFF;
        cursor.set_position((pos + 20) as u64);
        Some((ext_size, 20, box_type, version, flags))
    } else if size == 0 {
        let remaining = data.len() - pos;
        let vf = u32::from_be_bytes(data[pos + 8..pos + 12].try_into().ok()?);
        let version = (vf >> 24) as u8;
        let flags = vf & 0x00FFFFFF;
        cursor.set_position((pos + 12) as u64);
        Some((remaining, 12, box_type, version, flags))
    } else {
        let vf = u32::from_be_bytes(data[pos + 8..pos + 12].try_into().ok()?);
        let version = (vf >> 24) as u8;
        let flags = vf & 0x00FFFFFF;
        cursor.set_position((pos + 12) as u64);
        Some((size, 12, box_type, version, flags))
    }
}

/// Write a box with the given type and data
pub fn write_box(output: &mut Vec<u8>, box_type: &[u8; 4], data: &[u8]) -> crate::Result<()> {
    let size = (8 + data.len()) as u32;
    output.write_all(&size.to_be_bytes())?;
    output.write_all(box_type)?;
    output.write_all(data)?;
    Ok(())
}

/// Write a full box with the given type, version, flags, and data
pub fn write_fullbox(output: &mut Vec<u8>, box_type: &[u8; 4], version: u8, flags: u32, data: &[u8]) -> crate::Result<()> {
    let vf = ((version as u32) << 24) | (flags & 0x00FFFFFF);
    let size = (12 + data.len()) as u32;
    output.write_all(&size.to_be_bytes())?;
    output.write_all(box_type)?;
    output.write_all(&vf.to_be_bytes())?;
    output.write_all(data)?;
    Ok(())
}
