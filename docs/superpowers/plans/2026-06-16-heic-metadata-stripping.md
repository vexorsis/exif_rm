# HEIC Metadata Stripping Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add HEIC image metadata stripping (EXIF, XMP, ICC) to exif_rm, with a shared ISOBMFF helper module extracted from video.rs.

**Architecture:** Extract `read_box_header` and `write_box` from `video.rs` into a new `isobmff.rs` shared module. Add a new `heic.rs` module that parses HEIC's item-based metadata model (iinf/iloc/iprp) and rebuilds boxes with metadata items excluded. Update format detection to distinguish HEIC from MP4 by inspecting ftyp brands.

**Tech Stack:** Rust, hand-rolled ISOBMFF parsing, no new dependencies

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `src/remove/isobmff.rs` | Create | Shared ISOBMFF box header reading/writing helpers |
| `src/remove/heic.rs` | Create | HeicRemover implementation with HEIC-specific box parsing/rebuilding |
| `src/remove/video.rs` | Modify | Remove extracted helpers, import from isobmff |
| `src/remove/mod.rs` | Modify | Add `heic` and `isobmff` module declarations |
| `src/traits.rs` | Modify | Add `Heic` variant to FileFormat enum |
| `src/format.rs` | Modify | Add HEIC format detection via ftyp brand inspection |
| `src/lib.rs` | Modify | Add Heic routing in get_remover |
| `Cargo.toml` | Modify | Add `heic` feature flag |
| `tests/integration.rs` | Modify | Add HEIC integration tests |

---

### Task 1: Add Heic feature flag and FileFormat variant

**Files:**
- Modify: `Cargo.toml:22-31`
- Modify: `src/traits.rs:1-15`
- Modify: `src/remove/mod.rs:1-16`

- [ ] **Step 1: Add `heic` feature to Cargo.toml**

Add `heic = []` to the `[features]` section and add `"heic"` to the `default` array.

```toml
default = ["jpeg", "png", "pdf", "office", "video", "webp", "mp3", "gif", "heic"]
# ... existing features ...
heic = []
```

- [ ] **Step 2: Add `Heic` variant to FileFormat enum**

In `src/traits.rs`, add `Heic` after `Gif`:

```rust
pub enum FileFormat {
    // ... existing variants ...
    Gif,
    Heic,
}
```

- [ ] **Step 3: Add `heic` module declaration to `src/remove/mod.rs`**

Add after the `gif` module:

```rust
#[cfg(feature = "heic")]
pub mod heic;
```

- [ ] **Step 4: Run `cargo check` to verify compilation**

Run: `cargo check`
Expected: compiles with warnings about unused `Heic` variant (expected, will be used later)

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml src/traits.rs src/remove/mod.rs
git commit -m "feat: add Heic feature flag and FileFormat variant"
```

---

### Task 2: Extract ISOBMFF helpers into shared module

**Files:**
- Create: `src/remove/isobmff.rs`
- Modify: `src/remove/video.rs:55-93`
- Modify: `src/remove/mod.rs`

- [ ] **Step 1: Create `src/remove/isobmff.rs` with extracted helpers**

Move `read_box_header` and `write_box` from `video.rs` into this new file, and add `read_fullbox_header`:

```rust
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

    let (total_size, header_size) = if size == 1 {
        if pos + 20 > data.len() {
            return None;
        }
        let ext_size = u64::from_be_bytes(data[pos + 8..pos + 16].try_into().ok()?) as usize;
        // version+flags at offset 16
        let vf = u32::from_be_bytes(data[pos + 16..pos + 20].try_into().ok()?);
        let version = (vf >> 24) as u8;
        let flags = vf & 0x00FFFFFF;
        cursor.set_position((pos + 20) as u64);
        return Some((ext_size, 20, box_type, version, flags));
    } else if size == 0 {
        let vf = u32::from_be_bytes(data[pos + 8..pos + 12].try_into().ok()?);
        let version = (vf >> 24) as u8;
        let flags = vf & 0x00FFFFFF;
        cursor.set_position((pos + 12) as u64);
        return Some((data.len() - pos, 12, box_type, version, flags));
    } else {
        let vf = u32::from_be_bytes(data[pos + 8..pos + 12].try_into().ok()?);
        let version = (vf >> 24) as u8;
        let flags = vf & 0x00FFFFFF;
        cursor.set_position((pos + 12) as u64);
        Some((size, 12, box_type, version, flags))
    };
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
```

- [ ] **Step 2: Add `isobmff` module declaration to `src/remove/mod.rs`**

Add at the top of the file (before format-specific modules), gated on either `video` or `heic`:

```rust
#[cfg(any(feature = "video", feature = "heic"))]
pub mod isobmff;
```

- [ ] **Step 3: Update `src/remove/video.rs` to use shared helpers**

Remove the local `read_box_header` and `write_box` functions (lines 57-93) and add the import:

```rust
use crate::remove::isobmff::{read_box_header, write_box};
```

The rest of `video.rs` stays unchanged.

- [ ] **Step 4: Run `cargo test` to verify no regression**

Run: `cargo test`
Expected: all existing tests pass (video tests still work with shared helpers)

- [ ] **Step 5: Commit**

```bash
git add src/remove/isobmff.rs src/remove/mod.rs src/remove/video.rs
git commit -m "refactor: extract ISOBMFF helpers into shared isobmff module"
```

---

### Task 3: Add HEIC format detection

**Files:**
- Modify: `src/format.rs:33-39`

- [ ] **Step 1: Update the ftyp detection block in `src/format.rs`**

Replace the existing MP4 ftyp detection block with brand-inspecting logic:

```rust
    // ISOBMFF container: box size (4 bytes) + "ftyp" (4 bytes)
    // Distinguish HEIC from MP4 by inspecting major/compatible brands
    if bytes.len() >= 8 && &bytes[4..8] == b"ftyp" {
        let ftyp_size = u32::from_be_bytes(bytes[0..4].try_into().unwrap()) as usize;
        if ftyp_size >= 12 && bytes.len() >= ftyp_size {
            let major_brand = &bytes[8..12];
            // compatible brands start at offset 16, each 4 bytes
            let has_heic = major_brand == b"heic"
                || bytes[16..ftyp_size].chunks_exact(4).any(|b| b == b"heic");

            if has_heic {
                #[cfg(feature = "heic")]
                return Ok(FileFormat::Heic);
                #[cfg(not(feature = "heic"))]
                return Err(Error::UnsupportedFormat);
            }
        }
        #[cfg(feature = "video")]
        return Ok(FileFormat::Mp4);
        #[cfg(not(feature = "video"))]
        return Err(Error::UnsupportedFormat);
    }
