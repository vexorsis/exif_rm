# GIF Metadata Stripping Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add GIF format support that strips Comment Extension blocks and non-looping Application Extension blocks while preserving animation structure.

**Architecture:** Hand-rolled binary parser walks the GIF block structure, copying structural blocks (header, LSD, GCT, image descriptors, GCE, NETSCAPE2.0/ANIMEXTS1.0, trailer) verbatim and skipping metadata blocks (comments, non-looping app extensions, plain text extensions). No new dependencies.

**Tech Stack:** Rust, no new crates

---

### Task 1: Add Gif variant to FileFormat enum and gif feature flag

**Files:**
- Modify: `src/traits.rs:3-14`
- Modify: `Cargo.toml:23-31`

- [ ] **Step 1: Add `Gif` variant to `FileFormat` enum in `src/traits.rs`**

Add `Gif` after `Mp3` in the enum:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, uniffi::Enum)]
pub enum FileFormat {
    #[default]
    Jpeg,
    Png,
    Pdf,
    Docx,
    Xlsx,
    Pptx,
    Mp4,
    Webp,
    Mp3,
    Gif,
}
```

- [ ] **Step 2: Add `gif` feature flag in `Cargo.toml`**

Add `gif` to the default features list and add the feature definition. No conditional dependencies needed.

```toml
default = ["jpeg", "png", "pdf", "office", "video", "webp", "mp3", "gif"]
```

And add the feature:

```toml
gif = []
```

- [ ] **Step 3: Commit**

```bash
git add src/traits.rs Cargo.toml
git commit -m "feat: add Gif variant to FileFormat enum and gif feature flag"
```

---

### Task 2: Add GIF format detection

**Files:**
- Modify: `src/format.rs:14-17`

- [ ] **Step 1: Add GIF detection in `src/format.rs` after PNG check**

Insert after the PNG detection block (after line 17), before the PDF check:

```rust
// GIF: GIF87a or GIF89a
if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
    #[cfg(feature = "gif")]
    return Ok(FileFormat::Gif);
    #[cfg(not(feature = "gif"))]
    return Err(Error::UnsupportedFormat);
}
```

- [ ] **Step 2: Write a failing test for GIF format detection**

Add to `tests/integration.rs`:

```rust
// --- GIF tests ---

fn create_minimal_gif() -> Vec<u8> {
    let mut gif = Vec::new();
    // Header: GIF89a
    gif.extend_from_slice(b"GIF89a");
    // Logical Screen Descriptor: 1x1, no GCT
    gif.extend_from_slice(&[0x01, 0x00]); // width = 1
    gif.extend_from_slice(&[0x01, 0x00]); // height = 1
    gif.push(0x00); // packed: no GCT, color resolution 1, no sort, GCT size = 0
    gif.push(0x00); // background color index
    gif.push(0x00); // pixel aspect ratio
    // Image Descriptor
    gif.push(0x2C); // image separator
    gif.extend_from_slice(&[0x00, 0x00]); // left = 0
    gif.extend_from_slice(&[0x00, 0x00]); // top = 0
    gif.extend_from_slice(&[0x01, 0x00]); // width = 1
    gif.extend_from_slice(&[0x01, 0x00]); // height = 1
    gif.push(0x00); // packed: no LCT, not interlaced
    // LZW minimum code size
    gif.push(0x02);
    // Image data sub-block: single pixel (index 0)
    gif.push(0x02); // sub-block size = 2
    gif.extend_from_slice(&[0x4C, 0x01]); // LZW compressed data for 1 pixel
    gif.push(0x00); // sub-block terminator
    // Trailer
    gif.push(0x3B);
    gif
}

#[test]
fn test_gif_format_detection() {
    let input = create_minimal_gif();
    let format = detect_format(&input).unwrap();
    assert_eq!(format, FileFormat::Gif);
}

