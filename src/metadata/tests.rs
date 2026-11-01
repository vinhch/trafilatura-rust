use std::collections::HashSet;

use super::*;
use crate::cancel::CancelToken;
use crate::dom::{ParseLimits, parse_document};
use crate::error::Rejection;

fn parse(html: &str) -> Tree {
    parse_document(html, &ParseLimits::default(), &Call::default()).unwrap()
}

fn page(head: &str) -> Tree {
    parse(&format!("<html><head>{head}</head><body></body></html>"))
}

fn scripts_page(texts: &[&str]) -> Tree {
    let head: String = texts
        .iter()
        .map(|text| format!(r#"<script type="application/ld+json">{text}</script>"#))
        .collect();
    page(&head)
}

fn read_scripts(texts: &[&str]) -> (Document, Result<(), JsonFailure>) {
    let tree = scripts_page(texts);
    let mut document = Document::new();
    let result = extract_meta_json(&tree, tree.root(), &mut document, &Call::default());
    (document, result)
}

fn opengraph(head: &str) -> Document {
    let tree = page(head);
    extract_opengraph(&tree, tree.root(), &Call::default()).unwrap()
}

fn examine(head: &str) -> Document {
    let tree = page(head);
    examine_meta(&tree, tree.root(), &Call::default()).unwrap()
}

fn invalid_value() -> ExtractError {
    ExtractError::Rejected(Rejection::InvalidValue)
}

#[test]
fn patterns_compile() {
    assert!(JSON_MINIFY.is_some());
    assert!(CLEAN_META_TAGS.is_some());
}

#[test]
fn opengraph_reads_og_properties() {
    let tree = parse(
        r#"<html><head>
        <meta property="og:title" content="Title">
        <meta property="og:url" content="not a url">
        <meta property="og:url" content="https://example.org/a">
        <meta property="og:url" content="still not a url">
        <meta property="og:author" content="By Jane Doe">
        <meta property="og:description" content="   ">
        <meta property="og:site_name" content="Site">
        <meta property="og:image" content="https://example.org/1.png">
        <meta property="og:image:url" content="https://example.org/2.png">
        <meta property="og:type" content="article">
        <meta property="og:locale" content="en">
        <meta property="og:title">
        </head><body><meta property="og:title" content="Body"></body></html>"#,
    );
    let document = extract_opengraph(&tree, tree.root(), &Call::default()).unwrap();
    // oracle: html5tree.parse_document(html) -> extract_opengraph(tree)
    assert_eq!(document.title.as_deref(), Some("Title"));
    assert_eq!(document.url.as_deref(), Some("https://example.org/a"));
    assert_eq!(document.author.as_deref(), Some("Jane Doe"));
    assert_eq!(document.description, None);
    assert_eq!(document.sitename.as_deref(), Some("Site"));
    assert_eq!(document.image.as_deref(), Some("https://example.org/2.png"));
    assert_eq!(document.pagetype.as_deref(), Some("article"));
}

#[test]
fn examine_meta_order_of_sources() {
    let document = examine(
        r#"<meta property="og:title" content="OG Title">
        <meta property="article:tag" content="&quot;a&quot;, 'b', , c">
        <meta property="article:tag" content="&quot;&quot;">
        <meta property="author" content="Ann Lee">
        <meta name="author" content="Bob Ray">
        <meta itemprop="author" content="Cy Doe">
        <meta name="title" content="Name Title">
        <meta itemprop="headline" content="Item Headline">
        <meta name="description" content="Name Description">
        <meta itemprop="description" content="Item Description">
        <meta name="keywords" content="k1, k2">
        <meta name="twitter:url" content="https://tw.example/x">
        <meta name="twitter:url" content="https://tw.example/y">
        <meta name="application-name" content="App">
        <meta name="twitter:app:name:iphone" content="App Two">
        <meta name="robots" content="  ">
        <meta name="unknown" content="x">
        <meta charset="utf-8">"#,
    );
    // oracle: html5tree.parse_document(html) -> examine_meta(tree)
    assert_eq!(document.title.as_deref(), Some("OG Title"));
    assert_eq!(document.author.as_deref(), Some("Ann Lee; Bob Ray; Cy Doe"));
    assert_eq!(document.description.as_deref(), Some("Name Description"));
    assert_eq!(document.url.as_deref(), Some("https://tw.example/x"));
    assert_eq!(document.sitename.as_deref(), Some("App Two"));
    assert_eq!(
        document.tags,
        Some(vec![
            "a, b, c".to_owned(),
            String::new(),
            "k1, k2".to_owned()
        ])
    );
}

#[test]
fn examine_meta_twitter_url_yields_to_og_url() {
    let document = examine(
        r#"<meta property="og:url" content="https://example.org/og">
        <meta name="twitter:url" content="https://tw.example/x">
        <meta property="article:publisher" content="Pub">
        <meta name="application-name" content="App">"#,
    );
    // oracle: html5tree.parse_document(html) -> examine_meta(tree)
    assert_eq!(document.url.as_deref(), Some("https://example.org/og"));
    assert_eq!(document.sitename.as_deref(), Some("Pub"));
    assert_eq!(document.tags, Some(Vec::new()));
}

#[test]
fn examine_meta_content_strips_tags() {
    let document = examine(r#"<meta name="description" content="<b>x</b>">"#);
    // oracle: html5tree.parse_document(html) -> examine_meta(tree).description
    assert_eq!(document.description.as_deref(), Some("x"));
}

#[test]
fn normalize_tags_strips_quotes() {
    // oracle: normalize_tags("\"a\", 'b', , c")
    assert_eq!(normalize_tags("\"a\", 'b', , c").unwrap(), "a, b, c");
    // oracle: normalize_tags("  ")
    assert_eq!(normalize_tags("  ").unwrap(), "");
    // oracle: normalize_tags("&amp;x   y")
    assert_eq!(normalize_tags("&amp;x   y").unwrap(), "&x y");
    // oracle: normalize_tags('""')
    assert_eq!(normalize_tags("\"\"").unwrap(), "");
}

#[test]
fn check_authors_case_insensitive() {
    let blacklist: HashSet<String> = ["JOHN DOE".to_owned()].into();
    // oracle: check_authors("john doe; Jane Roe", {"JOHN DOE"})
    assert_eq!(
        check_authors("john doe; Jane Roe", &blacklist).as_deref(),
        Some("Jane Roe")
    );
    // oracle: check_authors(" John Doe ;", {"JOHN DOE"})
    assert_eq!(
        check_authors(" John Doe ;", &blacklist).as_deref(),
        Some("")
    );
    // oracle: check_authors("; a ;b;", set())
    assert_eq!(
        check_authors("; a ;b;", &HashSet::new()).as_deref(),
        Some("a; b")
    );
}

#[test]
fn extract_meta_json_minifies_then_parses() {
    let (document, result) = read_scripts(&[
        "{\n \"@context\": \"https://schema.org\",\n \"@type\": \"Article\",\n \"headline\":\t\"A  B\u{1}C\"\n}",
    ]);
    // oracle: html5tree.parse_document(html) -> extract_meta_json(tree, Document())
    assert_eq!(result, Ok(()));
    assert_eq!(document.title.as_deref(), Some("A B\u{1}C"));
}

#[test]
fn extract_meta_json_falls_back_on_decode_error() {
    let (document, result) = read_scripts(&[
        r#"{"@context":"https://schema.org","@type":"Article","headline":"Broken Headline",,}"#,
    ]);
    // oracle: html5tree.parse_document(html) -> extract_meta_json(tree, Document())
    assert_eq!(result, Ok(()));
    assert_eq!(document.title.as_deref(), Some("Broken Headline"));
    assert_eq!(document.pagetype.as_deref(), Some("article"));
}

#[test]
fn extract_meta_json_reads_both_script_types() {
    let tree = page(
        r#"<script type="application/settings+json">{"@context":"https://schema.org","@type":"Article","headline":"One"}</script>
        <script type="text/javascript">{"@context":"https://schema.org","@type":"Article","headline":"Skipped"}</script>
        <script type="application/ld+json"></script>"#,
    );
    let mut document = Document::new();
    let result = extract_meta_json(&tree, tree.root(), &mut document, &Call::default());
    // oracle: html5tree.parse_document(html) -> extract_meta_json(tree, Document()).title
    assert_eq!(result, Ok(()));
    assert_eq!(document.title.as_deref(), Some("One"));
}

#[test]
fn deep_json_ld_stops_the_script_loop() {
    let deep = format!(
        r#"{{"@context":"https://schema.org","@type":"Article","headline":"T","x":{}{}}}"#,
        "[".repeat(10_000),
        "]".repeat(10_000)
    );
    let second = r#"{"@context":"https://schema.org","@type":"Article","headline":"Second"}"#;
    let (document, result) = read_scripts(&[&deep, second]);
    // oracle: html5tree.parse_document(html) -> extract_meta_json(tree, Document())
    assert_eq!(result, Err(JsonFailure::Raised));
    assert_eq!(document.title, None);
}

#[test]
fn long_integer_json_ld_stops_the_script_loop() {
    let long = format!(
        r#"{{"@context":"https://schema.org","@type":"Article","headline":"T","x":{}}}"#,
        "9".repeat(5_000)
    );
    let second = r#"{"@context":"https://schema.org","@type":"Article","headline":"Second"}"#;
    let (document, result) = read_scripts(&[&long, second]);
    // oracle: html5tree.parse_document(html) -> extract_meta_json(tree, Document())
    assert_eq!(result, Err(JsonFailure::Raised));
    assert_eq!(document.title, None);
}

#[test]
fn overlong_charref_in_meta_escapes() {
    let digits = "9".repeat(5_000);
    // `&amp;` leaves the charref in the attribute value for `unescape` to meet
    for head in [
        format!(r#"<meta name="author" content="&amp;#{digits};">"#),
        format!(r#"<meta property="article:tag" content="&amp;#{digits};">"#),
        format!(r#"<meta property="og:author" content="&amp;#{digits};">"#),
    ] {
        let tree = page(&head);
        // oracle: html5tree.parse_document(html) -> examine_meta(tree) raises ValueError
        assert_eq!(
            examine_meta(&tree, tree.root(), &Call::default()).unwrap_err(),
            invalid_value()
        );
    }
}

#[test]
fn overlong_charref_in_script_is_caught() {
    let digits = "9".repeat(5_000);
    // reaches `unescape` in the `normalize_json` of the minified text
    let in_text = format!(r#"{{"a":"\n&#{digits};"}}"#);
    // no backslash, so the text parses or fails alone, and the author search reaches `unescape`
    let in_parse_error = format!(r#"{{"@type":"Person","name":"Jo &#{digits};""#);
    for text in [in_text, in_parse_error] {
        let (_, result) = read_scripts(&[&text]);
        // oracle: html5tree.parse_document(html) -> extract_meta_json(tree, Document()) raises ValueError
        assert_eq!(result, Err(JsonFailure::Raised));
    }
}

#[test]
fn cancelled_call_stops_every_reader() {
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    let tree = scripts_page(&[r#"{"@context":"https://schema.org","@type":"Article"}"#]);
    let root = tree.root();
    assert_eq!(
        extract_opengraph(&tree, root, &call).unwrap_err(),
        ExtractError::Cancelled
    );
    assert_eq!(
        examine_meta(&tree, root, &call).unwrap_err(),
        ExtractError::Cancelled
    );
    assert_eq!(
        extract_meta_json(&tree, root, &mut Document::new(), &call),
        Err(JsonFailure::Stop(ExtractError::Cancelled))
    );
}
