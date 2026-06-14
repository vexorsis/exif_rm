# MP3 ID3 Metadata Stripping Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add MP3 audio file support by stripping ID3v1 and ID3v2 metadata tags with a hand-rolled parser.

**Architecture:** New `Mp3Remover` in `src/remove/mp3.rs` strips ID3v2 header tags (by decoding the syncsafe size and skipping) and ID3v1 trailer tags (by detecting the "TAG" signature at the end). New `mp3` feature flag gates the module. Format detection checks for ID3v2 header, MP3 sync words, and ID3v1 trailer.

**Tech Stack:** Rust, no new dependencies.

---

### Task 1: Add `Mp3` variant to `FileFormat` enum and `mp3` feature flag

**Files:**
- Modify: `src/traits.rs:3-13`
- Modify: `Cargo.toml:23-29`

- [ ] **Step 1: Add `Mp3` variant to `FileFormat` enum in `src/traits.rs`**

Add `Mp3` after `Webp`:

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
}
```

- [ ] **Step 2: Add `mp3` feature flag to `Cargo.toml`**

Add `mp3` to the default features list and add the feature definition (no dependencies):

```toml
default = ["jpeg", "png", "pdf", "office", "video", "webp", "mp3"]
```

```toml
mp3 = []
```

Insert the `mp3 = []` line after the `webp = []` line.

- [ ] **Step 3: Run `cargo check` to verify compilation**

Run: `cargo check`
Expected: compiles with warnings about unused `Mp3` variant (that's fine, we'll use it soon)

- [ ] **Step 4: Commit**

```bash
git add src/traits.rs Cargo.toml
git commit -m "feat: add Mp3 variant to FileFormat enum and mp3 feature flag"
```

---

### Task 2: Add MP3 format detection

**Files:**
- Modify: `src/format.rs:1-50`

- [ ] **Step 1: Add MP3 detection logic in `src/format.rs`**

Insert before the Office detection block (before the `// Office Open XML` comment). Add it after the WebP detection block:

```rust
    // MP3: ID3v2 header, MPEG sync word, or ID3v1 trailer
    if bytes.starts_with(b"ID3") {
        #[cfg(feature = "mp3")]
        return Ok(FileFormat::Mp3);
        #[cfg(not(feature = "mp3"))]
        return Err(Error::UnsupportedFormat);
    }
    if bytes.len() >= 2 && bytes[0] == 0xFF && (bytes[1] & 0xE0) == 0xE0 {
        #[cfg(feature = "mp3")]
        return Ok(FileFormat::Mp3);
        #[cfg(not(feature = "mp3"))]
        return Err(Error::UnsupportedFormat);
    }
    if bytes.len() >= 128 && &bytes[bytes.len() - 128..bytes.len() - 125] == b"TAG" {
        #[cfg(feature = "mp3")]
        return Ok(FileFormat::Mp3);
        #[cfg(not(feature = "mp3"))]
        return Err(Error::UnsupportedFormat);
    }
```

The sync word check `bytes[1] & 0xE0 == 0xE0` covers all valid MPEG audio frame header second bytes (0xFB, 0xF3, 0xF2, etc.) — any byte where the top 3 bits are 111.

- [ ] **Step 2: Run `cargo test` to verify existing tests still pass**

Run: `cargo test`
Expected: all existing tests pass

- [ ] **Step 3: Commit**

```bash
git add src/format.rs
git commit -m "feat: add MP3 format detection (ID3v2, sync word, ID3v1)"
```

---

### Task 3: Write failing integration tests for MP3

**Files:**
- Modify: `tests/integration.rs`

- [ ] **Step 1: Add MP3 test helpers and 6 integration tests at the end of `tests/integration.rs`**

