use super::*;

fn load(html: &str) -> Result<Tree, ExtractError> {
    load_html(
        HtmlInput::Str(html),
        SIZE,
        &ParseLimits::default(),
        &Call::default(),
    )
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
    let beginning = pystr::lower(&html.chars().take(50).collect::<String>());
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
