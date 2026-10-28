use super::*;
use encoding_rs::{WINDOWS_1251, WINDOWS_1252};

use crate::settings::{DEFAULT_MAX_INPUT_BYTES, MAX_FILE_SIZE};

const SIZE: usize = MAX_FILE_SIZE;

fn load(html: &str) -> Result<Tree, ExtractError> {
    load_html(
        HtmlInput::Str(html),
        SIZE,
        &ParseLimits::default(),
        &Call::default(),
    )
}

fn load_bytes(bytes: &[u8]) -> Result<Tree, ExtractError> {
    load_html(
        HtmlInput::Bytes(bytes),
        SIZE,
        &ParseLimits::default(),
        &Call::default(),
    )
}

fn capped(max: usize) -> ParseLimits {
    ParseLimits {
        max_input_bytes: Some(max),
        ..ParseLimits::default()
    }
}

#[test]
fn load_html_rejects_input_over_the_input_cap() {
    // deviation: D-11
    let page = "<html><body><p>a</p><p>b</p></body></html>";
    let len = page.len();
    let call = Call::default();
    assert!(load_html(HtmlInput::Str(page), SIZE, &capped(len), &call).is_ok());
    assert!(load_html(HtmlInput::Bytes(page.as_bytes()), SIZE, &capped(len), &call).is_ok());
    for input in [HtmlInput::Str(page), HtmlInput::Bytes(page.as_bytes())] {
        assert!(matches!(
            load_html(input, SIZE, &capped(len - 1), &call),
            Err(ExtractError::Rejected(Rejection::InputTooLarge))
        ));
    }
    let unlimited = ParseLimits {
        max_input_bytes: None,
        ..ParseLimits::default()
    };
    assert!(load_html(HtmlInput::Str(page), SIZE, &unlimited, &call).is_ok());
}