```rust
// --- MP3 tests ---

/// Minimal MPEG-1 Layer III audio frame header (4 bytes) + small payload.
/// This is a valid-looking sync word + frame header, not real audio.
fn minimal_mpeg_frame() -> Vec<u8> {
    // MPEG1, Layer3, no CRC, 128kbps, 44100Hz, no padding, stereo
    let mut frame = vec![0xFF, 0xFB, 0x90, 0x00];
    // Pad with some dummy data to make it look like a frame
    frame.extend_from_slice(&[0u8; 100]);
    frame
}

/// Build an ID3v2 tag header with the given body size (syncsafe encoded).
/// Returns just the 10-byte header.
fn id3v2_header(tag_body_size: u32, has_footer: bool) -> Vec<u8> {
    let mut header = Vec::with_capacity(10);
    header.extend_from_slice(b"ID3");           // magic
    header.extend_from_slice(&[0x03, 0x00]);    // version 2.3.0
    let flags = if has_footer { 0x10u8 } else { 0x00u8 };
    header.push(flags);                          // flags
    // syncsafe integer encoding for size
    let size = tag_body_size;
    header.push(((size >> 21) & 0x7F) as u8);
    header.push(((size >> 14) & 0x7F) as u8);
    header.push(((size >> 7) & 0x7F) as u8);
    header.push((size & 0x7F) as u8);
    header
}

/// Build an ID3v1 tag (128 bytes) with dummy metadata.
fn id3v1_tag() -> Vec<u8> {
    let mut tag = Vec::with_capacity(128);
    tag.extend_from_slice(b"TAG");               // signature
    tag.extend_from_slice(b"Title\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"); // 30 bytes
    tag.extend_from_slice(b"Artist\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"); // 30 bytes
    tag.extend_from_slice(b"Album\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"); // 30 bytes
    tag.extend_from_slice(b"2024");              // year (4 bytes)
    tag.extend_from_slice(&[0u8; 30]);           // comment (30 bytes)
    tag.push(0);                                  // genre
    tag
}

fn create_mp3_with_id3v2() -> Vec<u8> {
    let body = b"TIT2\x00\x00\x00\x04\x00\x00\x00Test"; // fake ID3v2 frame
    let mut file = id3v2_header(body.len() as u32, false);
    file.extend_from_slice(body);
    file.extend_from_slice(&minimal_mpeg_frame());
    file
}

fn create_mp3_with_id3v1() -> Vec<u8> {
    let mut file = minimal_mpeg_frame();
    file.extend_from_slice(&id3v1_tag());
    file
}

fn create_mp3_with_both_tags() -> Vec<u8> {
    let body = b"TIT2\x00\x00\x00\x04\x00\x00\x00Test";
    let mut file = id3v2_header(body.len() as u32, false);
    file.extend_from_slice(body);
    file.extend_from_slice(&minimal_mpeg_frame());
    file.extend_from_slice(&id3v1_tag());
    file
}

#[test]
fn test_mp3_strip_removes_id3v2() {
    let input = create_mp3_with_id3v2();
    let output = strip_metadata(&input).unwrap();
    assert!(!output.starts_with(b"ID3"), "ID3v2 header should be removed");
    assert!(output.starts_with(&[0xFF, 0xFB]), "audio data should start with sync word");
}

#[test]
fn test_mp3_strip_removes_id3v1() {
    let input = create_mp3_with_id3v1();
    let output = strip_metadata(&input).unwrap();
    assert!(!output.windows(3).any(|w| w == b"TAG"), "ID3v1 tag should be removed");
}

#[test]
fn test_mp3_strip_removes_both_tags() {
    let input = create_mp3_with_both_tags();
    let output = strip_metadata(&input).unwrap();
    assert!(!output.starts_with(b"ID3"), "ID3v2 header should be removed");
    assert!(!output.windows(3).any(|w| w == b"TAG"), "ID3v1 tag should be removed");
    assert!(output.starts_with(&[0xFF, 0xFB]), "audio data should remain");
}

#[test]
fn test_mp3_format_detection() {
    let input = create_mp3_with_id3v2();
    let format = detect_format(&input).unwrap();
    assert_eq!(format, FileFormat::Mp3);
}

#[test]
fn test_mp3_clean_passthrough() {
    let input = minimal_mpeg_frame();
    let output = strip_metadata(&input).unwrap();
    assert_eq!(input, output, "clean MP3 should pass through unchanged");
}

#[test]
fn test_mp3_only_tags_returns_error() {
    // ID3v2 header with 0-byte body, no audio data
    let input = id3v2_header(0, false);
    let result = strip_metadata(&input);
    assert!(result.is_err(), "file with only tags and no audio should error");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -- test_mp3`
Expected: compilation error — `Mp3` variant not handled in `get_remover`, `mp3` module doesn't exist yet

- [ ] **Step 3: Commit**

```bash
git add tests/integration.rs
git commit -m "test: add failing MP3 integration tests"
```

---

### Task 4: Implement `Mp3Remover`

**Files:**
- Create: `src/remove/mp3.rs`
- Modify: `src/remove/mod.rs`
- Modify: `src/lib.rs:5,25-48`

- [ ] **Step 1: Create `src/remove/mp3.rs` with the `Mp3Remover` implementation**

```rust
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
        // syncsafe size = 14 bytes
        input.extend_from_slice(&[0x00, 0x00, 0x00, 0x0E]);
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
```

- [ ] **Step 2: Add `mp3` module to `src/remove/mod.rs`**

Add after the `webp` module:

```rust
#[cfg(feature = "mp3")]
pub mod mp3;
```

- [ ] **Step 3: Wire `Mp3` into `get_remover()` in `src/lib.rs`**

Add the `mp3` feature to the `any()` gate on line 5:

```rust
#[cfg(any(feature = "jpeg", feature = "png", feature = "pdf", feature = "office", feature = "video", feature = "webp", feature = "mp3"))]
```

Add the `Mp3` arm in `get_remover()` after the `Webp` arm:

```rust
        #[cfg(feature = "mp3")]
        FileFormat::Mp3 => Box::new(remove::mp3::Mp3Remover),
```

- [ ] **Step 4: Run all tests**

Run: `cargo test`
Expected: all tests pass, including the 6 new MP3 integration tests and 5 new unit tests

- [ ] **Step 5: Commit**

```bash
git add src/remove/mp3.rs src/remove/mod.rs src/lib.rs
git commit -m "feat: implement Mp3Remover with ID3v1/v2 tag stripping"
```

---

### Task 5: Update README and Cargo.toml description

**Files:**
- Modify: `README.md`
- Modify: `Cargo.toml:4`

- [ ] **Step 1: Update `Cargo.toml` description to include MP3**

Change line 4 from:
```toml
description = "Remove metadata from JPEG, PNG, WebP, PDF, DOCX, XLSX, PPTX, MP4, MOV files"
```
to:
```toml
description = "Remove metadata from JPEG, PNG, WebP, PDF, DOCX, XLSX, PPTX, MP4, MOV, MP3 files"
```

- [ ] **Step 2: Update `README.md` to document MP3 support**

Read the current README and add MP3 to the supported formats table. Add a row for MP3 with the metadata types "ID3v1, ID3v2" and the description "Artist, title, album, cover art, lyrics, comments, etc."

- [ ] **Step 3: Run `cargo test` to confirm nothing broke**

Run: `cargo test`
Expected: all tests pass

- [ ] **Step 4: Commit**

```bash
git add Cargo.toml README.md
git commit -m "docs: add MP3 to supported formats in README and Cargo.toml"
```
