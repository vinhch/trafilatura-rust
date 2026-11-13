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
            Err(ExtractError::Rejected(Rejection::InputTooLarge(
                InputLimit::MaxInputBytes
            )))
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
}

fn encoding_case(name: &str) -> &'static EncodingCase {
    ENCODING_CASES
        .iter()
        .find(|case| case.name == name)
        .unwrap_or_else(|| panic!("no encoding case {name}"))
}

#[test]
fn test_input_detect_encoding() {
    assert_eq!(detect_encoding("高山云雾出好茶".as_bytes()), ["utf-8"]);
    for name in ["gb18030", "gb18030_x1000"] {
        let guessed = detect_encoding(encoding_case(name).input);
        assert!(guessed.iter().any(|guess| guess == "gb18030"), "{name}");
    }
}

#[test]
fn detect_encoding_matches_charset_normalizer() {
    let failures: Vec<String> = ENCODING_CASES
        .iter()
        .filter_map(|case| {
            let guesses = detect_encoding(case.input);
            let decoded = decode_file(case.input, MAX_FILE_SIZE);
            let mut problems = Vec::new();
            if guesses != case.guesses {
                problems.push(format!("guesses {guesses:?}, oracle {:?}", case.guesses));
            }
            if decoded != case.decoded {
                problems.push("decoded text differs".to_owned());
            }
            (!problems.is_empty()).then(|| format!("{}: {}", case.name, problems.join("; ")))
        })
        .collect();
    assert!(failures.is_empty(), "{failures:#?}");
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
    let latin1: Vec<u8> = "<html><body>\u{2f}\u{2e}\u{9f}</body></html>"
        .chars()
        .map(|c| u8::try_from(u32::from(c)).unwrap())
        .collect();
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
    // oracle: upstream load_html("t <div>a</div>") is None: the root div holds one child element
    assert!(load("t <div>a</div>").is_err());
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
fn load_html_leading_text_does_not_count_as_a_child() {
    // oracle: upstream load_html("text <p>a</p>") and load_html("x<div><p>a</p><p>b</p></div>") are None:
    // the root holds one child element, so the leading text adds no child
    assert!(load("text <p>a</p>").is_err());
    assert!(load("x<div><p>a</p><p>b</p></div>").is_err());
    assert!(load("hello <b>x</b> <i>y</i>").is_ok());
    assert!(load("hello <b>x</b>").is_err());
    assert!(load("just text").is_err());
}
