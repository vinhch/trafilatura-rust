use super::*;
use encoding_rs::{WINDOWS_1251, WINDOWS_1252};

const SIZE: usize = MAX_FILE_SIZE;

fn load(html: &str) -> Result<Tree, ExtractError> {
    load_html(
        HtmlInput::Str(html),
        &ParseLimits::default(),
        &Call::default(),
    )
}

fn load_bytes(bytes: &[u8]) -> Result<Tree, ExtractError> {
    load_html(
        HtmlInput::Bytes(bytes),
        &ParseLimits::default(),
        &Call::default(),
    )
}

fn repaired(html: &str) -> String {
    let beginning = html.chars().take(50).collect::<String>().to_lowercase();
    repair_faulty_html(html, &beginning).into_owned()
}

#[test]
fn patterns_compile() {
    assert!(DOCTYPE_TAG.is_some());
    assert!(FAULTY_HTML.is_some());
    assert!(INVALID_XML_CHARS.is_some());
    assert!(LOOKS_LIKE_FULL_HTML.is_some());
}

#[test]
fn test_input_detect_encoding() {
    let utf8 = "高山云雾出好茶".as_bytes();
    assert_eq!(detect_encoding(utf8), vec![UTF_8]);
    let (gb, _, _) = encoding_rs::GB18030.encode("高山云雾出好茶");
    let guessed = detect_encoding(&gb.repeat(1000));
    // encoding_rs names the GBK superset-compatible pair separately; either decodes the text.
    assert!(guessed.contains(&encoding_rs::GB18030) || guessed.contains(&encoding_rs::GBK));
}

#[test]
fn test_input_is_dubious_html() {
    assert!(is_dubious_html("This is a string."));
    assert!(!is_dubious_html("<html>"));
}

#[test]
fn test_repair_faulty_html() {
    assert_eq!(
        repaired("<!DOCTYPE html PUBLIC />\n<html></html>"),
        "\n<html></html>"
    );
    assert_eq!(repaired("<html>\n</html>"), "<html>\n</html>");
    assert_eq!(repaired("<html/>\n</html>"), "<html>\n</html>");
    assert_eq!(
        repaired("<!DOCTYPE html>\n<html lang=\"en-US\"/>\n<head/>\n<body/>\n</html>"),
        "<!DOCTYPE html>\n<html lang=\"en-US\">\n<head/>\n<body/>\n</html>"
    );
    // Upstream appends a newline to a doctype-led page that has none.
    assert_eq!(
        repaired("<!DOCTYPE html><html><head></head><body>Foo <br/> Bar</body></html>"),
        "<!DOCTYPE html><html><head></head><body>Foo <br/> Bar</body></html>\n"
    );
}

#[test]
fn test_repair_strips_xml_invalid_chars() {
    let page = "<html><body><p>a\u{0}b\u{1d}c\u{fffe}\u{ffff}d</p>\t<p>keep\tme</p></body></html>";
    assert_eq!(
        repaired(page),
        "<html><body><p>abcd</p>\t<p>keep\tme</p></body></html>"
    );
}

#[test]
fn test_input_load_html() {
    assert!(load("<html><body>ÄÖÜ</body></html>").is_ok());
    assert!(load_bytes(b"<html><body>\x2f\x2e\x9f</body></html>").is_ok());
    let (latin1, _, _) = WINDOWS_1252.encode("<html><body>\u{2f}\u{2e}\u{9f}</body></html>");
    assert!(load_bytes(&latin1).is_ok());
}

#[test]
fn load_html_rejects_dubious_single_block() {
    assert_eq!(
        load("<p>x</p>").unwrap_err(),
        ExtractError::Rejected(Rejection::EmptyTree)
    );
    // lxml returns the lone wrapper element, whose one child is still below the threshold.
    assert!(load("<div><p>x</p></div>").is_err());
    assert!(load("<div><p>x</p><p>y</p></div>").is_ok());
    assert!(load("<p>x</p><p>y</p>").is_ok());
}

#[test]
fn load_html_rejects_blank() {
    for blank in ["", "  \n"] {
        assert_eq!(
            load(blank).unwrap_err(),
            ExtractError::Rejected(Rejection::EmptyTree)
        );
    }
}

#[test]
fn decode_utf8_borrows() {
    assert!(matches!(
        decode_file(b"<html>\xc3\xa4</html>", SIZE),
        Cow::Borrowed("<html>\u{e4}</html>")
    ));
}

#[test]
fn decode_keeps_bom() {
    assert!(decode_file(b"\xef\xbb\xbf<html>", SIZE).starts_with('\u{feff}'));
}

