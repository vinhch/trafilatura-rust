//! The JSON and CSV builders against upstream's output. Each expected value is the output of one
//! `scripts/oracle.sh` run, whose command is in the comment above its test.

use super::xml;
use crate::cancel::Call;
use crate::settings::Document;
use crate::xml::{build_json_output, xml_to_csv};

/// A document whose body and comments body are copies of the `body` and `comments` snippets.
fn document(body: &str, comments: &str) -> Document {
    let mut doc = Document::new();
    let body_src = xml(body);
    doc.body = body_src
        .copy_into(body_src.root(), &mut doc.tree, &Call::default())
        .unwrap();
    let comments_src = xml(comments);
    doc.comments_body = comments_src
        .copy_into(comments_src.root(), &mut doc.tree, &Call::default())
        .unwrap();
    doc
}

// scripts/oracle.sh 'from lxml import etree; from trafilatura.settings import Document; from trafilatura.xml import build_json_output; doc = Document(); doc.title = "a" + chr(34) + "b" + chr(92) + "c" + chr(10) + "d" + chr(9) + "e" + chr(1) + "f" + chr(0x2028) + chr(0xe9) + chr(8) + chr(12) + chr(127); doc.body = etree.fromstring("<p>Text</p>"); print(build_json_output(doc, True))'
#[test]
fn json_escapes_match_python() {
    let mut doc = document("<p>Text</p>", "<body/>");
    doc.title = Some("a\"b\\c\nd\te\u{1}f\u{2028}é\u{8}\u{c}\u{7f}".to_owned());
    let expected = concat!(
        r#"{"title": "a\"b\\c\nd\te\u0001f"#,
        "\u{2028}",
        r#"é\b\f"#,
        "\u{7f}",
        r#"", "author": null, "hostname": null, "date": null, "fingerprint": null, "id": null, "license": null, "comments": "", "raw_text": null, "text": "Text", "language": null, "image": null, "pagetype": null, "filedate": null, "source": null, "source-hostname": null, "excerpt": null, "categories": "", "tags": ""}"#
    );
    assert_eq!(
        build_json_output(&mut doc, true, &Call::default()).unwrap(),
        expected
    );
}

// scripts/oracle.sh 'from lxml import etree; from trafilatura.settings import Document; from trafilatura.xml import build_json_output; doc = Document(); doc.title = "Title"; doc.author = ""; doc.url = "https://e.org/p"; doc.hostname = "e.org"; doc.description = "Desc"; doc.sitename = "Site"; doc.date = "2026-01-02"; doc.categories = ["c1", "c2"]; doc.tags = []; doc.fingerprint = "FP"; doc.id = "ID1"; doc.license = "CC"; doc.raw_text = "Raw"; doc.text = "not used"; doc.language = "en"; doc.image = "https://e.org/i.jpg"; doc.pagetype = "article"; doc.filedate = "2026-10-01"; doc.body = etree.fromstring("<p>Text</p>"); doc.commentsbody = etree.fromstring("<p>Note</p>"); print(build_json_output(doc, True)); print(build_json_output(doc, False))'
#[test]
fn json_matches_python_with_and_without_metadata() {
    let mut doc = document("<p>Text</p>", "<p>Note</p>");
    doc.title = Some("Title".to_owned());
    // an empty string is written as it is, not as `null`
    doc.author = Some(String::new());
    doc.url = Some("https://e.org/p".to_owned());
    doc.hostname = Some("e.org".to_owned());
    doc.description = Some("Desc".to_owned());
    doc.sitename = Some("Site".to_owned());
    doc.date = Some("2026-01-02".to_owned());
    doc.categories = Some(vec!["c1".to_owned(), "c2".to_owned()]);
    doc.tags = Some(Vec::new());
    doc.fingerprint = Some("FP".to_owned());
    doc.id = Some("ID1".to_owned());
    doc.license = Some("CC".to_owned());
    doc.raw_text = Some("Raw".to_owned());
    // the JSON `text` is the body, never this field
    doc.text = Some("not used".to_owned());
    doc.language = Some("en".to_owned());
    doc.image = Some("https://e.org/i.jpg".to_owned());
    doc.pagetype = Some("article".to_owned());
    doc.filedate = Some("2026-10-01".to_owned());
    assert_eq!(
        build_json_output(&mut doc, true, &Call::default()).unwrap(),
        r#"{"title": "Title", "author": "", "hostname": "e.org", "date": "2026-01-02", "fingerprint": "FP", "id": "ID1", "license": "CC", "comments": "Note", "raw_text": "Raw", "text": "Text", "language": "en", "image": "https://e.org/i.jpg", "pagetype": "article", "filedate": "2026-10-01", "source": "https://e.org/p", "source-hostname": "Site", "excerpt": "Desc", "categories": "c1;c2", "tags": ""}"#
    );
    assert_eq!(
        build_json_output(&mut doc, false, &Call::default()).unwrap(),
        r#"{"text": "Text", "comments": "Note"}"#
    );
}