#[test]
fn test_gif87a_format_detection() {
    let mut gif = create_minimal_gif();
    gif[0..6].copy_from_slice(b"GIF87a");
    let format = detect_format(&gif).unwrap();
    assert_eq!(format, FileFormat::Gif);
}
```

- [ ] **Step 3: Run test to verify it passes**

Run: `cargo test test_gif_format_detection --features gif`
Expected: PASS (detection is already wired up)

- [ ] **Step 4: Commit**

```bash
git add src/format.rs tests/integration.rs
git commit -m "feat: add GIF format detection (GIF87a/GIF89a)"
```

---

### Task 3: Add GifRemover module scaffolding

**Files:**
- Create: `src/remove/gif.rs`
- Modify: `src/remove/mod.rs:1-14`
- Modify: `src/lib.rs:5,47`

- [ ] **Step 1: Create `src/remove/gif.rs` with minimal GifRemover (passthrough)**

```rust
use crate::error::Error;
use crate::traits::{FileFormat, MetadataRemover, RemovalOptions};

pub struct GifRemover;

impl MetadataRemover for GifRemover {
    fn format(&self) -> FileFormat {
        FileFormat::Gif
    }

    fn remove_metadata(&self, input: &[u8], _options: &RemovalOptions) -> crate::Result<Vec<u8>> {
        if !input.starts_with(b"GIF87a") && !input.starts_with(b"GIF89a") {
            return Err(Error::FormatDetectionFailed);
        }
        // TODO: implement actual stripping
        Ok(input.to_vec())
    }
}
```

- [ ] **Step 2: Add module declaration in `src/remove/mod.rs`**

Add after the `mp3` line:

```rust
#[cfg(feature = "gif")]
pub mod gif;
```

- [ ] **Step 3: Add `gif` to the `any()` gate in `src/lib.rs` line 5**

Change:
```rust
#[cfg(any(feature = "jpeg", feature = "png", feature = "pdf", feature = "office", feature = "video", feature = "webp", feature = "mp3"))]
```
To:
```rust
#[cfg(any(feature = "jpeg", feature = "png", feature = "pdf", feature = "office", feature = "video", feature = "webp", feature = "mp3", feature = "gif"))]
```

- [ ] **Step 4: Add match arm in `get_remover()` in `src/lib.rs`**

Add after the `Mp3` arm (after line 47):

```rust
#[cfg(feature = "gif")]
FileFormat::Gif => Box::new(remove::gif::GifRemover),
```

- [ ] **Step 5: Run tests to verify compilation**

Run: `cargo test --features gif`
Expected: All existing tests pass, new GIF tests pass (passthrough)

- [ ] **Step 6: Commit**

```bash
git add src/remove/gif.rs src/remove/mod.rs src/lib.rs
git commit -m "feat: add GifRemover module scaffolding"
```

---

### Task 4: Implement GifRemover metadata stripping

**Files:**
- Modify: `src/remove/gif.rs`

- [ ] **Step 1: Replace the passthrough implementation with the full parser**

Replace the entire contents of `src/remove/gif.rs`:

```rust
use crate::error::Error;
use crate::traits::{FileFormat, MetadataRemover, RemovalOptions};

pub struct GifRemover;

impl MetadataRemover for GifRemover {
    fn format(&self) -> FileFormat {
        FileFormat::Gif
    }

