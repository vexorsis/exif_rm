# HEIC Metadata Stripping Design

## Overview

Add HEIC (High Efficiency Image Coding) metadata stripping to exif_rm, following the project's established pattern of feature-gated format modules with hand-rolled parsers.

HEIC files use the ISOBMFF container format (same as MP4/MOV) but with an item-based metadata model instead of tracks. This design extends the existing ISOBMFF box-walking infrastructure from `video.rs` and adds HEIC-specific parsing for the HEIF item model.

## Scope

- Standard HEIC files only (ftyp major brand or compatible brand = `heic`)
- Strip EXIF, XMP, and ICC profile metadata
- Hand-rolled ISOBMFF parser (no new dependencies)
- Feature-gated behind `heic` feature flag

## Architecture

### New module: `src/remove/heic.rs`

Implements `HeicRemover` with the `MetadataRemover` trait. Contains all HEIC-specific logic: parsing `meta`, `iinf`, `iloc`, `iprp`/`ipco`/`ipma` boxes and rebuilding them with metadata items excluded.

### Shared ISOBMFF helpers: `src/remove/isobmff.rs`

Extract from `video.rs` into a shared module (gated with `cfg(any(feature = "video", feature = "heic"))`):

- `read_box_header(cursor) -> Option<(total_size, header_size, box_type)>` — existing, unchanged
- `write_box(output, box_type, data) -> Result<()>` — existing, unchanged
- `read_fullbox_header(cursor) -> Option<(total_size, header_size, box_type, version, flags)>` — new, for full boxes like `meta`, `iinf`, `iloc`

`video.rs` switches to `use crate::remove::isobmff::{read_box_header, write_box}` — no behavioral change.

### FileFormat enum

Add `Heic` variant to `FileFormat` in `src/traits.rs`.

### Format detection

Update `src/format.rs` to differentiate HEIC from MP4 by inspecting the `ftyp` box body:

- Parse `ftyp` box: `[major_brand: 4B] [minor_version: 4B] [compatible_brands: 4B each]`
- If major brand is `heic` or any compatible brand is `heic` → return `Heic`
- Otherwise → return `Mp4` (existing behavior)
- HEIC detection gated behind `cfg(feature = "heic")`; if feature is off, return `UnsupportedFormat`

### Routing

`get_remover` in `src/lib.rs` dispatches `FileFormat::Heic` to `HeicRemover`.

### Feature flag

Add `heic` to `Cargo.toml` features and default features. Add `#[cfg(feature = "heic")] pub mod heic;` to `src/remove/mod.rs`.

## HEIC Metadata Model & Stripping Strategy

HEIC uses ISOBMFF's item-based model. Metadata lives as "items" referenced by `iloc` (item location) and described by `iinf` (item info).

### Metadata items to strip

| Metadata | Storage | Stripping action |
|----------|---------|-----------------|
| EXIF | Item with type `Exif` in `iinf`, located via `iloc` | Remove `iinf` entry and its `iloc` extent(s) |
| XMP | Item with type `mime` in `iinf` with `content_type` = `application/rdf+xml`, located via `iloc` | Remove `iinf` entry and its `iloc` extent(s) |
| ICC profile | `colr` property box inside `ipco`, referenced by `ipma` | Remove `colr` from `ipco` and its association from `ipma` |

### Stripping algorithm

1. Parse top-level boxes — walk ISOBMFF structure looking for `meta`
2. Parse `iinf` — identify items with type `Exif` or `mime` (XMP). Record their item IDs.
3. Rebuild `iinf` — exclude metadata items.
4. Rebuild `iloc` — exclude extents for metadata item IDs.
5. Process `iprp`/`ipco`/`ipma` — if `options.icc_profile`, find and remove `colr` from `ipco` and its association from `ipma`.
6. Pass through all other top-level boxes unchanged (`ftyp`, `mdat`, etc.).

### Full box handling

`meta`, `iinf`, and `iloc` are full boxes (version + flags after box header). When rebuilding, preserve the version and flags from the original box.

### iloc version handling

- Version 0: item_id = 16-bit, extent_offset = 16/32/64-bit (per offset_size), extent_length = 16/32/64-bit (per length_size)
- Version 1: item_id = 16-bit, construction_method = 4-bit, extent_offset/length same as v0
- Version 2+: reject with `InvalidData` error

`iloc` header also contains `offset_size` and `length_size` fields (4 bits each) that determine extent field widths. These must be preserved when rebuilding.

### RemovalOptions mapping

| Option | HEIC action |
|--------|------------|
| `exif: true` | Remove `Exif` item |
| `xmp: true` | Remove `mime` item with XMP content type |
| `icc_profile: true` | Remove `colr` from `ipco` + `ipma` |
| `iptc` / `document_properties` / `comments` / `timestamps` | No-op (not applicable to HEIC) |

## Error Handling

- `InvalidData("HEIC")` — input doesn't have valid `ftyp` with `heic` brand, or is too short/corrupted
- `InvalidData("HEIC: no meta box found")` — `meta` box is missing
- `InvalidData("HEIC: no boxes processed")` — output is empty after processing

## Edge Cases

- **No metadata present** — return input unchanged (passthrough)
- **Truncated box** — break out of box-walking loop early, same as `video.rs`
- **Multiple `iloc` extents per item** — all extents for a removed item ID are excluded from rebuilt `iloc`
- **`meta` at top level** — HEIC has `meta` at top level; only process top-level `meta` boxes

## Testing

All tests use synthetic binary data constructed from scratch (no fixture files), consistent with project convention.

### Unit tests in `heic.rs`

1. Passthrough — HEIC with no metadata items returned unchanged
2. Strip EXIF — HEIC with `Exif` item, verify item removed from rebuilt `iinf` and `iloc`
3. Strip XMP — HEIC with `mime` XMP item, verify removal
4. Strip ICC — HEIC with `colr` in `ipco`, verify `colr` removed and `ipma` association removed
5. Keep ICC by default — verify `colr` preserved when `icc_profile: false`
6. Invalid header — non-HEIC input returns error
7. Missing meta box — HEIC without `meta` returns error
8. Truncated data — `ftyp` with `heic` brand but truncated data returns error

### Integration tests in `tests/integration.rs`

1. Format detection — synthetic HEIC bytes detected as `Heic`
2. Strip removes metadata — HEIC with EXIF+XMP, verify both stripped
3. Image data preserved — verify `mdat` content survives stripping

### Test data construction

Each test builds a minimal valid HEIC byte sequence: `ftyp` box + `meta` box (containing `hdlr`, `iinf`, `iloc`, `iprp`/`ipco`/`ipma`) + `mdat` box. Metadata items are added as needed per test.