#[test]
fn decode_windows_1251() {
    let text = "Съешь же ещё этих мягких французских булок, да выпей чаю. ".repeat(20);
    let (bytes, _, _) = WINDOWS_1251.encode(&text);
    assert_eq!(decode_file(&bytes, SIZE), text);
}

#[test]
fn decode_mostly_utf8_keeps_valid_text() {
    let text = "Zażółć gęślą jaźń, a potem przeczytaj długi akapit o sprawach wsi. ".repeat(20);
    let mut bytes = text.clone().into_bytes();
    bytes.extend_from_slice(b" caf\xe9 ");
    bytes.extend_from_slice(text.as_bytes());
    assert!(detect_encoding(&bytes).is_empty());
    assert_eq!(
        decode_file(&bytes, SIZE),
        format!("{text} caf\u{fffd} {text}")
    );
}

#[test]
fn mostly_utf8_needs_more_than_ten_valid_chars_per_invalid_sequence() {
    // Each "é" is one valid non-ASCII character; the lone 0xE9 is one invalid sequence.
    let page = |valid: usize| {
        let mut bytes = "é".repeat(valid).into_bytes();
        bytes.extend_from_slice(b"x\xe9x");
        bytes
    };
    assert!(!is_mostly_utf8(&page(10)));
    assert!(detect_encoding(&page(10)).len() == 1);
    assert!(is_mostly_utf8(&page(11)));
    assert!(detect_encoding(&page(11)).is_empty());
}

#[test]
fn decode_falls_back_to_replacement() {
    // A Shift_JIS lead byte before a space is malformed, so strict decoding fails and U+FFFD takes over.
    let junk = b"<p>\x81 ok</p>";
    assert!(
        encoding_rs::SHIFT_JIS
            .decode_without_bom_handling_and_without_replacement(junk)
            .is_none()
    );
    let decoded = decode_with(junk, &[encoding_rs::SHIFT_JIS]);
    assert_eq!(decoded, "<p>\u{fffd} ok</p>");
    assert_eq!(decode_with(junk, &[]), decoded);
    // A later candidate that decodes strictly wins over the fallback.
    assert_eq!(
        decode_with(junk, &[encoding_rs::SHIFT_JIS, WINDOWS_1252]),
        "<p>\u{81} ok</p>"
    );
}

#[test]
fn load_html_full_document_ignores_synthetic_head() {
    for page in [
        "<!DOCTYPE foo><p>x</p>",
        "<!DOCTYPE foo><p>x</p><p>y</p>",
        "<!DOCTYPE foo>text",
    ] {
        assert!(load(page).is_err(), "{page}");
    }
    let late_html = format!("{}<html><body><p>x</p></body></html>", " ".repeat(60));
    assert!(load(&late_html).is_err());
}

#[test]
fn load_html_leading_text_counts_implicit_paragraph() {
    assert!(load("text <p>a</p>").is_ok());
    assert!(load("x<div><p>a</p><p>b</p></div>").is_ok());
    assert!(load("hello <b>x</b> <i>y</i>").is_ok());
    assert!(load("hello <b>x</b>").is_err());
    assert!(load("just text").is_err());
}

#[test]
fn detect_encoding_samples_head_and_tail() {
    let german = "Größe über Äpfel und Öl, schön gemäß Maßstab. ".repeat(200);
    let (german, _, _) = WINDOWS_1252.encode(&german);
    let cyrillic = "Съешь же ещё этих мягких французских булок. ".repeat(400);
    let (cyrillic, _, _) = WINDOWS_1251.encode(&cyrillic);
    let head = german.get(..5000).unwrap();
    let tail = german.get(german.len() - 5000..).unwrap();
    let mut content = head.to_vec();
    content.extend_from_slice(cyrillic.get(..10_000).unwrap());
    content.extend_from_slice(tail);

    let mut sample = EncodingDetector::new(Iso2022JpDetection::Deny);
    sample.feed(head, false);
    sample.feed(tail, true);
    let mut whole = EncodingDetector::new(Iso2022JpDetection::Deny);
    whole.feed(&content, true);

    let expected = sample.guess(None, Utf8Detection::Deny);
    assert_ne!(
        expected,
        whole.guess(None, Utf8Detection::Deny),
        "the middle must change the whole-input guess"
    );
    assert_eq!(detect_encoding(&content), vec![expected]);
}

#[cfg(feature = "decompress")]
mod decompress {
    use std::io::Write;

    use super::*;

    const HTML: &str = "<html><head/><body><div>ABC</div></body></html>";

