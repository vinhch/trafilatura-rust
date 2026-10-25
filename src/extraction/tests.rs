use std::collections::HashSet;

use super::*;
use crate::settings::Config;

const BS: char = '\u{5c}';

fn options(format: Format, with_metadata: bool) -> Extractor {
    let mut options = Extractor::new(
        Config::default(),
        format,
        false,
        false,
        None,
        false,
        false,
        HashSet::new(),
        HashSet::new(),
    );
    options.with_metadata = with_metadata;
    options
}

fn render(doc: &mut Document, format: Format, with_metadata: bool) -> String {
    determine_returnstring(doc, &options(format, with_metadata), &Call::default()).unwrap()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

/// A body with one paragraph holding `text`.
fn paragraph(doc: &mut Document, text: &str) {
    let p = doc.tree.new_element("p");
    doc.tree.append(doc.body, p);
    doc.tree.node_mut(p).text = Some(text.to_owned());
}

#[test]
fn yaml_scalar_matches_python() {
    let quoted = |text: &str| format!("\"{text}\"");
    let rows: Vec<(String, String)> = vec![
        // oracle: _yaml_scalar("\u216b" + "x")
        ("\u{216b}x".into(), quoted("\u{216b}x")),
        // oracle: _yaml_scalar("\u01c5" + "x")
        ("\u{1c5}x".into(), "\u{1c5}x".into()),
        // oracle: _yaml_scalar("x" + chr(0x1c))
        ("x\u{1c}".into(), quoted(&format!("x{BS}u001c"))),
        // oracle: _yaml_scalar("")
        (String::new(), quoted("")),
        // oracle: _yaml_scalar(" a")
        (" a".into(), quoted(" a")),
        // oracle: _yaml_scalar("a ")
        ("a ".into(), quoted("a ")),
        // oracle: _yaml_scalar("a #b")
        ("a #b".into(), quoted("a #b")),
        // oracle: _yaml_scalar("a:")
        ("a:".into(), quoted("a:")),
        // oracle: _yaml_scalar("a: b")
        ("a: b".into(), quoted("a: b")),
        // oracle: _yaml_scalar("Yes")
        ("Yes".into(), quoted("Yes")),
        // oracle: _yaml_scalar("None")
        ("None".into(), quoted("None")),
        // oracle: _yaml_scalar("NULL")
        ("NULL".into(), quoted("NULL")),
        // oracle: _yaml_scalar("~")
        ("~".into(), quoted("~")),
        // oracle: _yaml_scalar("1a")
        ("1a".into(), quoted("1a")),
        // oracle: _yaml_scalar("\u00e9t\u00e9")
        ("\u{e9}t\u{e9}".into(), "\u{e9}t\u{e9}".into()),
        // oracle: _yaml_scalar("a" + chr(0x2028) + "b")
        ("a\u{2028}b".into(), "a\u{2028}b".into()),
        // oracle: _yaml_scalar(chr(0x345) + "x")
        ("\u{345}x".into(), quoted("\u{345}x")),
        // oracle: _yaml_scalar("a" + chr(0x7f) + "b")
        ("a\u{7f}b".into(), quoted("a\u{7f}b")),
        // oracle: _yaml_scalar("a" + chr(0x85) + "b")
        ("a\u{85}b".into(), "a\u{85}b".into()),
        // oracle: _yaml_scalar("a\nb")
        ("a\nb".into(), quoted(&format!("a{BS}nb"))),
        // oracle: _yaml_scalar('say "hi"')
        ("say \"hi\"".into(), "say \"hi\"".into()),
        // oracle: _yaml_scalar("Title")
        ("Title".into(), "Title".into()),
    ];
    for (input, expected) in rows {
        assert_eq!(yaml_scalar(&input), expected, "input {input:?}");
    }
}

#[test]
fn txt_header_matches_python() {
    let mut doc = Document::new();
    doc.title = Some("T".into());
    doc.categories = Some(strings(&["it's", "say \"hi\"", "a\nb", "\u{85}", "\u{e9}"]));
    doc.id = Some("7".into());
    doc.fingerprint = Some(String::new());
    // oracle: determine_returnstring(Document(title="T", categories=["it's", 'say "hi"', "a\nb",
    //   chr(0x85), chr(0xe9)], idval="7", fingerprint=""), Extractor(output_format=fmt,
    //   with_metadata=True)) for fmt in ("txt", "markdown")
    let expected = format!(
        "---\ntitle: T\ncategories: [\"it's\", 'say \"hi\"', 'a{BS}nb', '{BS}x85', '\u{e9}']\nid: \"7\"\n---"
    );
    for format in [Format::Txt, Format::Markdown] {
        assert_eq!(render(&mut doc, format, true), expected, "{format:?}");
    }
    // oracle: determine_returnstring(Document(), Extractor(output_format="txt", with_metadata=True))
    assert_eq!(render(&mut Document::new(), Format::Txt, true), "---\n---");
}

#[test]
fn html_meta_matches_python() {
    let mut doc = Document::new();
    doc.title = Some("T".into());
    doc.categories = Some(strings(&["a", "b"]));
    doc.id = Some("7".into());
    // oracle: determine_returnstring(Document(title="T", categories=["a", "b"], idval="7"),
    //   Extractor(output_format="html", with_metadata=True))
    assert_eq!(
        render(&mut doc, Format::Html, true),
        "<html>\n  <head>\n    <meta name=\"title\" content=\"T\"/>\n    \
         <meta name=\"categories\" content=\"a;b\"/>\n    <meta name=\"id\" content=\"7\"/>\n  \
         </head>\n  <body/>\n</html>"
    );
    // oracle: determine_returnstring(Document(), Extractor(output_format="html", with_metadata=True))
    assert_eq!(
        render(&mut Document::new(), Format::Html, true),
        "<html>\n  <head/>\n  <body/>\n</html>"
    );
}

#[test]
fn empty_document_in_each_format() {
    // oracle: determine_returnstring(Document(), Extractor(output_format=fmt)) for fmt in
    //   ("txt", "markdown", "html")
    for (format, expected) in [
        (Format::Txt, ""),
        (Format::Markdown, ""),
        (Format::Html, "<html>\n  <body/>\n</html>"),
    ] {
        assert_eq!(
            render(&mut Document::new(), format, false),
            expected,
            "{format:?}"
        );
    }
}

#[test]
fn returnstring_is_nfc() {
    // oracle: body = Element("body"); SubElement(body, "p").text = "e" + chr(0x301);
    //   determine_returnstring(Document(body=body), Extractor(output_format=fmt))
    //   for fmt in ("txt", "markdown", "html")
    for (format, expected) in [
        (Format::Txt, "\u{e9}"),
        (Format::Markdown, "\u{e9}"),
        (
            Format::Html,
            "<html>\n  <body>\n    <p>\u{e9}</p>\n  </body>\n</html>",
        ),
    ] {
        let mut doc = Document::new();
        paragraph(&mut doc, "e\u{301}");
        assert_eq!(render(&mut doc, format, false), expected, "{format:?}");
    }
}