#[test]
fn default_input_cap_is_64_mib() {
    assert_eq!(DEFAULT_MAX_INPUT_BYTES, 64 * 1024 * 1024);
    assert_eq!(
        ParseLimits::default().max_input_bytes,
        Some(DEFAULT_MAX_INPUT_BYTES)
    );
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
fn has_invalid_xml_char_matches_the_regex() {
    let regex = INVALID_XML_CHARS.as_ref().expect("pattern compiles");
    for c in (0..=0xFFFF)
        .chain([0x10000, 0x10FFFF])
        .filter_map(char::from_u32)
    {
        let text = format!("a{c}b");
        assert_eq!(has_invalid_xml_char(&text), regex.is_match(&text), "{c:?}");
    }
}

#[test]
fn repair_faulty_html_borrows_a_clean_doctype_page() {
    let page = "<!DOCTYPE html>\n<html></html>";
    let repaired = repair_faulty_html(page, "<!doctype html>\n<html></html>");
    assert!(matches!(repaired, Cow::Borrowed(_)));
    assert_eq!(repaired, page);
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
fn load_html_counts_ins_del_noscript_as_inline() {
    // lxml 6 (libxml2) gives `len == 1` for each of these, so they are rejected like `t <b>a</b>`;
    // a real block element makes the implied `p` a second child.
    for html in [
        "t <ins>a</ins>",
        "t <del>a</del>",
        "t <noscript>a</noscript>",
    ] {
        assert_eq!(
            load(html).unwrap_err(),
            ExtractError::Rejected(Rejection::EmptyTree),
            "{html}"
        );
    }
    assert!(load("t <div>a</div>").is_ok());
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

fn binary_sample(magic: &[u8]) -> Vec<u8> {
    let mut out = magic.to_vec();
    out.extend((0..16_384u32).map(|i| ((i * 7919 + 13) % 251) as u8));
    out
}

// gzip header with the FNAME flag and the file name `name`
fn gzip_named_sample(name: &[u8]) -> Vec<u8> {
    let mut magic = vec![0x1f, 0x8b, 0x08, 0x08, 1, 2, 3, 4, 0x00, 0x03];
    magic.extend(name);
    magic.push(0);
    binary_sample(&magic)
}

// zip local file header whose name field is `name`
fn zip_named_sample(name: &[u8]) -> Vec<u8> {
    let mut magic = b"PK\x03\x04\x14\x00".to_vec();
    magic.extend([0; 20]);
    magic.extend([10, 0, 0, 0]);
    magic.extend(name);
    binary_sample(&magic)
}

const BINARY_MAGICS: [&[u8]; 13] = [
    b"%PDF-",
    b"PK\x03\x04",
    b"%PDF-1.4\n",
    b"%PDF-1.7\n",
    b"\x89PNG\r\n\x1a\n",
    b"\xff\xd8\xff\xdb",
    b"\xff\xd8\xff\xe0",
    b"\xff\xd8\xff\xe1",
    b"GIF87a",
    b"GIF89a",
    b"RIFF\x00\x00\x00\x00WEBPVP8 ",
    b"\x1f\x8b\x08",
    b"\x28\xb5\x2f\xfd",
];

fn no_parse() -> ParseLimits {
    // max_nodes 0 makes any parse fail with TreeTooLarge, so EmptyTree proves no parse ran
    ParseLimits {
        max_nodes: Some(0),
        ..ParseLimits::default()
    }
}

#[test]
fn binary_input_is_rejected_before_parsing() {
    // deviation: D-12
    for magic in BINARY_MAGICS {
        assert!(
            matches!(
                load_html(
                    HtmlInput::Bytes(&binary_sample(magic)),
                    SIZE,
                    &no_parse(),
                    &Call::default()
                ),
                Err(ExtractError::Rejected(Rejection::EmptyTree))
            ),
            "{magic:?}"
        );
    }
}

#[test]
fn binary_samples_give_upstream_none() {
    // oracle: upstream load_html returns None for each of these samples (scripts/oracle.sh)
    for magic in BINARY_MAGICS {
        assert!(
            matches!(
                load_bytes(&binary_sample(magic)),
                Err(ExtractError::Rejected(Rejection::EmptyTree))
            ),
            "{magic:?}"
        );
    }
}

#[test]
fn binary_with_an_html_name_is_parsed() {
    // oracle: a header naming `index.html` puts `html` in upstream's first 50 characters (it
    // lowercases them), so load_html returns a tree for each sample, `INDEX.HTML` included
    for name in [&b"index.html"[..], b"INDEX.HTML"] {
        for sample in [gzip_named_sample(name), zip_named_sample(name)] {
            assert!(matches!(
                load_html(
                    HtmlInput::Bytes(&sample),
                    SIZE,
                    &no_parse(),
                    &Call::default()
                ),
                Err(ExtractError::Rejected(Rejection::TreeTooLarge))
            ));
        }
    }
}

#[test]
fn text_with_a_binary_magic_prefix_is_rejected() {
    // oracle: upstream load_html returns a tree of 2 children for this input, which has no `html`
    // in its first 50 characters; the shortcut rejects it
    assert!(matches!(
        load_bytes(b"GIF89a<p>a</p><p>b</p>"),
        Err(ExtractError::Rejected(Rejection::EmptyTree))
    ));
}

#[test]
fn text_that_looks_binary_is_parsed() {
    let call = Call::default();
    // Str input never takes the shortcut
    assert!(matches!(
        load_html(
            HtmlInput::Str("%PDF-1.4\n<html><body><p>a</p></body></html>"),
            SIZE,
            &no_parse(),
            &call
        ),
        Err(ExtractError::Rejected(Rejection::TreeTooLarge))
    ));
    // a BOM page and leading whitespace are HTML
    for page in [
        &b"\xef\xbb\xbf<html><body><p>a</p></body></html>"[..],
        b"  \n<html><body><p>a</p></body></html>",
    ] {
        assert!(matches!(
            load_html(HtmlInput::Bytes(page), SIZE, &no_parse(), &call),
            Err(ExtractError::Rejected(Rejection::TreeTooLarge))
        ));
    }
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

    #[test]
    fn input_cap_reads_the_raw_length_not_the_decompressed_one() {
        // gzip of a page larger than the cap, itself smaller than the cap
        let page = format!("<html><body>{}</body></html>", "<p>word</p>".repeat(2_000));
        let compressed = gzip(page.as_bytes());
        assert!(compressed.len() < page.len());
        let limits = capped(compressed.len());
        assert!(
            load_html(
                HtmlInput::Bytes(&compressed),
                SIZE,
                &limits,
                &Call::default()
            )
            .is_ok()
        );
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
    fn compressed_html_is_not_binary() {
        // deviation: D-12 (the gzip magic is checked after decompression)
        let page = "<html><body><p>one</p><p>two</p></body></html>";
        let tree = load_bytes(&gzip(page.as_bytes())).unwrap();
        assert_eq!(tree.iter(tree.root(), &["p"]).count(), 2);
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

// The text helpers. Expected values come from upstream Python (`scripts/oracle.sh`); the oracle
// comment names the expression.
mod text {
    use std::borrow::Cow;
    use std::collections::HashSet;

    use super::*;
    use crate::context::Ctx;
    use crate::dom::parse_fragment;
    use crate::settings::{Config, Extractor, Format};

    fn tree(html: &str) -> Tree {
        parse_document(html, &ParseLimits::default(), &Call::default()).expect("parses")
    }

    /// `textfilter` of a `<p>` with this text and tail.
    fn filtered(text: Option<&str>, tail: Option<&str>) -> bool {
        let mut tree =
            parse_fragment("<p></p>", &ParseLimits::default(), &Call::default()).expect("parses");
        let p = tree.children(tree.root()).next().expect("p");
        let node = tree.node_mut(p);
        node.text = text.map(String::from);
        node.tail = tail.map(String::from);
        textfilter(&tree, p)
    }

    type Row = (
        String,
        Vec<(String, String)>,
        Option<String>,
        Option<String>,
    );

    fn row(tag: &str, attrs: &[(&str, &str)], text: Option<&str>, tail: Option<&str>) -> Row {
        (
            tag.to_string(),
            attrs
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            text.map(String::from),
            tail.map(String::from),
        )
    }

    /// Every element of `html` after `sanitize_tree`, as (tag, attributes, text, tail). The
    /// empty `head` html5ever adds is left out: libxml2 (the oracle) does not make one (D-2).
    fn sanitized(html: &str) -> Vec<Row> {
        let mut tree = tree(&format!("<html><body>{html}</body></html>"));
        let root = tree.root();
        sanitize_tree(&mut tree, root, &Call::default()).expect("not cancelled");
        tree.iter(root, &[])
            .filter(|&id| &*tree.node(id).name != "head")
            .map(|id| {
                let node = tree.node(id);
                (
                    node.name.to_string(),
                    node.attrs
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                    node.text.clone(),
                    node.tail.clone(),
                )
            })
            .collect()
    }

    #[test]
    fn text_patterns_compile() {
        assert!(RE_FILTER.is_some());
        assert!(IMAGE_EXTENSION.is_some());
        assert!(RE_HTML_LANG.is_some());
    }

    #[test]
    fn trim_uses_python_whitespace() {
        // oracle: trim("\tTest  "), trim("\t\tTest  Test\r\n")
        assert_eq!(trim("\tTest  "), "Test");
        assert_eq!(trim("\t\tTest  Test\r\n"), "Test Test");
        // oracle: trim("a\x1cb\xa0c" + chr(0x200b) + "d")
        assert_eq!(trim("a\x1cb\u{a0}c\u{200b}d"), "a b c\u{200b}d");
        // oracle: trim(""), trim(" \x1c "), trim("\xa0a\tb\n\nc "), trim("one")
        assert_eq!(trim(""), "");
        assert_eq!(trim(" \x1c "), "");
        assert_eq!(trim("\u{a0}a\tb\n\nc "), "a b c");
        assert_eq!(trim("one"), "one");
    }

    #[test]
    fn line_processing_matches_python() {
        // oracle: line_processing(*args)
        let table: &[(&str, bool, bool, Option<&str>)] = &[
            ("a\nb", false, false, Some("a b")),
            ("a.\nb", false, false, Some("a. b")),
            ("p\nq", false, false, Some("p q")),
            ("P\nQ", false, false, Some("P Q")),
            ("{\nz", false, false, Some("{ z")),
            ("}\nz", false, false, Some("} z")),
            ("x>\ny", false, false, Some("x> y")),
            ("a\n\nb", false, false, Some("a b")),
            ("p\n\nq", false, false, Some("p q")),
            ("&nbsp;x", false, false, Some("x")),
            ("  \x1c ", false, false, None),
            ("", false, false, None),
            ("&#13;&#10;a&#10;b", false, false, Some("a b")),
            ("a\nb", true, false, Some("a\nb")),
            ("a\nb", false, true, Some("a b")),
            (" a ", false, true, Some(" a ")),
            (" a ", true, true, Some(" a ")),
            ("\u{a0}a\n", false, true, Some(" a ")),
            ("a\0b", false, false, Some("ab")),
            ("  x  \n  y ", false, false, Some("x y")),
            ("\n", false, false, None),
            ("   ", false, true, None),
            ("a\x1cb", true, false, Some("a\x1cb")),
            ("a\u{200b}b", false, false, Some("ab")),
        ];
        for &(line, preserve, trailing, expected) in table {
            assert_eq!(
                line_processing(line, preserve, trailing).as_deref(),
                expected,
                "{line:?} {preserve} {trailing}"
            );
        }
    }

    #[test]
    fn sanitize_matches_python() {
        // oracle: sanitize(*args)
        let table: &[(&str, bool, bool, Option<&str>)] = &[
            (
                "Hello\nworld\u{2424}\n\u{85}c\n  \n d  e ",
                false,
                false,
                Some("Hello\nworld\nc\nd e"),
            ),
            ("a\nb", false, true, Some("a b")),
            ("  a\n b ", true, false, Some("  a\n b ")),
            ("x\u{2424}y", false, false, Some("xy")),
            ("", false, false, Some("")),
            ("  ", false, true, None),
            ("a\n\n  b\x0bc", false, false, Some("a\nb\nc")),
            ("a\r\nb\rc", false, false, Some("a\nb\nc")),
            ("&nbsp;a\n&#10;b", false, false, Some("a\nb")),
            ("Test&nbsp;Text", false, false, Some("Test Text")),
            ("  a b  ", true, true, Some("  a b  ")),
            ("\ta\n", true, false, Some("\ta")),
            // oracle: sanitize("a\n␤\nb"), sanitize("x &nbsp;y\n&#13;")
            ("a\n\u{2424}\nb", false, false, Some("a\n\nb")),
            ("x &nbsp;y\n&#13;", false, false, Some("x y")),
        ];
        for &(text, preserve, trailing, expected) in table {
            assert_eq!(
                sanitize(text, preserve, trailing).as_deref(),
                expected,
                "{text:?} {preserve} {trailing}"
            );
        }
    }

    #[test]
    fn control_characters_follow_python_printable_and_space() {
        // oracle: remove_control_characters(s)
        let table = [
            ("abc", "abc"),
            ("a\0b\x1fc\x7fd\u{200b}e\u{ad}f", "ab\x1fcdef"),
            ("a\tb\nc\u{a0}d\x1c", "a\tb\nc\u{a0}d\x1c"),
            ("\u{feff}x", "x"),
            ("a\u{e000}b\u{10ffff}c", "abc"),
            ("", ""),
            ("\u{2028}x\u{2029}", "\u{2028}x\u{2029}"),
        ];
        for (input, expected) in table {
            assert_eq!(remove_control_characters(input), expected, "{input:?}");
        }
        // Clean input is borrowed, not copied.
        assert!(matches!(remove_control_characters("abc"), Cow::Borrowed(_)));
        // oracle: return_printables_and_spaces(c)
        let kept: Vec<Option<char>> = ['a', '\0', ' ', '\x1c', '\u{ad}', '\u{a0}']
            .into_iter()
            .map(return_printables_and_spaces)
            .collect();
        assert_eq!(
            kept,
            [
                Some('a'),
                None,
                Some(' '),
                Some('\x1c'),
                None,
                Some('\u{a0}')
            ]
        );
    }

    #[test]
    fn text_chars_test_matches_python() {
        // oracle: text_chars_test(s) for None, "", " ", "\x1c\n", " a ", "\xa0"
        let results: Vec<bool> = [
            None,
            Some(""),
            Some(" "),
            Some("\x1c\n"),
            Some(" a "),
            Some("\u{a0}"),
        ]
        .into_iter()
        .map(text_chars_test)
        .collect();
        assert_eq!(results, [false, false, false, false, true, false]);
    }

    #[test]
    fn textfilter_matches_python() {
        // oracle: textfilter(element) with the text set; a text lxml cannot hold ("\x1c") is
        // evaluated with the body of upstream's `textfilter` on the string
        let table: &[(&str, bool)] = &[
            ("Mehr zum Thema:", true),
            ("More on this topic", true),
            ("Print", true),
            ("\x1c", true),
            ("PR\u{130}NT", true),
            ("P\u{131}nterest", true),
            ("Faceboo\u{212a}", true),
            ("Test Text", false),
            ("Instagram", true),
            ("\t\t", true),
            ("More on this", true),
            ("More on this 123456789", false),
            ("More on this 12345678", false),
            ("  Print", true),
            ("Share\nPrint", true),
            ("Print\nShare", true),
            ("E-Mail", true),
            ("EMAIL", true),
            ("Mehr zum Thema", true),
            ("\u{b2}Print", false),
            ("Print\u{b2}", false),
            ("Xing\u{345}", false),
            ("\u{345}Xing", true),
            ("Pocket\x1c", true),
            ("Linkedin", true),
            ("LINKEDIN", true),
            ("\u{17f}ina", false),
            ("Twitter", true),
            ("Drucken!", false),
            ("!!Drucken", true),
            ("Pdf", true),
            ("QQ", true),
            ("x\u{2028}Print", true),
            ("Print\x1cx", true),
            ("More on this\u{85}", true),
        ];
        for &(text, expected) in table {
            assert_eq!(filtered(Some(text), None), expected, "{text:?}");
        }
    }

    #[test]
    fn textfilter_reads_tail_only_without_text() {
        // oracle: textfilter(element) with text/tail set
        assert!(filtered(None, Some("Print")));
        assert!(filtered(None, None));
        assert!(!filtered(Some("x"), Some("Print")));
        assert!(filtered(None, Some(" ")));
    }

    #[test]
    fn is_image_file_matches_python() {
        let long = |tail: usize| format!("{}.jpg", "x".repeat(tail));
        // oracle: is_image_file(s)
        let table: Vec<(Option<String>, bool)> = vec![
            (Some("a.jpg?x=1".into()), true),
            (Some("a.jpgx".into()), false),
            (Some("a.jpg\u{b2}".into()), false),
            (Some(long(8189)), false), // 8193 characters
            (Some(long(8188)), true),  // 8192 characters
            (Some(long(8190)), false),
            (None, false),
            (Some(String::new()), false),
            (Some("A.JPG".into()), true),
            (Some("a.jpeg".into()), true),
            (Some("a.heic".into()), true),
            (Some("a.hei\u{131}".into()), false),
            (Some("a.AV\u{130}F".into()), true),
            (Some("a.png".into()), true),
            (Some(".png".into()), false),
            (Some(" .png".into()), false),
            (Some("a b.png".into()), true),
            (Some("a.png.txt".into()), true),
            (Some("a.png.".into()), true),
            (Some("a.png_".into()), false),
            (Some("a.png-".into()), true),
            (Some("a.webp\u{e9}".into()), false),
            (Some("a.gif\x1c".into()), true),
            (Some("a.bmp\u{200b}".into()), true),
            (Some("x.jpg\u{345}".into()), true),
            (Some("a.webP\u{212a}".into()), false),
            (Some("\u{17f}.JPG".into()), true),
        ];
        for (src, expected) in table {
            let shown: String = src.iter().flat_map(|s| s.chars().take(40)).collect();
            assert_eq!(is_image_file(src.as_deref()), expected, "{shown:?}");
        }
    }

    #[test]
    fn image_src_matches_python() {
        let img = |attrs: &str| {
            let tree = parse_fragment(
                &format!("<img {attrs}>"),
                &ParseLimits::default(),
                &Call::default(),
            )
            .expect("parses");
            let img = tree.children(tree.root()).next().expect("img");
            image_src(&tree, img).map(String::from)
        };
        // oracle: image_src(element) with the attributes set in this order
        assert_eq!(
            img(r#"data-src="a.jpg" src="b.png""#).as_deref(),
            Some("a.jpg")
        );
        assert_eq!(
            img(r#"data-src="a.txt" src="b.png""#).as_deref(),
            Some("b.png")
        );
        assert_eq!(
            img(r#"src="b.txt" data-srcset="c.webp 2x""#).as_deref(),
            Some("c.webp 2x")
        );
        assert_eq!(
            img(r#"src="b.txt" data-srcset="c.txt" data-src2="d.gif""#).as_deref(),
            Some("d.gif")
        );
        assert_eq!(img(""), None);
        assert_eq!(img(r#"data-src="a.txt""#), None);
        assert_eq!(
            img(r#"src="b.png" data-src-x="c.png""#).as_deref(),
            Some("b.png")
        );
    }

    #[test]
    fn check_html_lang_matches_python() {
        let page = |meta: &str, html_attrs: &str| {
            format!(
                "<html{html_attrs}><head>{meta}</head><body><p>Some text here</p></body></html>"
            )
        };
        let og = |content: &str| {
            page(
                &format!(r#"<meta property="og:locale" content="{content}">"#),
                "",
            )
        };
        // oracle: check_html_lang(load_html(page), target, strict)
        let table: Vec<(String, &str, bool, bool)> = vec![
            (og("de_DE"), "de", false, true),
            (og("de_DE"), "en", false, false),
            (og("de_DE"), "de_de", false, false),
            (og("en"), "e", false, false),
            (og("eng"), "g", false, true),
            (og("eng"), "en", false, true),
            (og("EN-us"), "us", false, true),
            (og("en"), "", false, true),
            (og("e"), "", false, false),
            (og(""), "", false, true),
            (og(""), "de", false, false),
            (
                page(
                    r#"<meta http-equiv="content-language" content="fr, en"><meta property="og:locale" content="de">"#,
                    "",
                ),
                "de",
                false,
                false,
            ),
            (
                page(
                    r#"<meta http-equiv="content-language" content="fr, en"><meta property="og:locale" content="de">"#,
                    "",
                ),
                "en",
                false,
                true,
            ),
            (
                page(
                    r#"<meta http-equiv="Content-Language" content="fr"><meta property="og:locale" content="de">"#,
                    "",
                ),
                "de",
                false,
                true,
            ),
            (
                page(r#"<meta property="og:locale">"#, ""),
                "de",
                false,
                true,
            ),
            (
                page(
                    r#"<meta property="og:locale" content="fr"><meta property="og:locale" content="de">"#,
                    "",
                ),
                "de",
                false,
                true,
            ),
            (page("", r#" lang="de-DE""#), "de", true, true),
            (page("", r#" lang="de-DE""#), "en", true, false),
            (page("", r#" lang="de-DE""#), "en", false, true),
            (page("", ""), "en", true, true),
            (page("", r#" lang="""#), "en", true, false),
            (page("", r#" lang="""#), "", true, true),
            (
                page(
                    r#"<meta property="og:locale" content="de">"#,
                    r#" lang="en""#,
                ),
                "en",
                true,
                false,
            ),
            (og("\u{130}de"), "de", false, true),
        ];
        for (html, target, strict, expected) in table {
            let tree = tree(&html);
            assert_eq!(
                check_html_lang(&tree, tree.root(), target, strict),
                expected,
                "{html} {target:?} {strict}"
            );
        }
    }

    #[test]
    fn is_acceptable_length_matches_python() {
        let options = Extractor::new(
            Config::default(),
            Format::Txt,
            false,
            false,
            None,
            false,
            false,
            HashSet::new(),
            HashSet::new(),
        );
        let call = Call::default();
        let ctx = Ctx {
            options: Cow::Borrowed(&options),
            call: &call,
        };
        // oracle: [is_acceptable_length(n, Extractor()) for n in (0, 9, 10, 11, 20000000, 20000001)]
        let results: Vec<bool> = [0, 9, 10, 11, 20_000_000, 20_000_001]
            .into_iter()
            .map(|len| is_acceptable_length(len, &ctx))
            .collect();
        assert_eq!(results, [false, false, true, true, true, false]);
    }

    #[test]
    fn language_filter_is_a_stub_until_phase_4() {
        assert!(!language_filter("text", "comments", Some("de")));
    }

    #[test]
    fn sanitize_tree_keeps_space_in_code() {
        // oracle: sanitize_tree(load_html(page)) then (tag, attrib, text, tail) for tree.iter()
        assert_eq!(
            sanitized("<p>a  b<code> x  y </code></p>"),
            [
                row("html", &[], None, None),
                row("body", &[], None, None),
                row("p", &[], Some("a b"), None),
                row("code", &[], Some(" x  y "), None),
            ]
        );
    }

    #[test]
    fn sanitize_tree_trims_by_protected_tags() {
        assert_eq!(
            sanitized(
                "<p>  a  <b> b  </b>  c\n d </p><pre>  l1\n  l2  </pre><div> x  y </div>\
                 <ul><li> i <a href='u'> r </a> </li></ul>"
            ),
            [
                row("html", &[], None, None),
                row("body", &[], None, None),
                row("p", &[], Some(" a "), None),
                row("b", &[], Some(" b "), Some(" c d ")),
                row("pre", &[], Some("  l1\n  l2  "), None),
                row("div", &[], Some("x y"), None),
                row("ul", &[], None, None),
                row("li", &[], Some("i"), None),
                row("a", &[("href", "u")], Some("r"), Some("")),
            ]
        );
        assert_eq!(
            sanitized(
                "<blockquote> q  </blockquote><h2> h  </h2><p>x <span> y </span> z</p>\
                 <p> <b>k</b> <i> m </i> </p>"
            ),
            [
                row("html", &[], None, None),
                row("body", &[], None, None),
                row("blockquote", &[], Some("q"), None),
                row("h2", &[], Some("h"), None),
                row("p", &[], Some("x "), None),
                row("span", &[], Some(" y "), Some(" z")),
                row("p", &[], None, None),
                row("b", &[], Some("k"), None),
                row("i", &[], Some(" m "), None),
            ]
        );
        assert_eq!(
            sanitized(
                "<p>a\u{2424}b\x1c c&nbsp;d</p><code>\n x\n\ty </code><div><code> a </code> t </div>"
            ),
            [
                row("html", &[], None, None),
                row("body", &[], None, None),
                row("p", &[], Some("a\u{2424}b c d"), None),
                row("code", &[], Some("\n x\n\ty "), None),
                row("div", &[], None, None),
                row("code", &[], Some(" a "), Some(" t ")),
            ]
        );
    }

    #[test]
    fn sanitize_tree_drops_attributes_with_a_colon() {
        // The tree declares no namespace, as lxml's `nsmap` is empty for a parsed page.
        assert_eq!(
            sanitized(r#"<p xlink:href="a" data-x="1" foo:bar="" lang="en">t</p>"#),
            [
                row("html", &[], None, None),
                row("body", &[], None, None),
                row("p", &[("data-x", "1"), ("lang", "en")], Some("t"), None),
            ]
        );
    }

    #[test]
    fn sanitize_tree_stops_when_cancelled() {
        let token = crate::cancel::CancelToken::new();
        token.cancel();
        let call = Call::default().cancel(&token);
        let mut tree = tree("<html><body><p>a  b</p></body></html>");
        let root = tree.root();
        assert_eq!(
            sanitize_tree(&mut tree, root, &call),
            Err(ExtractError::Cancelled)
        );
    }
}

#[test]
fn as_list_matches_python() {
    let text = r#"{"o":{},"s":"x","l":[1,"a"],"e":[],"z":null,"n":5,"b":true}"#;
    let json = crate::pyjson::loads(text, true, &Call::default())
        .unwrap()
        .unwrap();
    let root = json.root();
    let get = |key: &str| json.get(root, key);
    let items = |key: &str| json.items(get(key).unwrap()).collect::<Vec<_>>();
    // oracle: as_list(None) gives []
    assert_eq!(as_list(&json, None), []);
    // oracle: as_list([1, 'a']) gives the list, as_list([]) gives []
    assert_eq!(as_list(&json, get("l")), items("l"));
    assert_eq!(as_list(&json, get("e")), []);
    // oracle: as_list('x') gives ['x'], as_list({}) gives [{}], as_list(5) gives [5], as_list(True) gives [True]
    for key in ["s", "o", "n", "b"] {
        assert_eq!(as_list(&json, get(key)), [get(key).unwrap()], "{key}");
    }
    // oracle: as_list(json.loads('null')) gives []
    assert_eq!(as_list(&json, get("z")), []);
}

#[test]
fn normalize_unicode_is_nfc() {
    // oracle: normalize_unicode("e" + chr(0x301) + " " + chr(0x212b)), normalize_unicode("\xe9")
    assert_eq!(normalize_unicode("e\u{301} \u{212b}"), "\u{e9} \u{c5}");
    assert!(matches!(normalize_unicode("\u{e9}"), Cow::Borrowed(_)));
}