    fn remove_metadata(&self, input: &[u8], _options: &RemovalOptions) -> crate::Result<Vec<u8>> {
        if input.len() < 13 {
            return Err(Error::FormatDetectionFailed);
        }
        if !input.starts_with(b"GIF87a") && !input.starts_with(b"GIF89a") {
            return Err(Error::FormatDetectionFailed);
        }

        let mut out = Vec::with_capacity(input.len());
        let mut pos = 0usize;

        // Copy header (6 bytes)
        out.extend_from_slice(&input[0..6]);
        pos = 6;

        // Copy Logical Screen Descriptor (7 bytes)
        if pos + 7 > input.len() {
            return Err(Error::InvalidData("Truncated LSD".into()));
        }
        out.extend_from_slice(&input[pos..pos + 7]);
        let packed = input[pos];
        pos += 7;

        // Copy Global Color Table if present
        let gct_flag = (packed & 0x80) != 0;
        if gct_flag {
            let gct_size_field = packed & 0x07;
            let gct_bytes = 3 * (1 << (gct_size_field as usize + 1));
            if pos + gct_bytes > input.len() {
                return Err(Error::InvalidData("Truncated GCT".into()));
            }
            out.extend_from_slice(&input[pos..pos + gct_bytes]);
            pos += gct_bytes;
        }

        // Walk blocks until trailer
        loop {
            if pos >= input.len() {
                return Err(Error::InvalidData("Missing trailer".into()));
            }
            let introducer = input[pos];
            pos += 1;

            match introducer {
                // Image Descriptor
                0x2C => {
                    out.push(0x2C);
                    if pos + 9 > input.len() {
                        return Err(Error::InvalidData("Truncated image descriptor".into()));
                    }
                    out.extend_from_slice(&input[pos..pos + 9]);
                    let img_packed = input[pos + 8];
                    pos += 9;

                    // Local Color Table if present
                    let lct_flag = (img_packed & 0x80) != 0;
                    if lct_flag {
                        let lct_size_field = img_packed & 0x07;
                        let lct_bytes = 3 * (1 << (lct_size_field as usize + 1));
                        if pos + lct_bytes > input.len() {
                            return Err(Error::InvalidData("Truncated LCT".into()));
                        }
                        out.extend_from_slice(&input[pos..pos + lct_bytes]);
                        pos += lct_bytes;
                    }

                    // LZW minimum code size (1 byte)
                    if pos >= input.len() {
                        return Err(Error::InvalidData("Missing LZW code size".into()));
                    }
                    out.push(input[pos]);
                    pos += 1;

                    // Copy image data sub-blocks
                    pos = copy_sub_blocks(&input, pos, &mut out)?;
                }

                // Extension
                0x21 => {
                    if pos >= input.len() {
                        return Err(Error::InvalidData("Truncated extension label".into()));
                    }
                    let label = input[pos];
                    pos += 1;

                    match label {
                        // Graphic Control Extension — preserve
                        0xF9 => {
                            out.push(0x21);
                            out.push(0xF9);
                            // Block size byte + data + terminator
                            if pos >= input.len() {
                                return Err(Error::InvalidData("Truncated GCE".into()));
                            }
                            let block_size = input[pos] as usize;
                            out.push(input[pos]);
                            pos += 1;
                            if pos + block_size > input.len() {
                                return Err(Error::InvalidData("Truncated GCE data".into()));
                            }
                            out.extend_from_slice(&input[pos..pos + block_size]);
                            pos += block_size;
                            // Terminator
                            if pos >= input.len() {
                                return Err(Error::InvalidData("Missing GCE terminator".into()));
                            }
                            out.push(input[pos]);
                            pos += 1;
                        }

                        // Application Extension — conditional
                        0xFF => {
                            // Read 11-byte application identifier + auth code
                            if pos + 11 > input.len() {
                                return Err(Error::InvalidData("Truncated app extension header".into()));
                            }
                            let app_id = &input[pos..pos + 11];
                            let is_looping = app_id.starts_with(b"NETSCAPE2.0")
                                || app_id.starts_with(b"ANIMEXTS1.0");

                            if is_looping {
                                // Preserve: write extension introducer + label + app id + sub-blocks
                                out.push(0x21);
                                out.push(0xFF);
                                out.extend_from_slice(app_id);
                                pos += 11;
                                pos = copy_sub_blocks(&input, pos, &mut out)?;
                            } else {
                                // Strip: skip app id + sub-blocks
                                pos += 11;
                                pos = skip_sub_blocks(&input, pos)?;
                            }
                        }

                        // Comment Extension — strip
                        0xFE => {
                            pos = skip_sub_blocks(&input, pos)?;
                        }

                        // Plain Text Extension — strip
                        0x01 => {
                            pos = skip_sub_blocks(&input, pos)?;
                        }

                        // Unknown extension — preserve (safe default)
                        _ => {
                            out.push(0x21);
                            out.push(label);
                            pos = copy_sub_blocks(&input, pos, &mut out)?;
                        }
                    }
                }

                // Trailer
                0x3B => {
                    out.push(0x3B);
                    break;
                }

                _ => {
                    return Err(Error::InvalidData(format!(
                        "Unexpected block introducer: 0x{:02X}",
                        introducer
                    )));
                }
            }
        }

        Ok(out)
    }
}

/// Skip sub-blocks (read size byte, skip that many bytes, repeat until size=0).
/// Returns the position after the terminator.
fn skip_sub_blocks(input: &[u8], mut pos: usize) -> crate::Result<usize> {
    loop {
        if pos >= input.len() {
            return Err(Error::InvalidData("Truncated sub-blocks".into()));
        }
        let size = input[pos] as usize;
        pos += 1;
        if size == 0 {
            break;
        }
        if pos + size > input.len() {
            return Err(Error::InvalidData("Truncated sub-block data".into()));
        }
        pos += size;
    }
    Ok(pos)
}

