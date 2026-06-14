use std::io::Read;
use exif_rm::{strip_metadata, strip_metadata_owned, detect_format, RemovalOptions, FileFormat};

// --- JPEG tests ---

fn create_jpeg_with_exif() -> Vec<u8> {
    let mut jpeg = vec![0xFF, 0xD8]; // SOI
    // APP1 with Exif header
    let exif_data = b"Exif\x00\x00MM\x00\x2a\x00\x00\x00\x08\x00\x01\x01\x12\x00\x03\x00\x00\x00\x01\x00\x01\x00\x00\x00\x00\x00\x00";
    let seg_len = (exif_data.len() + 2) as u16;
    jpeg.push(0xFF);
    jpeg.push(0xE1);
    jpeg.extend_from_slice(&seg_len.to_be_bytes());
    jpeg.extend_from_slice(exif_data);
    // DQT (minimal)
    jpeg.extend_from_slice(&[0xFF, 0xDB, 0x00, 0x43]);
    jpeg.extend_from_slice(&[0u8; 65]);
    // SOF0 (minimal 1x1)
    jpeg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x0B, 0x08, 0x00, 0x01, 0x00, 0x01, 0x01, 0x01, 0x11, 0x00]);
    // DHT (minimal)
    jpeg.extend_from_slice(&[0xFF, 0xC4, 0x00, 0x1F, 0x00]);
    jpeg.extend_from_slice(&[0u8; 28]);
    // SOS + data + EOI
    jpeg.extend_from_slice(&[0xFF, 0xDA, 0x00, 0x08, 0x01, 0x01, 0x00, 0x00, 0x3F, 0x00]);
    jpeg.extend_from_slice(&[0x7F]); // fake entropy data
    jpeg.extend_from_slice(&[0xFF, 0xD9]); // EOI
    jpeg
}

#[test]
fn test_jpeg_strip_removes_exif() {
    let input = create_jpeg_with_exif();
    let output = strip_metadata(&input).unwrap();
    // Should not contain APP1 marker (0xFF 0xE1)
    assert!(!output.windows(2).any(|w| w == &[0xFF, 0xE1]));
}

#[test]
fn test_jpeg_format_detection() {
    let input = create_jpeg_with_exif();
    let format = detect_format(&input).unwrap();
    assert_eq!(format, FileFormat::Jpeg);
}

#[test]
fn test_jpeg_keep_dqt_sof() {
    let input = create_jpeg_with_exif();
    let output = strip_metadata(&input).unwrap();
    // DQT and SOF0 markers should be preserved
    assert!(output.windows(2).any(|w| w == &[0xFF, 0xDB]));
    assert!(output.windows(2).any(|w| w == &[0xFF, 0xC0]));
}

// --- PNG tests ---

fn create_png_with_text() -> Vec<u8> {
    let mut png = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A]; // signature
    // IHDR chunk: 1x1 8-bit grayscale
    let ihdr_data: &[u8] = &[0, 0, 0, 1, 0, 0, 0, 1, 8, 0, 0, 0, 0];
    png.extend_from_slice(&(ihdr_data.len() as u32).to_be_bytes());
    png.extend_from_slice(b"IHDR");
    png.extend_from_slice(ihdr_data);
    let mut crc_input = b"IHDR".to_vec();
    crc_input.extend_from_slice(ihdr_data);
    let crc = crc32fast::hash(&crc_input);
    png.extend_from_slice(&crc.to_be_bytes());
    // tEXt chunk with metadata
    let text_data = b"Author\x00Test Author";
    png.extend_from_slice(&(text_data.len() as u32).to_be_bytes());
    png.extend_from_slice(b"tEXt");
    png.extend_from_slice(text_data);
    let mut crc_input = b"tEXt".to_vec();
    crc_input.extend_from_slice(text_data);
    let crc = crc32fast::hash(&crc_input);
    png.extend_from_slice(&crc.to_be_bytes());
    // IDAT chunk (minimal 1x1 grayscale pixel)
    let idat_data: &[u8] = &[0x08, 0xD7, 0x63, 0xF8, 0xCF, 0xC0, 0x00, 0x00, 0x00, 0x02, 0x00, 0x01];
    png.extend_from_slice(&(idat_data.len() as u32).to_be_bytes());
    png.extend_from_slice(b"IDAT");
    png.extend_from_slice(idat_data);
    let mut crc_input = b"IDAT".to_vec();
    crc_input.extend_from_slice(idat_data);
    let crc = crc32fast::hash(&crc_input);
    png.extend_from_slice(&crc.to_be_bytes());
    // IEND chunk
    png.extend_from_slice(&0u32.to_be_bytes());
    png.extend_from_slice(b"IEND");
    let crc = crc32fast::hash(b"IEND");
    png.extend_from_slice(&crc.to_be_bytes());
    png
}