// scripts/oracle.sh 'from lxml import etree; from trafilatura.settings import Document; from trafilatura.xml import build_json_output; doc = Document(); doc.body = etree.fromstring("<p>" + chr(60) + "hi rend=" + chr(39) + "#b" + chr(39) + ">x</hi></p>"); print(build_json_output(doc, False))'
#[test]
fn json_ignores_formatting() {
    let mut doc = document(r##"<p><hi rend="#b">x</hi></p>"##, "<body/>");
    assert_eq!(
        build_json_output(&mut doc, false, &Call::default()).unwrap(),
        r#"{"text": "x", "comments": ""}"#
    );
}

// scripts/oracle.sh 'from lxml import etree; from trafilatura.settings import Document; from trafilatura.xml import xmltocsv; doc = Document(); doc.url = "a" + chr(10) + "b"; doc.id = "say " + chr(34) + "hi" + chr(34); doc.fingerprint = ""; doc.hostname = "x" + chr(9) + "y"; doc.license = "c" + chr(13) + "d"; doc.pagetype = "plain"; print(repr(xmltocsv(doc, False)))'
#[test]
fn csv_quoting_matches_python() {
    let mut doc = document("<body/>", "<body/>");
    doc.url = Some("a\nb".to_owned());
    doc.id = Some("say \"hi\"".to_owned());
    // an empty field is `null`, as the falsy `d or null` of upstream writes it
    doc.fingerprint = Some(String::new());
    doc.hostname = Some("x\ty".to_owned());
    doc.license = Some("c\rd".to_owned());
    doc.pagetype = Some("plain".to_owned());
    assert_eq!(
        xml_to_csv(&mut doc, false, &Call::default()).unwrap(),
        "\"a\nb\"\t\"say \"\"hi\"\"\"\tnull\t\"x\ty\"\tnull\tnull\tnull\tnull\tnull\t\"c\rd\"\tplain\r\n"
    );
}

// scripts/oracle.sh 'from lxml import etree; from trafilatura.settings import Document; from trafilatura.xml import xmltocsv; doc = Document(); doc.url = "https://example.org"; doc.body = etree.fromstring("<body><p>first line</p><p>say " + chr(34) + "hi" + chr(34) + "</p><p>x" + chr(9) + "y</p></body>"); doc.commentsbody = etree.fromstring("<body><p>Note " + chr(34) + "x" + chr(34) + "</p></body>"); print(repr(xmltocsv(doc, False)))'
#[test]
fn csv_multiline_matches_python() {
    let mut doc = document(
        "<body><p>first line</p><p>say \"hi\"</p><p>x\ty</p></body>",
        "<body><p>Note \"x\"</p></body>",
    );
    doc.url = Some("https://example.org".to_owned());
    assert_eq!(
        xml_to_csv(&mut doc, false, &Call::default()).unwrap(),
        "https://example.org\tnull\tnull\tnull\tnull\tnull\tnull\t\"first line\nsay \"\"hi\"\"\nx\ty\"\t\"Note \"\"x\"\"\"\tnull\tnull\r\n"
    );
}

// scripts/oracle.sh 'from lxml import etree; from trafilatura.settings import Document; from trafilatura.xml import xmltocsv; doc = Document(); doc.body = etree.fromstring("<p>" + chr(60) + "hi rend=" + chr(39) + "#b" + chr(39) + ">x</hi></p>"); print(repr(xmltocsv(doc, True)))'
#[test]
fn csv_formatting_matches_python() {
    let mut doc = document(r##"<p><hi rend="#b">x</hi></p>"##, "<body/>");
    assert_eq!(
        xml_to_csv(&mut doc, true, &Call::default()).unwrap(),
        "null\tnull\tnull\tnull\tnull\tnull\tnull\t\"**x**\n\"\tnull\tnull\tnull\r\n"
    );
}
