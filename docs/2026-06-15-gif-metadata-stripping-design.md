# GIF Metadata Stripping Design

## Goal

Add GIF format support to exif_rm, stripping Comment Extension blocks and non-looping Application Extension blocks (e.g., XMP) while preserving animation structure (NETSCAPE2.0/ANIMEXTS1.0 looping extensions, Graphic Control Extensions, image data).

## Format Detection

GIF files start with a 6-byte signature: `GIF87a` or `GIF89a`.

Detection is added to `format.rs` after PNG in the priority chain. The magic bytes are unambiguous.

## GIF Binary Structure

```
Header (6 bytes): "GIF87a" or "GIF89a"
Logical Screen Descriptor (7 bytes): width, height, packed field, bg color, pixel aspect ratio
Global Color Table: optional, size from packed field in LSD
Blocks until Trailer (0x3B):
  Image Descriptor (0x2C): 10 bytes + optional Local Color Table + LZW sub-blocks
  Extension (0x21) + label byte:
    0xF9 Graphic Control Extension: 3 data sub-blocks — PRESERVE
    0xFF Application Extension: 11-byte app ID + auth code + sub-blocks — CONDITIONAL
    0xFE Comment Extension: sub-blocks — STRIP
    0x01 Plain Text Extension: sub-blocks — STRIP
  Trailer (0x3B) — PRESERVE
```

## Strip/Preserve Rules

| Block | Action | Reason |
|-------|--------|--------|
| Header + LSD + GCT | Preserve | Structural |
| Image Descriptor + LCT + image data | Preserve | Core content |
| Graphic Control Extension (0xF9) | Preserve | Structural for animation timing/transparency |
| Application Extension (0xFF) — NETSCAPE2.0 | Preserve | Animation looping |
| Application Extension (0xFF) — ANIMEXTS1.0 | Preserve | Animation looping (variant) |
| Application Extension (0xFF) — other | Strip | Metadata (XMP, etc.) |
| Comment Extension (0xFE) | Strip | Metadata |
| Plain Text Extension (0x01) | Strip | Metadata (rarely used) |
| Trailer (0x3B) | Preserve | File terminator |

## GifRemover Implementation

Hand-rolled parser, no new dependencies. Follows the same pattern as Mp3Remover and VideoRemover.

### Algorithm

1. Validate header — check for `GIF87a` or `GIF89a` at offset 0
2. Copy header (6 bytes) + LSD (7 bytes) verbatim
3. If GCT flag is set in LSD packed field, copy GCT verbatim (size = 3 * 2^(GCT size field + 1) bytes)
4. Walk blocks by reading introducer byte:
   - `0x2C` (Image Descriptor): copy 10-byte descriptor, check for LCT flag and copy LCT if present, then copy LZW sub-blocks verbatim (read size byte, copy size+1 bytes, repeat until size=0)
   - `0x21,0xF9` (Graphic Control Extension): copy verbatim — label byte (0xF9), block size byte (always 4), 4 bytes of data, terminator byte (0x00)
   - `0x21,0xFF` (Application Extension): read 11-byte application identifier + auth code. If `NETSCAPE2.0` or `ANIMEXTS1.0`, copy entire extension verbatim. Otherwise, skip all sub-blocks (strip)
   - `0x21,0xFE` (Comment Extension): skip all sub-blocks (strip)
   - `0x21,0x01` (Plain Text Extension): skip all sub-blocks (strip)
   - `0x3B` (Trailer): copy verbatim, done
5. Return output buffer

### Sub-block skipping

Core primitive for stripping: read 1-byte size, if >0 skip that many bytes, repeat until size=0.

### RemovalOptions

GifRemover ignores `RemovalOptions` entirely — always strips comments and non-looping app extensions. Consistent with Mp3Remover and VideoRemover behavior.

## Codebase Changes

1. **`src/traits.rs`** — Add `Gif` variant to `FileFormat` enum
2. **`Cargo.toml`** — Add `gif` feature flag, add to default features list
3. **`src/format.rs`** — Add GIF detection after PNG in priority chain
4. **`src/remove/gif.rs`** — New file: `GifRemover` implementing `MetadataRemover`
5. **`src/remove/mod.rs`** — Add `#[cfg(feature = "gif")] pub mod gif;`
6. **`src/lib.rs`** — Add `FileFormat::Gif => Box::new(GifRemover)` in `get_remover()`
7. **`tests/integration.rs`** — Add GIF integration tests
8. **`README.md`** — Add GIF to supported formats table

## Error Handling

- `FormatDetectionFailed` — bytes don't start with GIF signature
- `InvalidData(String)` — malformed GIF structure (unexpected EOF, invalid block introducer)
- No new error types

## Testing

### Unit tests (in `gif.rs`)

- GIF with Comment Extension — verify comment is stripped, image data preserved
- GIF with XMP Application Extension — verify XMP is stripped
- GIF with NETSCAPE2.0 looping — verify looping extension is preserved
- GIF with ANIMEXTS1.0 looping — verify looping extension is preserved
- Animated GIF with multiple frames — verify all frames and GCE blocks preserved
- GIF with Plain Text Extension — verify it's stripped
- GIF87a format — verify detection and processing works
- Malformed GIF — verify appropriate error

### Integration tests (in `integration.rs`)

- `strip_metadata` on GIF with metadata — verify metadata removed
- `detect_format` on GIF bytes — returns `FileFormat::Gif`
- Format detection edge cases (GIF87a vs GIF89a)