#[test]
fn test_png_strip_removes_text() {
    let input = create_png_with_text();
    let output = strip_metadata(&input).unwrap();
    // Should not contain tEXt chunk type
    assert!(!output.windows(4).any(|w| w == b"tEXt"));
}

#[test]
fn test_png_format_detection() {
    let input = create_png_with_text();
    let format = detect_format(&input).unwrap();
    assert_eq!(format, FileFormat::Png);
}

#[test]
fn test_png_preserves_ihdr_idat_iend() {
    let input = create_png_with_text();
    let output = strip_metadata(&input).unwrap();
    assert!(output.windows(4).any(|w| w == b"IHDR"));
    assert!(output.windows(4).any(|w| w == b"IDAT"));
    assert!(output.windows(4).any(|w| w == b"IEND"));
}

// --- PDF tests ---

fn create_minimal_pdf() -> Vec<u8> {
    use lopdf::Document as LopdfDoc;
    use lopdf::{Dictionary, Object};

    let mut doc = LopdfDoc::new();

    // Create a pages object
    let pages_id = doc.add_object(Object::Dictionary(Dictionary::from_iter([
        (b"Type".to_vec(), Object::Name(b"Pages".to_vec())),
        (b"Kids".to_vec(), Object::Array(vec![])),
        (b"Count".to_vec(), Object::Integer(0)),
    ])));

    // Create a catalog
    let catalog_id = doc.add_object(Object::Dictionary(Dictionary::from_iter([
        (b"Type".to_vec(), Object::Name(b"Catalog".to_vec())),
        (b"Pages".to_vec(), Object::Reference(pages_id)),
    ])));

    // Create an Info dictionary with metadata
    let info_id = doc.add_object(Object::Dictionary(Dictionary::from_iter([
        (b"Title".to_vec(), Object::String(b"Test Document".to_vec(), lopdf::StringFormat::Literal)),
        (b"Author".to_vec(), Object::String(b"Test Author".to_vec(), lopdf::StringFormat::Literal)),
    ])));

    doc.trailer.set(b"Root", Object::Reference(catalog_id));
    doc.trailer.set(b"Info", Object::Reference(info_id));

    let mut output = Vec::new();
    doc.save_to(&mut output).unwrap();
    output
}

#[test]
fn test_pdf_strip_removes_info() {
    let input = create_minimal_pdf();
    let output = strip_metadata(&input).unwrap();
    // The stripped PDF should not contain the Info dict string values
    let output_str = String::from_utf8_lossy(&output);
    assert!(!output_str.contains("Test Document"));
    assert!(!output_str.contains("Test Author"));
}

#[test]
fn test_pdf_format_detection() {
    let input = create_minimal_pdf();
    let format = detect_format(&input).unwrap();
    assert_eq!(format, FileFormat::Pdf);
}

// --- Office (DOCX) tests ---