```

- [ ] **Step 2: Run `cargo test` to verify**

Run: `cargo test`
Expected: all existing tests pass (MP4 detection still works)

- [ ] **Step 3: Commit**

```bash
git add src/format.rs
git commit -m "feat: add HEIC format detection via ftyp brand inspection"
```

---

### Task 4: Implement HeicRemover — skeleton and passthrough

**Files:**
- Create: `src/remove/heic.rs`
- Modify: `src/lib.rs:5-53`

- [ ] **Step 1: Create `src/remove/heic.rs` with skeleton HeicRemover**

Write the initial `HeicRemover` struct with `MetadataRemover` trait impl. For now, just validate the ftyp header and pass through all boxes unchanged:

```rust
use crate::error::Error;
use crate::remove::isobmff::{read_box_header, write_box};
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
            && !input[16..ftyp_size].chunks_exact(4).any(|b| b == b"heic")
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
```

- [ ] **Step 2: Add Heic routing in `src/lib.rs`**

Add the `heic` feature to the `any()` gate on line 5:

```rust
#[cfg(any(feature = "jpeg", feature = "png", feature = "pdf", feature = "office", feature = "video", feature = "webp", feature = "mp3", feature = "gif", feature = "heic"))]
pub mod remove;
```

Add the `Heic` match arm in `get_remover` after the `Gif` arm:

```rust
        #[cfg(feature = "heic")]
        FileFormat::Heic => Box::new(remove::heic::HeicRemover),
```

- [ ] **Step 3: Write a failing test for passthrough**

Add a `#[cfg(test)]` section at the bottom of `src/remove/heic.rs`:

