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
        format!("<html{html_attrs}><head>{meta}</head><body><p>Some text here</p></body></html>")
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
fn check_html_lang_lowercases_like_python_str_lower() {
    // Python 3.13 leaves U+10D50 unchanged; Rust's to_lowercase maps it to U+10D70.
    // oracle: check_html_lang(load_html('<html lang="\U00010d50">...'), target, True)
    let html = "<html lang=\"\u{10d50}\"><head></head><body><p>Some text here</p></body></html>";
    let tree = tree(html);
    assert!(!check_html_lang(&tree, tree.root(), "\u{10d70}", true));
    assert!(check_html_lang(&tree, tree.root(), "\u{10d50}", true));
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
fn language_filter_stub_rejects_text() {
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
