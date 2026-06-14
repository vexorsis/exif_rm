# MP3 ID3 Metadata Stripping

## Summary

Add MP3 audio file support to exif_rm by stripping ID3v1 and ID3v2 metadata tags. Hand-rolled parser, no new dependencies, strip everything.

## Format Detection

Add `Mp3` variant to `FileFormat` enum. Detection in `src/format.rs` checks:

1. First 3 bytes are `ID3` (0x49, 0x44, 0x33) — ID3v2 header present
2. First 2 bytes match an MP3 sync word — `0xFF 0xFB`, `0xFF 0xF3`, `0xFF 0xF2`, or `0xFF 0xE0`-`0xFF 0xFF` (MPEG audio frame header)
3. Last 128 bytes start with `TAG` (0x54, 0x41, 0x47) — ID3v1 trailer

If any match, return `Mp3`.

## ID3v2 Header Stripping

ID3v2 tags sit at the file start. Structure:

- Header: 10 bytes — "ID3" + version (2 bytes) + flags (1 byte) + size (4 bytes, syncsafe integer)
- Frames: variable-length body (size bytes from header)
- Footer: optional 10 bytes if bit 4 of flags byte is set

Syncsafe size encoding: each byte uses only 7 bits (bit 7 always 0), 4 bytes can represent up to 256MB.

Stripping logic:

1. Read 10-byte ID3v2 header
2. Decode syncsafe size from bytes [6..10]
3. If footer flag (bit 4) set, add 10 bytes
4. Skip `10 + size + optional_footer` bytes from start
5. Trim leading `0x00` padding bytes before first sync word

## ID3v1 Trailer Stripping

ID3v1 tags are exactly 128 bytes at the file end:

- Signature: "TAG" (0x54, 0x41, 0x47) — first 3 bytes
- Body: 125 bytes of fixed-width fields

Stripping logic:

1. Check file has at least 128 bytes
2. Read last 128 bytes
3. If they start with "TAG", chop them off

Both tags can coexist — strip ID3v2 from front first, then check remaining data for ID3v1 trailer.

## Mp3Remover Implementation

New `src/remove/mp3.rs` with `Mp3Remover` struct implementing `MetadataRemover`:

- `format() -> FileFormat::Mp3`
- `remove_metadata(data, options) -> Result<Vec<u8>>`:
  1. Strip ID3v2 from front (if present)
  2. Strip ID3v1 from end (if present)
  3. If result is empty (no audio data), return `InvalidData` error
  4. Return clean audio data

No new `RemovalOptions` fields needed — existing defaults (all `true`) are sufficient since we strip everything.

## Codebase Changes

| File | Change |
|------|--------|
| `src/traits.rs` | Add `Mp3` variant to `FileFormat` enum |
| `src/format.rs` | Add MP3 detection logic |
| `src/remove/mod.rs` | Add `#[cfg(feature = "mp3")]` module re-export |
| `src/lib.rs` | Add `Mp3` case to `get_remover()` dispatch |
| `Cargo.toml` | Add `mp3` feature (default), no new dependencies |
| `tests/integration.rs` | Add 6 MP3 integration tests |

No changes to CLI, UniFFI bindings, iOS/Android code, or CI.

## Error Handling

All errors use existing `InvalidData(String)` variant:

- No audio data after stripping: `"MP3 file contains no audio data"`
- Truncated ID3v2 header (file starts with "ID3" but < 10 bytes): `"Truncated ID3v2 header"`
- ID3v2 declared size exceeds file length: `"ID3v2 tag size exceeds file length"`

## Integration Tests

1. Strip removes ID3v2 tag
2. Strip removes ID3v1 tag
3. Strip removes both ID3v2 + ID3v1
4. MP3 format detection
5. Clean MP3 (no tags) passes through unchanged
6. Error on file with only tags, no audio

Tests construct synthetic minimal MP3 bytes (ID3v2 header + MPEG frame + optional ID3v1 trailer), consistent with existing test patterns.