```rust
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

        // ftyp box: major_brand=heic
        let mut ftyp_content = b"heic".to_vec();
        ftyp_content.extend_from_slice(&0u32.to_be_bytes()); // minor_version
        heic.extend_from_slice(&make_box(b"ftyp", &ftyp_content));

        // meta box (fullbox) with minimal hdlr
        let mut hdlr_content = Vec::new();
        hdlr_content.extend_from_slice(&0u32.to_be_bytes()); // pre_defined
        hdlr_content.extend_from_slice(b"pict");             // handler_type
        hdlr_content.extend_from_slice(&[0u8; 12]);          // reserved
        hdlr_content.push(0);                                // name (null-terminated)
        let hdlr = make_fullbox(b"hdlr", 0, 0, &hdlr_content);

        let meta = make_fullbox(b"meta", 0, 0, &hdlr);
        heic.extend_from_slice(&meta);

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
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test --features heic -- heic`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/remove/heic.rs src/lib.rs
git commit -m "feat: add HeicRemover skeleton with passthrough"
```

---

### Task 5: Implement iinf parsing and EXIF item removal

**Files:**
- Modify: `src/remove/heic.rs`

- [ ] **Step 1: Add iinf/iloc parsing types and EXIF stripping logic**

Add these structs and the stripping implementation to `heic.rs`. The `remove_metadata` method now parses `meta` box contents to find and remove EXIF items:

Add these types after the imports:

```rust
struct ItemInfo {
    item_id: u16,
    item_type: [u8; 4],
    content_type: Option<String>,
}
```

Replace the entire `remove_metadata` method with:

```rust
    fn remove_metadata(&self, input: &[u8], options: &RemovalOptions) -> crate::Result<Vec<u8>> {
        if input.len() < 12 || &input[4..8] != b"ftyp" {
            return Err(Error::InvalidData("HEIC".into()));
        }

        let ftyp_size = u32::from_be_bytes(input[0..4].try_into().unwrap()) as usize;
        let major_brand = &input[8..12];
        if major_brand != b"heic"
            && !input[16..ftyp_size].chunks_exact(4).any(|b| b == b"heic")
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
                    let meta_data = &input[box_start + header_size..box_end];
                    let cleaned_meta = process_meta_box(meta_data, options)?;
                    if !cleaned_meta.is_empty() {
                        output.extend_from_slice(&input[box_start..box_start + header_size]);
                        output.extend_from_slice(&cleaned_meta);
                    }
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
```

Add the helper functions after the impl block:

```rust
fn process_meta_box(meta_data: &[u8], options: &RemovalOptions) -> crate::Result<Vec<u8>> {
    // meta is a fullbox: skip version+flags (4 bytes)
    if meta_data.len() < 4 {
        return Ok(meta_data.to_vec());
    }
    let version_flags = &meta_data[0..4];
    let inner_data = &meta_data[4..];

    let mut cursor = Cursor::new(inner_data);
    let mut removed_ids: Vec<u16> = Vec::new();

    // First pass: parse iinf to find metadata item IDs
    while let Some((total_size, header_size, box_type)) = read_box_header(&mut cursor) {
        let data_start = cursor.position() as usize;
        let box_end = data_start + total_size - header_size;

        if box_end > inner_data.len() {
            break;
        }

        if &box_type == b"iinf" {
            let iinf_data = &inner_data[data_start..box_end];
            removed_ids = find_metadata_item_ids(iinf_data, options);
        }

        cursor.set_position(box_end as u64);
    }

    // If nothing to remove, return as-is
    if removed_ids.is_empty() {
        return Ok(meta_data.to_vec());
    }

    // Second pass: rebuild meta box contents, filtering iinf and iloc
    let mut result = version_flags.to_vec();
    cursor.set_position(0);

    while let Some((total_size, header_size, box_type)) = read_box_header(&mut cursor) {
        let box_start = cursor.position() as usize - header_size;
        let data_start = cursor.position() as usize;
        let box_end = box_start + total_size;

        if box_end > inner_data.len() {
            break;
        }

        match &box_type {
            b"iinf" => {
                let iinf_data = &inner_data[data_start..box_end];
                let cleaned = rebuild_iinf(iinf_data, &removed_ids)?;
                result.extend_from_slice(&inner_data[box_start..box_start + header_size]);
                result.extend_from_slice(&cleaned);
            }
            b"iloc" => {
                let iloc_data = &inner_data[data_start..box_end];
                let cleaned = rebuild_iloc(iloc_data, &removed_ids)?;
                result.extend_from_slice(&inner_data[box_start..box_start + header_size]);
                result.extend_from_slice(&cleaned);
            }
            _ => {
                result.extend_from_slice(&inner_data[box_start..box_end]);
            }
        }

        cursor.set_position(box_end as u64);
    }

    Ok(result)
}

fn find_metadata_item_ids(iinf_data: &[u8], options: &RemovalOptions) -> Vec<u16> {
    let mut ids = Vec::new();

    // iinf is a fullbox: version(1) + flags(3) + entry_count(2 for v0, 4 for v1+)
    if iinf_data.len() < 6 {
        return ids;
    }
    let version = iinf_data[0];
    let entry_count_size = if version == 0 { 2usize } else { 4 };
    if iinf_data.len() < 4 + entry_count_size {
        return ids;
    }
    let entry_count = if version == 0 {
        u16::from_be_bytes(iinf_data[4..6].try_into().unwrap()) as usize
    } else {
        u32::from_be_bytes(iinf_data[4..8].try_into().unwrap()) as usize
    };

    let mut pos = 4 + entry_count_size;
    for _ in 0..entry_count {
        // Each entry is an "infe" fullbox
        if pos + 8 > iinf_data.len() {
            break;
        }
        let infe_size = u32::from_be_bytes(iinf_data[pos..pos + 4].try_into().unwrap()) as usize;
        let infe_type = &iinf_data[pos + 4..pos + 8];
        if infe_type != b"infe" || pos + infe_size > iinf_data.len() {
            pos += infe_size;
            continue;
        }

        // infe is a fullbox: version(1) + flags(3)
        if pos + 12 > iinf_data.len() {
            break;
        }
        let infe_version = iinf_data[pos + 8];
        let item_id_size = if infe_version < 2 { 2usize } else { 4 };
        let item_type_offset = pos + 12 + item_id_size;
        if infe_version >= 2 {
            // v2: item_id(4) + item_type(4)
            if item_type_offset + 4 > pos + infe_size {
                pos += infe_size;
                continue;
            }
            let item_id = u32::from_be_bytes(iinf_data[pos + 12..pos + 16].try_into().unwrap()) as u16;
            let item_type: [u8; 4] = iinf_data[item_type_offset..item_type_offset + 4].try_into().unwrap();

            let should_remove = (options.exif && item_type == *b"Exif")
                || (options.xmp && item_type == *b"mime");

            if should_remove {
                ids.push(item_id);
            }
        } else {
            // v0/v1: item_id(2) + item_protection_index(2) + item_type(4) [+ item_name null-terminated]
            if pos + 20 > pos + infe_size {
                pos += infe_size;
                continue;
            }
            let item_id = u16::from_be_bytes(iinf_data[pos + 12..pos + 14].try_into().unwrap());
            let item_type: [u8; 4] = iinf_data[pos + 16..pos + 20].try_into().unwrap();

            let should_remove = (options.exif && item_type == *b"Exif")
                || (options.xmp && item_type == *b"mime");

            if should_remove {
                ids.push(item_id);
            }
        }

        pos += infe_size;
    }

    ids
}

fn rebuild_iinf(iinf_data: &[u8], removed_ids: &[u16]) -> crate::Result<Vec<u8>> {
    if iinf_data.len() < 6 {
        return Ok(iinf_data.to_vec());
    }
    let version = iinf_data[0];
    let entry_count_size = if version == 0 { 2usize } else { 4 };
    let entry_count = if version == 0 {
        u16::from_be_bytes(iinf_data[4..6].try_into().unwrap()) as usize
    } else {
        u32::from_be_bytes(iinf_data[4..8].try_into().unwrap()) as usize
    };

    let mut result = Vec::with_capacity(iinf_data.len());
    result.extend_from_slice(&iinf_data[0..4]); // version + flags
    // placeholder for entry_count
    let count_offset = result.len();
    if version == 0 {
        result.extend_from_slice(&0u16.to_be_bytes());
    } else {
        result.extend_from_slice(&0u32.to_be_bytes());
    }

    let mut pos = 4 + entry_count_size;
    let mut new_count: u32 = 0;

    for _ in 0..entry_count {
        if pos + 8 > iinf_data.len() {
            break;
        }
        let infe_size = u32::from_be_bytes(iinf_data[pos..pos + 4].try_into().unwrap()) as usize;
        if pos + infe_size > iinf_data.len() {
            break;
        }

        // Check if this item should be removed
        let should_remove = is_infe_removed(&iinf_data[pos..pos + infe_size], removed_ids);

        if !should_remove {
            result.extend_from_slice(&iinf_data[pos..pos + infe_size]);
            new_count += 1;
        }

        pos += infe_size;
    }

    // Update entry count
    if version == 0 {
        result[count_offset..count_offset + 2].copy_from_slice(&(new_count as u16).to_be_bytes());
    } else {
        result[count_offset..count_offset + 4].copy_from_slice(&new_count.to_be_bytes());
    }

    Ok(result)
}

fn is_infe_removed(infe_data: &[u8], removed_ids: &[u16]) -> bool {
    if infe_data.len() < 12 {
        return false;
    }
    let version = infe_data[8];
    if version >= 2 {
        if infe_data.len() < 16 {
            return false;
        }
        let item_id = u32::from_be_bytes(infe_data[12..16].try_into().unwrap()) as u16;
        removed_ids.contains(&item_id)
    } else {
        if infe_data.len() < 14 {
            return false;
        }
        let item_id = u16::from_be_bytes(infe_data[12..14].try_into().unwrap());
        removed_ids.contains(&item_id)
    }
}

fn rebuild_iloc(iloc_data: &[u8], removed_ids: &[u16]) -> crate::Result<Vec<u8>> {
    // iloc is a fullbox: version(1) + flags(3) + offset_size(4bits) + length_size(4bits) + ...
    if iloc_data.len() < 8 {
        return Ok(iloc_data.to_vec());
    }
    let version = iloc_data[0];
    if version > 1 {
        return Err(Error::InvalidData("HEIC: unsupported iloc version".into()));
    }

    let offset_size = (iloc_data[4] >> 4) as usize;
    let length_size = (iloc_data[4] & 0x0F) as usize;
    let base_offset_size = (iloc_data[5] >> 4) as usize;
    let index_size = if version == 1 { (iloc_data[5] & 0x0F) as usize } else { 0 };

    let item_count_size = if version < 2 { 2usize } else { 4 };
    if iloc_data.len() < 6 + item_count_size {
        return Ok(iloc_data.to_vec());
    }

    let item_count = if version < 2 {
        u16::from_be_bytes(iloc_data[6..8].try_into().unwrap()) as usize
    } else {
        u32::from_be_bytes(iloc_data[6..10].try_into().unwrap()) as usize
    };

    let mut result = Vec::with_capacity(iloc_data.len());
    // Copy header through item_count
    let header_end = 6 + item_count_size;
    result.extend_from_slice(&iloc_data[0..header_end]);

    // Placeholder for new item_count
    let count_offset = 6;

    let mut pos = header_end;
    let mut new_count: u32 = 0;

    for _ in 0..item_count {
        let item_start = pos;
        if pos + 2 > iloc_data.len() {
            break;
        }
        let item_id = u16::from_be_bytes(iloc_data[pos..pos + 2].try_into().unwrap());
        pos += 2;

        if version == 1 {
            // construction_method: 4 bits reserved + 4 bits construction_method
            if pos + 2 > iloc_data.len() {
                break;
            }
            pos += 2; // construction_method
        }

        // data_reference_index (16-bit)
        if pos + 2 > iloc_data.len() {
            break;
        }
        pos += 2;

        // base_offset
        pos += base_offset_size;

        let extent_count = if pos + 2 > iloc_data.len() {
            break;
        } else {
            u16::from_be_bytes(iloc_data[pos..pos + 2].try_into().unwrap())
        };
        pos += 2;

        // extent_count is already read; now we know total item size
        // Calculate total item bytes to skip/read extents
        let extent_size = index_size + offset_size + length_size;
        let extents_total = extent_count as usize * extent_size;
        let item_end = pos + extents_total;

        if item_end > iloc_data.len() {
            break;
        }

        if removed_ids.contains(&item_id) {
            // Skip this item entirely
            pos = item_end;
        } else {
            // Keep this item — copy from item_start to item_end
            result.extend_from_slice(&iloc_data[item_start..item_end]);
            new_count += 1;
            pos = item_end;
        }
    }

    // Update item_count in result
    if version < 2 {
        result[count_offset..count_offset + 2].copy_from_slice(&(new_count as u16).to_be_bytes());
    } else {
        result[count_offset..count_offset + 4].copy_from_slice(&new_count.to_be_bytes());
    }

    Ok(result)
}
```

- [ ] **Step 2: Add EXIF stripping test to the test module in `heic.rs`**

Add this test helper and test case inside the `mod tests` block:

```rust
    fn create_heic_with_exif() -> Vec<u8> {
        let mut heic = Vec::new();

        // ftyp box
        let mut ftyp_content = b"heic".to_vec();
        ftyp_content.extend_from_slice(&0u32.to_be_bytes());
        heic.extend_from_slice(&make_box(b"ftyp", &ftyp_content));

        // Build meta box contents
        // hdlr
        let mut hdlr_content = Vec::new();
        hdlr_content.extend_from_slice(&0u32.to_be_bytes());
        hdlr_content.extend_from_slice(b"pict");
        hdlr_content.extend_from_slice(&[0u8; 12]);
        hdlr_content.push(0);
        let hdlr = make_fullbox(b"hdlr", 0, 0, &hdlr_content);

        // iinf with 2 items: Exif (id=1) and hvc1 (id=2)
        let mut iinf_entries = Vec::new();
        // infe for Exif item (v0): item_id(2) + protection_index(2) + item_type(4) + name(null)
        let mut infe1 = Vec::new();
        infe1.extend_from_slice(&1u16.to_be_bytes()); // item_id
        infe1.extend_from_slice(&0u16.to_be_bytes()); // protection_index
        infe1.extend_from_slice(b"Exif");              // item_type
        infe1.push(0);                                 // name
        iinf_entries.extend_from_slice(&make_fullbox(b"infe", 0, 0, &infe1));

        // infe for hvc1 image item (v0)
        let mut infe2 = Vec::new();
        infe2.extend_from_slice(&2u16.to_be_bytes());
        infe2.extend_from_slice(&0u16.to_be_bytes());
        infe2.extend_from_slice(b"hvc1");
        infe2.push(0);
        iinf_entries.extend_from_slice(&make_fullbox(b"infe", 0, 0, &infe2));

        // iinf fullbox: version=0, flags=0, entry_count=2
        let mut iinf_content = Vec::new();
        iinf_content.extend_from_slice(&2u16.to_be_bytes()); // entry_count
        iinf_content.extend_from_slice(&iinf_entries);
        let iinf = make_fullbox(b"iinf", 0, 0, &iinf_content);

        // iloc with 2 items (version=0, offset_size=4, length_size=4, base_offset_size=0)
        let mut iloc_content = Vec::new();
        iloc_content.push(0x44); // offset_size=4, length_size=4
        iloc_content.push(0x00); // base_offset_size=0, index_size=0 (v0 has no index_size)
        iloc_content.extend_from_slice(&2u16.to_be_bytes()); // item_count=2
        // Item 1 (Exif): item_id=1, data_ref_index=0, extent_count=1, offset=100, length=10
        iloc_content.extend_from_slice(&1u16.to_be_bytes()); // item_id
        iloc_content.extend_from_slice(&0u16.to_be_bytes()); // data_reference_index
        iloc_content.extend_from_slice(&1u16.to_be_bytes()); // extent_count
        iloc_content.extend_from_slice(&100u32.to_be_bytes()); // extent_offset
        iloc_content.extend_from_slice(&10u32.to_be_bytes());  // extent_length
        // Item 2 (hvc1): item_id=2, data_ref_index=0, extent_count=1, offset=110, length=20
        iloc_content.extend_from_slice(&2u16.to_be_bytes());
        iloc_content.extend_from_slice(&0u16.to_be_bytes());
        iloc_content.extend_from_slice(&1u16.to_be_bytes());
        iloc_content.extend_from_slice(&110u32.to_be_bytes());
        iloc_content.extend_from_slice(&20u32.to_be_bytes());
        let iloc = make_fullbox(b"iloc", 0, 0, &iloc_content);

        // Assemble meta
        let mut meta_inner = Vec::new();
        meta_inner.extend_from_slice(&hdlr);
        meta_inner.extend_from_slice(&iinf);
        meta_inner.extend_from_slice(&iloc);
        let meta = make_fullbox(b"meta", 0, 0, &meta_inner);
        heic.extend_from_slice(&meta);

        // mdat box
        heic.extend_from_slice(&make_box(b"mdat", b"fake image data"));

        heic
    }

    #[test]
    fn test_heic_strip_exif() {
        let input = create_heic_with_exif();
        let output = HeicRemover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        // Output should not contain "Exif" item type in iinf
        // The ftyp and mdat should still be present
        assert_eq!(&output[4..8], b"ftyp");
        assert!(output.windows(4).any(|w| w == b"mdat"));
        // The output should be smaller (Exif item removed)
        assert!(output.len() < input.len(), "output should be smaller after stripping EXIF");
    }
```

- [ ] **Step 3: Run tests to verify**

Run: `cargo test --features heic -- heic`
Expected: both passthrough and strip_exif tests pass

- [ ] **Step 4: Commit**

```bash
git add src/remove/heic.rs
git commit -m "feat: implement iinf/iloc parsing and EXIF item removal for HEIC"
```

---

### Task 6: Implement XMP item removal

**Files:**
- Modify: `src/remove/heic.rs`

The `find_metadata_item_ids` function already handles XMP (it checks for `item_type == b"mime"` when `options.xmp` is true). However, for XMP we need to verify the content_type is `application/rdf+xml`. For simplicity and correctness at this stage, we'll treat all `mime` items as XMP — this is the common case in real HEIC files.

- [ ] **Step 1: Add XMP stripping test**

Add to the test module in `heic.rs`:

```rust
    fn create_heic_with_xmp() -> Vec<u8> {
        let mut heic = Vec::new();

        // ftyp box
        let mut ftyp_content = b"heic".to_vec();
        ftyp_content.extend_from_slice(&0u32.to_be_bytes());
        heic.extend_from_slice(&make_box(b"ftyp", &ftyp_content));

        // hdlr
        let mut hdlr_content = Vec::new();
        hdlr_content.extend_from_slice(&0u32.to_be_bytes());
        hdlr_content.extend_from_slice(b"pict");
        hdlr_content.extend_from_slice(&[0u8; 12]);
        hdlr_content.push(0);
        let hdlr = make_fullbox(b"hdlr", 0, 0, &hdlr_content);

        // iinf with 2 items: mime/XMP (id=1) and hvc1 (id=2)
        let mut iinf_entries = Vec::new();
        let mut infe1 = Vec::new();
        infe1.extend_from_slice(&1u16.to_be_bytes());
        infe1.extend_from_slice(&0u16.to_be_bytes());
        infe1.extend_from_slice(b"mime");
        infe1.push(0);
        iinf_entries.extend_from_slice(&make_fullbox(b"infe", 0, 0, &infe1));

        let mut infe2 = Vec::new();
        infe2.extend_from_slice(&2u16.to_be_bytes());
        infe2.extend_from_slice(&0u16.to_be_bytes());
        infe2.extend_from_slice(b"hvc1");
        infe2.push(0);
        iinf_entries.extend_from_slice(&make_fullbox(b"infe", 0, 0, &infe2));

        let mut iinf_content = Vec::new();
        iinf_content.extend_from_slice(&2u16.to_be_bytes());
        iinf_content.extend_from_slice(&iinf_entries);
        let iinf = make_fullbox(b"iinf", 0, 0, &iinf_content);

        // iloc
        let mut iloc_content = Vec::new();
        iloc_content.push(0x44);
        iloc_content.push(0x00);
        iloc_content.extend_from_slice(&2u16.to_be_bytes());
        iloc_content.extend_from_slice(&1u16.to_be_bytes());
        iloc_content.extend_from_slice(&0u16.to_be_bytes());
        iloc_content.extend_from_slice(&1u16.to_be_bytes());
        iloc_content.extend_from_slice(&100u32.to_be_bytes());
        iloc_content.extend_from_slice(&10u32.to_be_bytes());
        iloc_content.extend_from_slice(&2u16.to_be_bytes());
        iloc_content.extend_from_slice(&0u16.to_be_bytes());
        iloc_content.extend_from_slice(&1u16.to_be_bytes());
        iloc_content.extend_from_slice(&110u32.to_be_bytes());
        iloc_content.extend_from_slice(&20u32.to_be_bytes());
        let iloc = make_fullbox(b"iloc", 0, 0, &iloc_content);

        let mut meta_inner = Vec::new();
        meta_inner.extend_from_slice(&hdlr);
        meta_inner.extend_from_slice(&iinf);
        meta_inner.extend_from_slice(&iloc);
        let meta = make_fullbox(b"meta", 0, 0, &meta_inner);
        heic.extend_from_slice(&meta);

        heic.extend_from_slice(&make_box(b"mdat", b"fake image data"));

        heic
    }

    #[test]
    fn test_heic_strip_xmp() {
        let input = create_heic_with_xmp();
        let output = HeicRemover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        assert_eq!(&output[4..8], b"ftyp");
        assert!(output.windows(4).any(|w| w == b"mdat"));
        assert!(output.len() < input.len(), "output should be smaller after stripping XMP");
    }
```

- [ ] **Step 2: Run tests to verify**

Run: `cargo test --features heic -- heic`
Expected: all tests pass (XMP removal already handled by `find_metadata_item_ids`)

- [ ] **Step 3: Commit**

```bash
git add src/remove/heic.rs
git commit -m "test: add XMP item removal test for HEIC"
```

---

### Task 7: Implement ICC profile removal via iprp/ipco/ipma

**Files:**
- Modify: `src/remove/heic.rs`

- [ ] **Step 1: Add ICC stripping logic to `process_meta_box`**

The `process_meta_box` function needs to handle `iprp` boxes when `options.icc_profile` is true. Add an `iprp` match arm in the second-pass rebuild loop:

In the second pass `match &box_type` block inside `process_meta_box`, add this arm before the `_` catch-all:

```rust
            b"iprp" => {
                if options.icc_profile {
                    let iprp_data = &inner_data[data_start..box_end];
                    let cleaned = process_iprp(iprp_data)?;
                    result.extend_from_slice(&inner_data[box_start..box_start + header_size]);
                    result.extend_from_slice(&cleaned);
                } else {
                    result.extend_from_slice(&inner_data[box_start..box_end]);
                }
            }
```

Add the `process_iprp` function:

```rust
fn process_iprp(iprp_data: &[u8]) -> crate::Result<Vec<u8>> {
    let mut cursor = Cursor::new(iprp_data);
    let mut ipco_index: Option<usize> = None; // index of colr box within ipco
    let mut colr_property_index: u8 = 0; // 1-based property index

    // First pass: find colr box index in ipco
    let mut prop_index: u8 = 1;
    while let Some((total_size, header_size, box_type)) = read_box_header(&mut cursor) {
        let data_start = cursor.position() as usize;
        let box_end = data_start + total_size - header_size;

        if box_end > iprp_data.len() {
            break;
        }

        if &box_type == b"ipco" {
            // Walk ipco children to find colr
            let ipco_data = &iprp_data[data_start..box_end];
            let mut ipco_cursor = Cursor::new(ipco_data);
            let mut idx: u8 = 1;
            while let Some((ipco_total, ipco_header, ipco_type)) = read_box_header(&mut ipco_cursor) {
                if &ipco_type == b"colr" {
                    ipco_index = Some(idx);
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

    let colr_idx = match ipco_index {
        Some(i) => i,
        None => return Ok(iprp_data.to_vec()), // no colr found, return as-is
    };

    // Second pass: rebuild iprp, removing colr from ipco and its association from ipma
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
                result.extend_from_slice(&iprp_data[box_start..box_start + header_size]);
                result.extend_from_slice(&cleaned);
            }
            b"ipma" => {
                let ipma_data = &iprp_data[data_start..box_end];
                let cleaned = rebuild_ipma(ipma_data, colr_idx)?;
                result.extend_from_slice(&iprp_data[box_start..box_start + header_size]);
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

    while let Some((total_size, header_size, box_type)) = read_box_header(&mut cursor) {
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
    // ipma is a fullbox: version(1) + flags(3) + entry_count(4)
    if ipma_data.len() < 8 {
        return Ok(ipma_data.to_vec());
    }
    let version = ipma_data[0];
    let flags = u32::from_be_bytes([0, ipma_data[1], ipma_data[2], ipma_data[3]]);
    let entry_count = u32::from_be_bytes(ipma_data[4..8].try_into().unwrap()) as usize;

    let mut result = Vec::with_capacity(ipma_data.len());
    // version + flags
    let vf = ((version as u32) << 24) | flags;
    result.extend_from_slice(&vf.to_be_bytes());
    // placeholder for entry_count
    result.extend_from_slice(&0u32.to_be_bytes());

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
            // Each association: 1 byte (essential + property_index bits) or 2 bytes
            let assoc_size = if version < 1 { 1 } else { 2 };
            if pos + assoc_size > ipma_data.len() {
                break;
            }
            let property_index = if version < 1 {
                ipma_data[pos] & 0x7F
            } else {
                u16::from_be_bytes([ipma_data[pos], ipma_data[pos + 1]]) & 0x7FFF
            };

            if property_index != colr_index as u16 {
                result.extend_from_slice(&ipma_data[pos..pos + assoc_size]);
            }
            pos += assoc_size;
        }
    }

    // We don't update entry_count since we keep the same number of items
    // (we only remove associations, not items)
    // But we need to update the count in case items were fully stripped
    // For simplicity, keep the original entry_count since items remain
    result[4..8].copy_from_slice(&entry_count.to_be_bytes());

    Ok(result)
}
```

- [ ] **Step 2: Add ICC stripping tests**

Add to the test module in `heic.rs`:

```rust
    fn create_heic_with_icc() -> Vec<u8> {
        let mut heic = Vec::new();

        // ftyp box
        let mut ftyp_content = b"heic".to_vec();
        ftyp_content.extend_from_slice(&0u32.to_be_bytes());
        heic.extend_from_slice(&make_box(b"ftyp", &ftyp_content));

        // hdlr
        let mut hdlr_content = Vec::new();
        hdlr_content.extend_from_slice(&0u32.to_be_bytes());
        hdlr_content.extend_from_slice(b"pict");
        hdlr_content.extend_from_slice(&[0u8; 12]);
        hdlr_content.push(0);
        let hdlr = make_fullbox(b"hdlr", 0, 0, &hdlr_content);

        // iinf with 1 item: hvc1 (id=1)
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

        // iloc with 1 item
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

        // iprp with ipco (containing ispe + colr) and ipma
        let ispe = make_box(b"ispe", &[
            0x00, 0x00, 0x00, 0x01,  // width
            0x00, 0x00, 0x00, 0x01,  // height
        ]);
        let colr = make_box(b"colr", b"nclx\x01\x01\x01\x00");
        let mut ipco_content = Vec::new();
        ipco_content.extend_from_slice(&ispe);
        ipco_content.extend_from_slice(&colr);
        let ipco = make_box(b"ipco", &ipco_content);

        // ipma (v0): entry_count=1, item_id=1, association_count=2, [ess+idx=ispe(1), ess+idx=colr(2)]
        let mut ipma_content = Vec::new();
        let vf = 0u32.to_be_bytes(); // version=0, flags=0
        ipma_content.extend_from_slice(&vf);
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

        // Assemble meta
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
        // colr should be removed from ipco
        assert!(!output.windows(4).any(|w| w == b"colr"), "colr should be removed when icc_profile option is set");
        // ispe should be preserved
        assert!(output.windows(4).any(|w| w == b"ispe"), "ispe should be preserved");
        // ftyp and mdat should still be present
        assert_eq!(&output[4..8], b"ftyp");
        assert!(output.windows(4).any(|w| w == b"mdat"));
    }

    #[test]
    fn test_heic_keep_icc_by_default() {
        let input = create_heic_with_icc();
        let output = HeicRemover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        // colr should be preserved with default options
        assert!(output.windows(4).any(|w| w == b"colr"), "colr should be preserved by default");
    }
```

- [ ] **Step 3: Run tests to verify**

Run: `cargo test --features heic -- heic`
Expected: all HEIC tests pass including strip_icc and keep_icc

- [ ] **Step 4: Commit**

```bash
git add src/remove/heic.rs
git commit -m "feat: implement ICC profile removal for HEIC via iprp/ipco/ipma"
```

---

### Task 8: Add error handling tests

**Files:**
- Modify: `src/remove/heic.rs`

- [ ] **Step 1: Add error case tests**

Add to the test module in `heic.rs`:

```rust
    #[test]
    fn test_heic_invalid_header() {
        let input = b"not a heic file at all".to_vec();
        let result = HeicRemover.remove_metadata(&input, &RemovalOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_heic_missing_meta_box() {
        let mut heic = Vec::new();
        // ftyp box with heic brand
        let mut ftyp_content = b"heic".to_vec();
        ftyp_content.extend_from_slice(&0u32.to_be_bytes());
        heic.extend_from_slice(&make_box(b"ftyp", &ftyp_content));
        // mdat but no meta
        heic.extend_from_slice(&make_box(b"mdat", b"fake image data"));

        let result = HeicRemover.remove_metadata(&heic, &RemovalOptions::default());
        assert!(result.is_err(), "HEIC without meta box should error");
    }

    #[test]
    fn test_heic_truncated_data() {
        // Valid ftyp header but truncated after that
        let mut heic = Vec::new();
        let mut ftyp_content = b"heic".to_vec();
        ftyp_content.extend_from_slice(&0u32.to_be_bytes());
        heic.extend_from_slice(&make_box(b"ftyp", &ftyp_content));
        // Truncated meta box (declares large size but has no data)
        heic.extend_from_slice(&50u32.to_be_bytes());
        heic.extend_from_slice(b"meta");

        let result = HeicRemover.remove_metadata(&heic, &RemovalOptions::default());
        assert!(result.is_err(), "truncated HEIC should error");
    }
```

- [ ] **Step 2: Run tests to verify**

Run: `cargo test --features heic -- heic`
Expected: all tests pass including error cases

- [ ] **Step 3: Commit**

```bash
git add src/remove/heic.rs
git commit -m "test: add HEIC error handling tests"
```

---

### Task 9: Add integration tests

**Files:**
- Modify: `tests/integration.rs`

- [ ] **Step 1: Add HEIC integration test helpers and tests**

Add at the end of `tests/integration.rs`:

```rust
// --- HEIC tests ---

fn create_minimal_heic() -> Vec<u8> {
    let mut heic = Vec::new();

    let make_box = |box_type: &[u8], content: &[u8]| -> Vec<u8> {
        let size = (8 + content.len()) as u32;
        let mut buf = size.to_be_bytes().to_vec();
        buf.extend_from_slice(box_type);
        buf.extend_from_slice(content);
        buf
    };

    let make_fullbox = |box_type: &[u8], version: u8, flags: u32, content: &[u8]| -> Vec<u8> {
        let vf = ((version as u32) << 24) | flags;
        let mut full_content = vf.to_be_bytes().to_vec();
        full_content.extend_from_slice(content);
        make_box(box_type, &full_content)
    };

    // ftyp box
    let mut ftyp_content = b"heic".to_vec();
    ftyp_content.extend_from_slice(&0u32.to_be_bytes());
    heic.extend_from_slice(&make_box(b"ftyp", &ftyp_content));

    // hdlr
    let mut hdlr_content = Vec::new();
    hdlr_content.extend_from_slice(&0u32.to_be_bytes());
    hdlr_content.extend_from_slice(b"pict");
    hdlr_content.extend_from_slice(&[0u8; 12]);
    hdlr_content.push(0);
    let hdlr = make_fullbox(b"hdlr", 0, 0, &hdlr_content);

    // iinf with 1 item: hvc1 (id=1)
    let mut infe1 = Vec::new();
    infe1.extend_from_slice(&1u16.to_be_bytes());
    infe1.extend_from_slice(&0u16.to_be_bytes());
    infe1.extend_from_slice(b"hvc1");
    infe1.push(0);
    let iinf_entries = make_fullbox(b"infe", 0, 0, &infe1);
    let mut iinf_content = Vec::new();
    iinf_content.extend_from_slice(&1u16.to_be_bytes());
    iinf_content.extend_from_slice(&iinf_entries);
    let iinf = make_fullbox(b"iinf", 0, 0, &iinf_content);

    // iloc with 1 item
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

    let mut meta_inner = Vec::new();
    meta_inner.extend_from_slice(&hdlr);
    meta_inner.extend_from_slice(&iinf);
    meta_inner.extend_from_slice(&iloc);
    let meta = make_fullbox(b"meta", 0, 0, &meta_inner);
    heic.extend_from_slice(&meta);

    heic.extend_from_slice(&make_box(b"mdat", b"fake image data"));

    heic
}

fn create_heic_with_metadata() -> Vec<u8> {
    let mut heic = Vec::new();

    let make_box = |box_type: &[u8], content: &[u8]| -> Vec<u8> {
        let size = (8 + content.len()) as u32;
        let mut buf = size.to_be_bytes().to_vec();
        buf.extend_from_slice(box_type);
        buf.extend_from_slice(content);
        buf
    };

    let make_fullbox = |box_type: &[u8], version: u8, flags: u32, content: &[u8]| -> Vec<u8> {
        let vf = ((version as u32) << 24) | flags;
        let mut full_content = vf.to_be_bytes().to_vec();
        full_content.extend_from_slice(content);
        make_box(box_type, &full_content)
    };

    // ftyp box
    let mut ftyp_content = b"heic".to_vec();
    ftyp_content.extend_from_slice(&0u32.to_be_bytes());
    heic.extend_from_slice(&make_box(b"ftyp", &ftyp_content));

    // hdlr
    let mut hdlr_content = Vec::new();
    hdlr_content.extend_from_slice(&0u32.to_be_bytes());
    hdlr_content.extend_from_slice(b"pict");
    hdlr_content.extend_from_slice(&[0u8; 12]);
    hdlr_content.push(0);
    let hdlr = make_fullbox(b"hdlr", 0, 0, &hdlr_content);

    // iinf with 3 items: Exif (id=1), mime/XMP (id=2), hvc1 (id=3)
    let mut iinf_entries = Vec::new();
    let mut infe1 = Vec::new();
    infe1.extend_from_slice(&1u16.to_be_bytes());
    infe1.extend_from_slice(&0u16.to_be_bytes());
    infe1.extend_from_slice(b"Exif");
    infe1.push(0);
    iinf_entries.extend_from_slice(&make_fullbox(b"infe", 0, 0, &infe1));

    let mut infe2 = Vec::new();
    infe2.extend_from_slice(&2u16.to_be_bytes());
    infe2.extend_from_slice(&0u16.to_be_bytes());
    infe2.extend_from_slice(b"mime");
    infe2.push(0);
    iinf_entries.extend_from_slice(&make_fullbox(b"infe", 0, 0, &infe2));

    let mut infe3 = Vec::new();
    infe3.extend_from_slice(&3u16.to_be_bytes());
    infe3.extend_from_slice(&0u16.to_be_bytes());
    infe3.extend_from_slice(b"hvc1");
    infe3.push(0);
    iinf_entries.extend_from_slice(&make_fullbox(b"infe", 0, 0, &infe3));

    let mut iinf_content = Vec::new();
    iinf_content.extend_from_slice(&3u16.to_be_bytes());
    iinf_content.extend_from_slice(&iinf_entries);
    let iinf = make_fullbox(b"iinf", 0, 0, &iinf_content);

    // iloc with 3 items
    let mut iloc_content = Vec::new();
    iloc_content.push(0x44);
    iloc_content.push(0x00);
    iloc_content.extend_from_slice(&3u16.to_be_bytes());
    // Exif item
    iloc_content.extend_from_slice(&1u16.to_be_bytes());
    iloc_content.extend_from_slice(&0u16.to_be_bytes());
    iloc_content.extend_from_slice(&1u16.to_be_bytes());
    iloc_content.extend_from_slice(&100u32.to_be_bytes());
    iloc_content.extend_from_slice(&10u32.to_be_bytes());
    // XMP item
    iloc_content.extend_from_slice(&2u16.to_be_bytes());
    iloc_content.extend_from_slice(&0u16.to_be_bytes());
    iloc_content.extend_from_slice(&1u16.to_be_bytes());
    iloc_content.extend_from_slice(&200u32.to_be_bytes());
    iloc_content.extend_from_slice(&15u32.to_be_bytes());
    // hvc1 item
    iloc_content.extend_from_slice(&3u16.to_be_bytes());
    iloc_content.extend_from_slice(&0u16.to_be_bytes());
    iloc_content.extend_from_slice(&1u16.to_be_bytes());
    iloc_content.extend_from_slice(&300u32.to_be_bytes());
    iloc_content.extend_from_slice(&20u32.to_be_bytes());
    let iloc = make_fullbox(b"iloc", 0, 0, &iloc_content);

    let mut meta_inner = Vec::new();
    meta_inner.extend_from_slice(&hdlr);
    meta_inner.extend_from_slice(&iinf);
    meta_inner.extend_from_slice(&iloc);
    let meta = make_fullbox(b"meta", 0, 0, &meta_inner);
    heic.extend_from_slice(&meta);

    heic.extend_from_slice(&make_box(b"mdat", b"fake image data with EXIF and XMP"));

    heic
}

#[test]
fn test_heic_format_detection() {
    let input = create_minimal_heic();
    let format = detect_format(&input).unwrap();
    assert_eq!(format, FileFormat::Heic);
}

#[test]
fn test_heic_strip_removes_metadata() {
    let input = create_heic_with_metadata();
    let output = strip_metadata(&input).unwrap();
    assert!(output.len() < input.len(), "output should be smaller after stripping metadata");
    // ftyp and mdat should be preserved
    assert_eq!(&output[4..8], b"ftyp");
    assert!(output.windows(4).any(|w| w == b"mdat"));
}

#[test]
fn test_heic_image_data_preserved() {
    let input = create_minimal_heic();
    let output = strip_metadata(&input).unwrap();
    // mdat content should survive
    assert!(output.windows(4).any(|w| w == b"mdat"));
}
```

- [ ] **Step 2: Run all integration tests**

Run: `cargo test -- tests::heic`
Expected: all HEIC integration tests pass

- [ ] **Step 3: Commit**

```bash
git add tests/integration.rs
git commit -m "test: add HEIC integration tests"
```

---

### Task 10: Update Cargo.toml description and README

**Files:**
- Modify: `Cargo.toml:6`
- Modify: `README.md`

- [ ] **Step 1: Update description in Cargo.toml**

Change line 6 to include HEIC:

```toml
description = "Remove metadata from JPEG, PNG, WebP, GIF, PDF, DOCX, XLSX, PPTX, MP4, MOV, MP3, HEIC files"
```

- [ ] **Step 2: Update README.md to add HEIC to supported formats**

Add HEIC to the supported formats list and table, following the existing pattern for other formats.

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: all tests pass across all formats

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml README.md
git commit -m "docs: add HEIC to supported formats in README and Cargo.toml"
```

---

## Self-Review

**1. Spec coverage:**
- Feature flag + FileFormat variant: Task 1
- Shared ISOBMFF helpers (read_box_header, write_box, read_fullbox_header, write_fullbox): Task 2
- Format detection via ftyp brands: Task 3
- HeicRemover skeleton + passthrough: Task 4
- iinf/iloc parsing + EXIF removal: Task 5
- XMP removal: Task 6
- ICC removal via iprp/ipco/ipma: Task 7
- Error handling (invalid header, missing meta, truncated): Task 8
- Integration tests: Task 9
- Docs/README: Task 10
- All spec sections covered.

**2. Placeholder scan:** No TBDs, TODOs, or vague steps. All code blocks contain complete implementations.

**3. Type consistency:** All functions use consistent types:
- `read_box_header` returns `Option<(usize, usize, [u8; 4])>`
- `write_box(output: &mut Vec<u8>, box_type: &[u8; 4], data: &[u8])`
- Item IDs are `u16` throughout
- Removed IDs tracked as `Vec<u16>`
