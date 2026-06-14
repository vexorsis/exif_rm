# exif_rm

Remove metadata from JPEG, PNG, WebP, PDF, DOCX, XLSX, PPTX, MP4, MOV, and MP3 files.

![License](https://img.shields.io/badge/license-MIT-blue)

## What It Does

- Strips EXIF, XMP, IPTC, ICC profiles, comments, and timestamps
- Works on images (JPEG, PNG, WebP), documents (PDF, DOCX, XLSX, PPTX), video (MP4, MOV), and audio (MP3)
- Pure Rust core with no runtime dependencies
- CLI tool included
- UniFFI bindings for iOS and Android

## Supported Formats

| Format | Metadata Removed |
|--------|------------------|
| JPEG | EXIF, XMP, IPTC, ICC profile, comments |
| PNG | eXIf, text chunks (tEXt/zTXt/iTXt), iCCP, tIME |
| WebP | EXIF, XMP, ICC profile |
| PDF | /Info dictionary, /Metadata stream |
| DOCX | core.xml, app.xml, custom.xml |
| XLSX | core.xml, app.xml, custom.xml |
| PPTX | core.xml, app.xml, custom.xml |
| MP4/MOV | iTunes metadata, user data (udta), timed metadata tracks |
| MP3 | ID3v1, ID3v2 (artist, title, album, cover art, lyrics, comments, etc.) |

## Quick Start

Add to `Cargo.toml`:

```toml
[dependencies]
exif_rm = "0.1"
```

```rust
use exif_rm::{strip_metadata, strip_metadata_with, RemovalOptions};

// Strip with defaults (keeps ICC profile)
let cleaned = strip_metadata(&file_bytes)?;

// Strip everything including ICC
let options = RemovalOptions { icc_profile: true, ..RemovalOptions::default() };
let cleaned = strip_metadata_with(&file_bytes, &options)?;
```

## CLI

```bash
cargo install exif_rm --features cli
```

```bash
# Strip metadata in-place
exif_rm photo.jpg

# Output to a different directory
exif_rm input.docx --output ./cleaned/

# Strip ICC profiles too
exif_rm image.png --strip-icc

# Create backups before modifying
exif_rm photo.jpg --backup bak

# Suppress output
exif_rm -q photo.jpg
```

## Prebuilt Packages

Download prebuilt packages from [GitHub Releases](https://github.com/vexorsis/exif_rm/releases).

### iOS

1. Download `exif_rm-ios-xcframework.zip` from the latest release
2. Unzip and drag `exif_rmFFI.xcframework` into your Xcode project
3. Add `exif_rm.swift` (included in the zip) to your target's sources

### Android

1. Download `exif_rm-android.aar` from the latest release
2. Copy it to your app's `libs/` directory
3. Add to `build.gradle.kts`:

```kotlin
dependencies {
    implementation(files("libs/exif_rm-android.aar"))
}
```

## Android (Maven Central)

The library is available on Maven Central. Add to your `build.gradle`:

```groovy
implementation("io.github.vexorsis:exif-rm:0.1.0")
```

## Building from Source (Mobile)

### iOS

Prerequisites: Xcode, Rust with `aarch64-apple-ios` and `aarch64-apple-ios-sim` targets.

```bash
./scripts/build-ios.sh
```

This produces an XCFramework and Swift bindings. Add the Swift Package at `ios-rust/` to your Xcode project. See `ios/` for a sample SwiftUI app.

### Android

Prerequisites: Android NDK, `cargo-ndk` (`cargo install cargo-ndk`), JDK 21.

```bash
export ANDROID_NDK_HOME=$HOME/Library/Android/sdk/ndk/<version>
./scripts/build-android.sh
cd android
./gradlew :library:assembleRelease
```

This produces `library-release.aar` containing native libraries for arm64-v8a and armeabi-v7a, plus Kotlin/UniFFI bindings. Copy the AAR into your project's `libs/` directory and add it as a dependency. See `android/` for a sample Kotlin/Compose app.

## API

| Function | Description |
|----------|-------------|
| `strip_metadata(input)` | Strip metadata with default options (keeps ICC) |
| `strip_metadata_with(input, options)` | Strip metadata with custom options |
| `detect_format(input)` | Detect file format from magic bytes |

**Key types:**

- `RemovalOptions` — granular control over which metadata categories to remove
- `FileFormat` — supported format enum (Jpeg, Png, Webp, Pdf, Docx, Xlsx, Pptx, Mp4, Mp3)
- `Error` — errors (UnsupportedFormat, InvalidData, EncryptedPdf, Io, External)

Full API documentation: [docs.rs/exif_rm](https://docs.rs/exif_rm)

## Contributing

PRs are welcome! By submitting a pull request, you agree that your contributions will be licensed under the MIT License.

## License

This project is licensed under the [MIT License](LICENSE).