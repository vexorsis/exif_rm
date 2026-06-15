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
        let mut pos;

        // Copy header (6 bytes)
        out.extend_from_slice(&input[0..6]);
        pos = 6;

        // Copy Logical Screen Descriptor (7 bytes)
        if pos + 7 > input.len() {
            return Err(Error::InvalidData("Truncated LSD".into()));
        }
        out.extend_from_slice(&input[pos..pos + 7]);
        let packed = input[pos + 4];
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
                    pos = copy_sub_blocks(input, pos, &mut out)?;
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
                                pos = copy_sub_blocks(input, pos, &mut out)?;
                            } else {
                                // Strip: skip app id + sub-blocks
                                pos += 11;
                                pos = skip_sub_blocks(input, pos)?;
                            }
                        }

                        // Comment Extension — strip
                        0xFE => {
                            pos = skip_sub_blocks(input, pos)?;
                        }

                        // Plain Text Extension — strip
                        0x01 => {
                            pos = skip_sub_blocks(input, pos)?;
                        }

                        // Unknown extension — preserve (safe default)
                        _ => {
                            out.push(0x21);
                            out.push(label);
                            pos = copy_sub_blocks(input, pos, &mut out)?;
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
        gif.push(0x0C); // sub-block size = 12
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