/// Copy sub-blocks verbatim to output (read size byte, copy size+1 bytes, repeat until size=0).
/// Returns the position after the terminator.
fn copy_sub_blocks(input: &[u8], mut pos: usize, out: &mut Vec<u8>) -> crate::Result<usize> {
    loop {
        if pos >= input.len() {
            return Err(Error::InvalidData("Truncated sub-blocks".into()));
        }
        let size = input[pos] as usize;
        out.push(input[pos]);
        pos += 1;
        if size == 0 {
            break;
        }
        if pos + size > input.len() {
            return Err(Error::InvalidData("Truncated sub-block data".into()));
        }
        out.extend_from_slice(&input[pos..pos + size]);
        pos += size;
    }
    Ok(pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_minimal_gif() -> Vec<u8> {
        let mut gif = Vec::new();
        gif.extend_from_slice(b"GIF89a");
        gif.extend_from_slice(&[0x01, 0x00, 0x01, 0x00]); // width=1, height=1
        gif.push(0x00); // packed: no GCT
        gif.push(0x00); // bg color
        gif.push(0x00); // aspect ratio
        gif.push(0x2C); // image separator
        gif.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // left, top
        gif.extend_from_slice(&[0x01, 0x00, 0x01, 0x00]); // width=1, height=1
        gif.push(0x00); // packed: no LCT
        gif.push(0x02); // LZW min code size
        gif.push(0x02); // sub-block size
        gif.extend_from_slice(&[0x4C, 0x01]); // compressed data
        gif.push(0x00); // sub-block terminator
        gif.push(0x3B); // trailer
        gif
    }

    fn create_gif_with_comment() -> Vec<u8> {
        let mut gif = Vec::new();
        gif.extend_from_slice(b"GIF89a");
        gif.extend_from_slice(&[0x01, 0x00, 0x01, 0x00]);
        gif.push(0x00);
        gif.push(0x00);
        gif.push(0x00);
        // Comment Extension
        gif.push(0x21);
        gif.push(0xFE);
        gif.push(0x0B); // sub-block size = 11
        gif.extend_from_slice(b"Test Comment");
        gif.push(0x00); // terminator
        // Image Descriptor
        gif.push(0x2C);
        gif.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        gif.extend_from_slice(&[0x01, 0x00, 0x01, 0x00]);
        gif.push(0x00);
        gif.push(0x02);
        gif.push(0x02);
        gif.extend_from_slice(&[0x4C, 0x01]);
        gif.push(0x00);
        gif.push(0x3B);
        gif
    }

    fn create_gif_with_netscape() -> Vec<u8> {
        let mut gif = Vec::new();
        gif.extend_from_slice(b"GIF89a");
        gif.extend_from_slice(&[0x01, 0x00, 0x01, 0x00]);
        gif.push(0x00);
        gif.push(0x00);
        gif.push(0x00);
        // NETSCAPE2.0 Application Extension (loop forever)
        gif.push(0x21);
        gif.push(0xFF);
        gif.extend_from_slice(b"NETSCAPE2.0"); // app identifier + auth code (11 bytes)
        gif.push(0x03); // sub-block size = 3
        gif.push(0x01); // sub-block ID
        gif.extend_from_slice(&[0x00, 0x00]); // loop count = 0 (infinite)
        gif.push(0x00); // terminator
        // Image Descriptor
        gif.push(0x2C);
        gif.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        gif.extend_from_slice(&[0x01, 0x00, 0x01, 0x00]);
        gif.push(0x00);
        gif.push(0x02);
        gif.push(0x02);
        gif.extend_from_slice(&[0x4C, 0x01]);
        gif.push(0x00);
        gif.push(0x3B);
        gif
    }

    fn create_gif_with_xmp() -> Vec<u8> {
        let mut gif = Vec::new();
        gif.extend_from_slice(b"GIF89a");
        gif.extend_from_slice(&[0x01, 0x00, 0x01, 0x00]);
        gif.push(0x00);
        gif.push(0x00);
        gif.push(0x00);
        // XMP Application Extension
        gif.push(0x21);
        gif.push(0xFF);
        gif.extend_from_slice(b"XMP DataXMP"); // app identifier + auth code (11 bytes)
        let xmp_content = b"<x:xmpmeta>fake</x:xmpmeta>";
        gif.push(xmp_content.len() as u8); // sub-block size
        gif.extend_from_slice(xmp_content);
        gif.push(0x00); // terminator
        // Image Descriptor
        gif.push(0x2C);
        gif.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        gif.extend_from_slice(&[0x01, 0x00, 0x01, 0x00]);
        gif.push(0x00);
        gif.push(0x02);
        gif.push(0x02);
        gif.extend_from_slice(&[0x4C, 0x01]);
        gif.push(0x00);
        gif.push(0x3B);
        gif
    }

    fn create_gif_with_gce() -> Vec<u8> {
        let mut gif = Vec::new();
        gif.extend_from_slice(b"GIF89a");
        gif.extend_from_slice(&[0x01, 0x00, 0x01, 0x00]);
        gif.push(0x00);
        gif.push(0x00);
        gif.push(0x00);
        // Graphic Control Extension
        gif.push(0x21);
        gif.push(0xF9);
        gif.push(0x04); // block size = 4
        gif.extend_from_slice(&[0x00, 0x0A, 0x00, 0x00]); // packed, delay=10, transparent
        gif.push(0x00); // terminator
        // Image Descriptor
        gif.push(0x2C);
        gif.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        gif.extend_from_slice(&[0x01, 0x00, 0x01, 0x00]);
        gif.push(0x00);
        gif.push(0x02);
        gif.push(0x02);
        gif.extend_from_slice(&[0x4C, 0x01]);
        gif.push(0x00);
        gif.push(0x3B);
        gif
    }

    fn create_gif_with_gct() -> Vec<u8> {
        let mut gif = Vec::new();
        gif.extend_from_slice(b"GIF89a");
        gif.extend_from_slice(&[0x01, 0x00, 0x01, 0x00]);
        // packed: GCT flag=1, color resolution=0 (1 bit), no sort, GCT size=0 (2 colors)
        gif.push(0x80);
        gif.push(0x00); // bg color
        gif.push(0x00); // aspect ratio
        // GCT: 2 colors * 3 bytes = 6 bytes
        gif.extend_from_slice(&[0xFF, 0x00, 0x00]); // color 0: red
        gif.extend_from_slice(&[0x00, 0xFF, 0x00]); // color 1: green
        // Comment Extension
        gif.push(0x21);
        gif.push(0xFE);
        gif.push(0x05);
        gif.extend_from_slice(b"Hello");
        gif.push(0x00);
        // Image Descriptor
        gif.push(0x2C);
        gif.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        gif.extend_from_slice(&[0x01, 0x00, 0x01, 0x00]);
        gif.push(0x00);
        gif.push(0x02);
        gif.push(0x02);
        gif.extend_from_slice(&[0x4C, 0x01]);
        gif.push(0x00);
        gif.push(0x3B);
        gif
    }

    #[test]
    fn test_minimal_gif_passthrough() {
        let input = create_minimal_gif();
        let output = GifRemover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        assert_eq!(input, output);
    }

    #[test]
    fn test_strip_comment() {
        let input = create_gif_with_comment();
        let output = GifRemover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        assert!(!output.windows(2).any(|w| w == &[0x21, 0xFE]), "comment extension should be removed");
        assert!(output.windows(4).any(|w| w == b"GIF8"), "header should be preserved");
        assert!(output.contains(&0x3B), "trailer should be preserved");
    }

    #[test]
    fn test_preserve_netscape() {
        let input = create_gif_with_netscape();
        let output = GifRemover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        assert!(output.windows(11).any(|w| w == b"NETSCAPE2.0"), "NETSCAPE2.0 should be preserved");
    }

    #[test]
    fn test_strip_xmp() {
        let input = create_gif_with_xmp();
        let output = GifRemover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        assert!(!output.windows(4).any(|w| w == b"XMP "), "XMP app extension should be removed");
    }

    #[test]
    fn test_preserve_gce() {
        let input = create_gif_with_gce();
        let output = GifRemover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        assert!(output.windows(2).any(|w| w == &[0x21, 0xF9]), "GCE should be preserved");
    }

    #[test]
    fn test_preserve_gct() {
        let input = create_gif_with_gct();
        let output = GifRemover.remove_metadata(&input, &RemovalOptions::default()).unwrap();
        // GCT should be preserved (red and green colors)
        assert!(output.windows(3).any(|w| w == &[0xFF, 0x00, 0x00]), "GCT color 0 should be preserved");
        assert!(output.windows(3).any(|w| w == &[0x00, 0xFF, 0x00]), "GCT color 1 should be preserved");
        // Comment should be stripped
        assert!(!output.windows(2).any(|w| w == &[0x21, 0xFE]));
    }

    #[test]
    fn test_invalid_header() {
        let input = b"NOTAGIF1234567";
        let result = GifRemover.remove_metadata(input, &RemovalOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_truncated_lsd() {
        let mut input = Vec::new();
        input.extend_from_slice(b"GIF89a");
        input.extend_from_slice(&[0x01, 0x00]); // only 2 bytes of LSD
        let result = GifRemover.remove_metadata(&input, &RemovalOptions::default());
        assert!(result.is_err());
    }

    #[test]
    fn test_gif87a() {
        let mut gif = create_minimal_gif();
        gif[0..6].copy_from_slice(b"GIF87a");
        let output = GifRemover.remove_metadata(&gif, &RemovalOptions::default()).unwrap();
        assert!(output.starts_with(b"GIF87a"));
    }

    #[test]
    fn test_animexts_preserved() {
        let mut gif = Vec::new();
        gif.extend_from_slice(b"GIF89a");
        gif.extend_from_slice(&[0x01, 0x00, 0x01, 0x00]);
        gif.push(0x00);
        gif.push(0x00);
        gif.push(0x00);
        // ANIMEXTS1.0 Application Extension
        gif.push(0x21);
        gif.push(0xFF);
        gif.extend_from_slice(b"ANIMEXTS1.0");
        gif.push(0x03);
        gif.push(0x01);
        gif.extend_from_slice(&[0x00, 0x00]);
        gif.push(0x00);
        // Image
        gif.push(0x2C);
        gif.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
        gif.extend_from_slice(&[0x01, 0x00, 0x01, 0x00]);
        gif.push(0x00);
        gif.push(0x02);
        gif.push(0x02);
        gif.extend_from_slice(&[0x4C, 0x01]);
        gif.push(0x00);
        gif.push(0x3B);
        let output = GifRemover.remove_metadata(&gif, &RemovalOptions::default()).unwrap();
        assert!(output.windows(11).any(|w| w == b"ANIMEXTS1.0"), "ANIMEXTS1.0 should be preserved");
    }
}
```

- [ ] **Step 2: Run unit tests**

Run: `cargo test --features gif -- gif`
Expected: All GIF unit tests pass

- [ ] **Step 3: Commit**

```bash
git add src/remove/gif.rs
git commit -m "feat: implement GifRemover with comment and app extension stripping"
```

---

### Task 5: Add GIF integration tests

**Files:**
- Modify: `tests/integration.rs`

- [ ] **Step 1: Add integration tests for GIF**

Add to `tests/integration.rs` after the existing GIF format detection tests from Task 2:

```rust
fn create_gif_with_metadata() -> Vec<u8> {
    let mut gif = Vec::new();
    gif.extend_from_slice(b"GIF89a");
    gif.extend_from_slice(&[0x01, 0x00, 0x01, 0x00]);
    gif.push(0x00);
    gif.push(0x00);
    gif.push(0x00);
    // Comment Extension
    gif.push(0x21);
    gif.push(0xFE);
    gif.push(0x0B);
    gif.extend_from_slice(b"Test Comment");
    gif.push(0x00);
    // NETSCAPE2.0 Application Extension
    gif.push(0x21);
    gif.push(0xFF);
    gif.extend_from_slice(b"NETSCAPE2.0");
    gif.push(0x03);
    gif.push(0x01);
    gif.extend_from_slice(&[0x00, 0x00]);
    gif.push(0x00);
    // XMP Application Extension
    gif.push(0x21);
    gif.push(0xFF);
    gif.extend_from_slice(b"XMP DataXMP");
    let xmp = b"<x:xmpmeta>fake</x:xmpmeta>";
    gif.push(xmp.len() as u8);
    gif.extend_from_slice(xmp);
    gif.push(0x00);
    // Graphic Control Extension
    gif.push(0x21);
    gif.push(0xF9);
    gif.push(0x04);
    gif.extend_from_slice(&[0x00, 0x0A, 0x00, 0x00]);
    gif.push(0x00);
    // Image Descriptor
    gif.push(0x2C);
    gif.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    gif.extend_from_slice(&[0x01, 0x00, 0x01, 0x00]);
    gif.push(0x00);
    gif.push(0x02);
    gif.push(0x02);
    gif.extend_from_slice(&[0x4C, 0x01]);
    gif.push(0x00);
    gif.push(0x3B);
    gif
}

#[test]
fn test_gif_strip_removes_metadata() {
    let input = create_gif_with_metadata();
    let output = strip_metadata(&input).unwrap();
    // Comment should be removed
    assert!(!output.windows(2).any(|w| w == &[0x21, 0xFE]));
    // XMP should be removed
    assert!(!output.windows(11).any(|w| w == b"XMP DataXMP"));
    // NETSCAPE2.0 should be preserved
    assert!(output.windows(11).any(|w| w == b"NETSCAPE2.0"));
    // GCE should be preserved
    assert!(output.windows(2).any(|w| w == &[0x21, 0xF9]));
    // Image data should be preserved
    assert!(output.contains(&0x2C));
    assert!(output.contains(&0x3B));
}

#[test]
fn test_gif_clean_passthrough() {
    let input = create_minimal_gif();
    let output = strip_metadata(&input).unwrap();
    assert_eq!(input, output, "clean GIF should pass through unchanged");
}

#[test]
fn test_gif_strip_owned() {
    let input = create_gif_with_metadata();
    let output = strip_metadata_owned(input).unwrap();
    assert!(!output.windows(2).any(|w| w == &[0x21, 0xFE]));
}
```

- [ ] **Step 2: Run all tests**

Run: `cargo test --features gif`
Expected: All tests pass (existing + new GIF tests)

- [ ] **Step 3: Commit**

```bash
git add tests/integration.rs
git commit -m "test: add GIF integration tests"
```

---

### Task 6: Update README and description

**Files:**
- Modify: `README.md:1-3,10,16-27,138`
- Modify: `Cargo.toml:5`

- [ ] **Step 1: Update README description line**

Change line 3:
```
Remove metadata from JPEG, PNG, WebP, PDF, DOCX, XLSX, PPTX, MP4, MOV, and MP3 files.
```
To:
```
Remove metadata from JPEG, PNG, WebP, GIF, PDF, DOCX, XLSX, PPTX, MP4, MOV, and MP3 files.
```

- [ ] **Step 2: Update "What It Does" section**

Change line 10:
```
- Works on images (JPEG, PNG, WebP), documents (PDF, DOCX, XLSX, PPTX), video (MP4, MOV), and audio (MP3)
```
To:
```
- Works on images (JPEG, PNG, WebP, GIF), documents (PDF, DOCX, XLSX, PPTX), video (MP4, MOV), and audio (MP3)
```

- [ ] **Step 3: Add GIF row to supported formats table**

Add after the WebP row (after line 21):

```
| GIF | Comment extensions, application extensions (XMP, etc.) — preserves animation looping |
```

- [ ] **Step 4: Update FileFormat enum listing**

Change line 138:
```
- `FileFormat` — supported format enum (Jpeg, Png, Webp, Pdf, Docx, Xlsx, Pptx, Mp4, Mp3)
```
To:
```
- `FileFormat` — supported format enum (Jpeg, Png, Webp, Gif, Pdf, Docx, Xlsx, Pptx, Mp4, Mp3)
```

- [ ] **Step 5: Update Cargo.toml description**

Change line 5:
```
description = "Remove metadata from JPEG, PNG, WebP, PDF, DOCX, XLSX, PPTX, MP4, MOV, MP3 files"
```
To:
```
description = "Remove metadata from JPEG, PNG, WebP, GIF, PDF, DOCX, XLSX, PPTX, MP4, MOV, MP3 files"
```

- [ ] **Step 6: Commit**

```bash
git add README.md Cargo.toml
git commit -m "docs: add GIF to supported formats in README and Cargo.toml"
```

---

### Task 7: Run full test suite and verify

- [ ] **Step 1: Run all tests with all features**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy --all-features`
Expected: No warnings

- [ ] **Step 3: Verify GIF-only feature build**

Run: `cargo test --no-default-features --features gif`
Expected: Only GIF-related tests compile and pass