fn create_minimal_docx() -> Vec<u8> {
    use std::io::Write;
    let buf = Vec::new();
    let cursor = std::io::Cursor::new(buf);
    let mut zip = zip::ZipWriter::new(cursor);
    let opts = zip::write::SimpleFileOptions::default();

    // [Content_Types].xml
    zip.start_file("[Content_Types].xml", opts).unwrap();
    zip.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>
<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\">
  <Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/>
  <Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/>
  <Override PartName=\"/docProps/core.xml\" ContentType=\"application/vnd.openxmlformats-package.core-properties+xml\"/>
  <Override PartName=\"/docProps/app.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.extended-properties+xml\"/>
</Types>").unwrap();

    // _rels/.rels
    zip.start_file("_rels/.rels", opts).unwrap();
    zip.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>
<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">
  <Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/>
  <Relationship Id=\"rId2\" Type=\"http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties\" Target=\"docProps/core.xml\"/>
</Relationships>").unwrap();

    // word/document.xml
    zip.start_file("word/document.xml", opts).unwrap();
    zip.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>
<w:document xmlns:w=\"http://schemas.openxmlformats.org/wordprocessingml/2006/main\">
  <w:body><w:p><w:r><w:t>Hello World</w:t></w:r></w:p></w:body>
</w:document>").unwrap();

    // docProps/core.xml
    zip.start_file("docProps/core.xml", opts).unwrap();
    zip.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>
<cp:coreProperties xmlns:cp=\"http://schemas.openxmlformats.org/package/2006/metadata/core-properties\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">
  <dc:creator>Test Author</dc:creator>
  <dc:title>Test Document</dc:title>
  <dcterms:created xmlns:dcterms=\"http://purl.org/dc/terms/\">2024-01-01T00:00:00Z</dcterms:created>
</cp:coreProperties>").unwrap();

    // docProps/app.xml
    zip.start_file("docProps/app.xml", opts).unwrap();
    zip.write_all(b"<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>
<Properties xmlns=\"http://schemas.openxmlformats.org/officeDocument/2006/extended-properties\">
  <Application>Microsoft Office Word</Application>
  <AppVersion>16.0000</AppVersion>
</Properties>").unwrap();

    zip.finish().unwrap().into_inner()
}

#[test]
fn test_docx_strip_removes_properties() {
    let input = create_minimal_docx();
    let output = strip_metadata(&input).unwrap();

    // Verify that the metadata values are actually gone
    let mut dst = zip::ZipArchive::new(std::io::Cursor::new(&output)).unwrap();
    let mut core = String::new();
    dst.by_name("docProps/core.xml").unwrap().read_to_string(&mut core).unwrap();
    assert!(!core.contains("Test Author"));
    assert!(!core.contains("Test Document"));
}

#[test]
fn test_docx_format_detection() {
    let input = create_minimal_docx();
    let format = detect_format(&input).unwrap();
    assert_eq!(format, FileFormat::Docx);
}

#[test]
fn test_docx_preserves_document_content() {
    let input = create_minimal_docx();
    let output = strip_metadata(&input).unwrap();

    // Verify word/document.xml is preserved
    let mut src = zip::ZipArchive::new(std::io::Cursor::new(&input)).unwrap();
    let mut dst = zip::ZipArchive::new(std::io::Cursor::new(&output)).unwrap();

    let mut src_doc = String::new();
    src.by_name("word/document.xml").unwrap().read_to_string(&mut src_doc).unwrap();
    let mut dst_doc = String::new();
    dst.by_name("word/document.xml").unwrap().read_to_string(&mut dst_doc).unwrap();
    assert_eq!(src_doc, dst_doc);
}

#[test]
fn test_docx_core_xml_is_sanitized() {
    let input = create_minimal_docx();
    let output = strip_metadata(&input).unwrap();

    let mut dst = zip::ZipArchive::new(std::io::Cursor::new(&output)).unwrap();
    let mut core = String::new();
    dst.by_name("docProps/core.xml").unwrap().read_to_string(&mut core).unwrap();

    // Should not contain the original author/title values
    assert!(!core.contains("Test Author"));
    assert!(!core.contains("Test Document"));
    // But should still have the XML structure
    assert!(core.contains("coreProperties"));
}

// --- Format detection edge cases ---

#[test]
fn test_unsupported_format() {
    let result = detect_format(b"random bytes that are not a file");
    assert!(result.is_err());
}

#[test]
fn test_empty_input() {
    let result = detect_format(&[]);
    assert!(result.is_err());
}

// --- UniFFI-compatible owned API tests ---

#[test]
fn test_strip_metadata_owned() {
    let input = create_jpeg_with_exif();
    let output = strip_metadata_owned(input.clone()).unwrap();
    // Should not contain APP1 marker
    assert!(!output.windows(2).any(|w| w == &[0xFF, 0xE1]));
}

// --- RemovalOptions tests ---

#[test]
fn test_keep_icc_by_default() {
    let options = RemovalOptions::default();
    assert!(!options.icc_profile);
}

#[test]
fn test_strip_all_includes_icc() {
    let options = RemovalOptions::all();
    assert!(options.icc_profile);
}

// --- MP4 tests ---

fn create_minimal_mp4() -> Vec<u8> {
    let mut mp4 = Vec::new();

    // Helper: build an ISOBMFF box
    let make_box = |box_type: &[u8], content: &[u8]| -> Vec<u8> {
        let size = (8 + content.len()) as u32;
        let mut buf = size.to_be_bytes().to_vec();
        buf.extend_from_slice(box_type);
        buf.extend_from_slice(content);
        buf
    };

    // Helper: build a fullbox (box with version+flags prefix)
    let make_fullbox = |box_type: &[u8], version: u8, flags: u32, content: &[u8]| -> Vec<u8> {
        let vf = ((version as u32) << 24) | flags;
        let mut full_content = vf.to_be_bytes().to_vec();
        full_content.extend_from_slice(content);
        make_box(box_type, &full_content)
    };

    // ftyp box: major_brand=isom, minor_version=0, compatible_brands=isom,iso2,avc1
    let mut ftyp_content = b"isom".to_vec();
    ftyp_content.extend_from_slice(&0u32.to_be_bytes()); // minor_version
    ftyp_content.extend_from_slice(b"isom");
    ftyp_content.extend_from_slice(b"iso2");
    ftyp_content.extend_from_slice(b"avc1");
    mp4.extend_from_slice(&make_box(b"ftyp", &ftyp_content));

    // mvhd box (version 0) with non-zero timestamps
    let mut mvhd_content = Vec::new();
    mvhd_content.extend_from_slice(&3788000000u32.to_be_bytes()); // creation_time (2024-01-13)
    mvhd_content.extend_from_slice(&3788000001u32.to_be_bytes()); // modification_time (2024-01-13)
    mvhd_content.extend_from_slice(&1000u32.to_be_bytes());       // timescale
    mvhd_content.extend_from_slice(&0u32.to_be_bytes());          // duration
    mvhd_content.extend_from_slice(&0x00010000u32.to_be_bytes()); // rate = 1.0
    mvhd_content.extend_from_slice(&0x0100u16.to_be_bytes());     // volume = 1.0
    mvhd_content.extend_from_slice(&[0u8; 10]);                   // reserved
    // matrix (36 bytes): identity
    mvhd_content.extend_from_slice(&0x00010000u32.to_be_bytes()); // a
    mvhd_content.extend_from_slice(&0u32.to_be_bytes());          // b
    mvhd_content.extend_from_slice(&0u32.to_be_bytes());          // u
    mvhd_content.extend_from_slice(&0u32.to_be_bytes());          // c
    mvhd_content.extend_from_slice(&0x00010000u32.to_be_bytes()); // d
    mvhd_content.extend_from_slice(&0u32.to_be_bytes());          // v
    mvhd_content.extend_from_slice(&0u32.to_be_bytes());          // x
    mvhd_content.extend_from_slice(&0u32.to_be_bytes());          // y
    mvhd_content.extend_from_slice(&0x40000000u32.to_be_bytes()); // w
    mvhd_content.extend_from_slice(&[0u8; 24]);                   // pre_defined
    mvhd_content.extend_from_slice(&2u32.to_be_bytes());          // next_track_id
    let mvhd = make_fullbox(b"mvhd", 0, 0, &mvhd_content);

    // hdlr box (handler for iTunes metadata)
    let mut hdlr_content = Vec::new();
    hdlr_content.extend_from_slice(&0u32.to_be_bytes()); // pre_defined
    hdlr_content.extend_from_slice(b"mdir");             // handler_type
    hdlr_content.extend_from_slice(&[0u8; 12]);          // reserved
    hdlr_content.push(0);                                // name (null-terminated)
    let hdlr = make_fullbox(b"hdlr", 0, 0, &hdlr_content);

    // ilst box: ©nam with title "Test Title"
    let title = b"Test Title";
    let mut data_content = Vec::new();
    data_content.extend_from_slice(&1u32.to_be_bytes());  // type_indicator = 1 (UTF-8)
    data_content.extend_from_slice(&0u32.to_be_bytes());  // locale = 0
    data_content.extend_from_slice(title);
    let data_box = make_box(b"data", &data_content);
    let nam_box = make_box(b"\xa9nam", &data_box);
    let ilst = make_box(b"ilst", &nam_box);

    // meta box (fullbox containing hdlr + ilst)
    let mut meta_inner = Vec::new();
    meta_inner.extend_from_slice(&hdlr);
    meta_inner.extend_from_slice(&ilst);
    let meta = make_fullbox(b"meta", 0, 0, &meta_inner);

    // udta box
    let udta = make_box(b"udta", &meta);

    // moov box
    let mut moov_content = Vec::new();
    moov_content.extend_from_slice(&mvhd);
    moov_content.extend_from_slice(&udta);
    mp4.extend_from_slice(&make_box(b"moov", &moov_content));

    // mdat box (empty)
    mp4.extend_from_slice(&make_box(b"mdat", &[]));

    mp4
}

#[test]
fn test_mp4_format_detection() {
    let input = create_minimal_mp4();
    let format = detect_format(&input).unwrap();
    assert_eq!(format, FileFormat::Mp4);
}

#[test]
fn test_mp4_strip_removes_metadata() {
    let input = create_minimal_mp4();
    let output = strip_metadata(&input).unwrap();

    // udta box should be removed (it contained the metadata)
    assert!(!output.windows(4).any(|w| w == b"udta"), "udta should be removed");
    // meta box should be removed
    assert!(!output.windows(4).any(|w| w == b"meta"), "meta should be removed");
}

#[test]
fn test_mp4_preserves_ftyp_mdat() {
    let input = create_minimal_mp4();
    let output = strip_metadata(&input).unwrap();

    // ftyp box should be preserved
    assert_eq!(&output[4..8], b"ftyp");
    // mdat box should be preserved
    assert!(output.windows(4).any(|w| w == b"mdat"), "mdat box should be preserved");
}

#[test]
fn test_mp4_strip_output_is_valid_mp4() {
    let input = create_minimal_mp4();
    let output = strip_metadata(&input).unwrap();

    // Output should still be a valid MP4 (ftyp at offset 4)
    assert!(output.len() >= 8, "output should be at least 8 bytes");
    assert_eq!(&output[4..8], b"ftyp", "ftyp should be at offset 4");
}

#[test]
fn test_mp4_strip_preserves_mvhd() {
    let input = create_minimal_mp4();
    let output = strip_metadata(&input).unwrap();

    // mvhd should still be present inside moov
    assert!(output.windows(4).any(|w| w == b"mvhd"), "mvhd should be preserved");
    // moov should still be present
    assert!(output.windows(4).any(|w| w == b"moov"), "moov should be preserved");
}

#[test]
fn test_mp4_strip_removes_udta() {
    let input = create_minimal_mp4();
    let output = strip_metadata(&input).unwrap();

    // udta box should be removed
    assert!(!output.windows(4).any(|w| w == b"udta"), "udta should be removed");
}

// --- WebP tests ---

fn create_minimal_webp() -> Vec<u8> {
    let vp8_payload: &[u8] = &[
        0x9D, 0x01, 0x2A,
        0x00, 0x01, 0x00, 0x01,
        0x00, 0x02, 0x00, 0x02,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    ];
    let vp8_chunk_size = vp8_payload.len() as u32;
    let riff_size = 4 + 8 + vp8_chunk_size;
    let mut webp = Vec::new();
    webp.extend_from_slice(b"RIFF");
    webp.extend_from_slice(&riff_size.to_le_bytes());
    webp.extend_from_slice(b"WEBP");
    webp.extend_from_slice(b"VP8 ");
    webp.extend_from_slice(&vp8_chunk_size.to_le_bytes());
    webp.extend_from_slice(vp8_payload);
    webp
}

#[test]
fn test_webp_format_detection() {
    let input = create_minimal_webp();
    let format = detect_format(&input).unwrap();
    assert_eq!(format, FileFormat::Webp);
}