    // brotli.compress(b"<html>a</html>")
    const BROTLI_HTML: &[u8] = &[
        0x8b, 0x06, 0x80, 0x3c, 0x68, 0x74, 0x6d, 0x6c, 0x3e, 0x61, 0x3c, 0x2f, 0x68, 0x74, 0x6d,
        0x6c, 0x3e, 0x03,
    ];
    // brotli.compress(b"0" * 100_000)
    const BROTLI_ZEROS: &[u8] = &[
        0x5b, 0x9f, 0x86, 0x81, 0x5f, 0x02, 0x26, 0x1e, 0x0b, 0x04, 0xb2, 0xfc, 0x02, 0x00,
    ];

    fn gzip(data: &[u8]) -> Vec<u8> {
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn zlib(data: &[u8]) -> Vec<u8> {
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(data).unwrap();
        encoder.finish().unwrap()
    }

    fn zstd(data: &[u8]) -> Vec<u8> {
        ruzstd::encoding::compress_to_vec(data, ruzstd::encoding::CompressionLevel::Fastest)
    }

    #[test]
    fn test_decode() {
        assert!(!decode_file(b" ", SIZE).is_empty());
        for compressed in [
            gzip(HTML.as_bytes()),
            zlib(HTML.as_bytes()),
            zstd(HTML.as_bytes()),
        ] {
            assert_eq!(
                handle_compressed_file(&compressed, SIZE).as_ref(),
                HTML.as_bytes()
            );
            assert_eq!(decode_file(&compressed, SIZE), HTML);
        }
        assert_eq!(
            handle_compressed_file(BROTLI_HTML, SIZE).as_ref(),
            b"<html>a</html>"
        );
    }

    #[test]
    fn decompress_gzip_and_cap() {
        let bomb = gzip(&vec![b'0'; 25_000_000]);
        assert_eq!(
            handle_compressed_file(&bomb, SIZE).as_ref(),
            bomb.as_slice()
        );
        assert_eq!(handle_compressed_file(&bomb, 30_000_000).len(), 25_000_000);
        // Exactly at the cap is accepted, one over is not.
        let data = gzip(&[b'a'; 100]);
        assert_eq!(handle_compressed_file(&data, 100).len(), 100);
        assert_eq!(handle_compressed_file(&data, 99).as_ref(), data.as_slice());
    }

    #[test]
    fn decompress_keeps_corrupt_input() {
        let whole = zstd(&HTML.as_bytes().repeat(500));
        let half = whole.get(..whole.len() / 2).unwrap();
        let small = zstd(HTML.as_bytes());
        let cut = small.get(..small.len() - 4).unwrap();
        for bad in [
            "äöüß".as_bytes(),
            b"\x1f\x8b\x08abc",
            b"\x28\xb5\x2f\xfdabc",
            half,
            cut,
        ] {
            assert_eq!(handle_compressed_file(bad, SIZE).as_ref(), bad);
        }
    }

    #[test]
    fn decompress_multi_member_and_padding() {
        let multi = [gzip(b"<html>part1 "), gzip(b"part2</html>")].concat();
        assert_eq!(
            handle_compressed_file(&multi, SIZE).as_ref(),
            b"<html>part1 part2</html>"
        );
        let padded = [gzip(b"<html>a "), vec![0; 8], gzip(b"b</html>"), vec![0; 8]].concat();
        assert_eq!(
            handle_compressed_file(&padded, SIZE).as_ref(),
            b"<html>a b</html>"
        );
        let frames = [zstd(b"<html>a "), zstd(b"b</html>")].concat();
        assert_eq!(
            handle_compressed_file(&frames, SIZE).as_ref(),
            b"<html>a b</html>"
        );
    }

    #[test]
    fn decompress_member_flood() {
        let member = gzip(b"a");
        let flood = member.repeat(MAX_MEMBERS + 1);
        assert_eq!(
            handle_compressed_file(&flood, SIZE).as_ref(),
            flood.as_slice()
        );
        let limit = member.repeat(MAX_MEMBERS);
        assert_eq!(
            handle_compressed_file(&limit, SIZE).as_ref(),
            vec![b'a'; MAX_MEMBERS]
        );
    }

    #[test]
    fn decompress_zstd_and_brotli_bombs() {
        let bomb = zstd(&vec![b'0'; 25_000_000]);
        assert_eq!(
            handle_compressed_file(&bomb, SIZE).as_ref(),
            bomb.as_slice()
        );
        assert_eq!(
            handle_compressed_file(BROTLI_ZEROS, 1000).as_ref(),
            BROTLI_ZEROS
        );
        assert_eq!(handle_compressed_file(BROTLI_ZEROS, SIZE).len(), 100_000);
    }
}
