use std::collections::HashSet;

use super::page::{
    HTMLTITLE_REGEX, LICENSE_REGEX, TEXT_LICENSE_REGEX, examine_title_element, extract_metainfo,
};
use super::*;
use crate::cancel::{CancelToken, assert_every_check_stops};
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
        r#"<script type="text/javascript">{"@context":"https://schema.org","@type":"Article","headline":"Skipped"}</script>
        <script type="application/settings+json">{"@context":"https://schema.org","@type":"Article","headline":"One"}</script>
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

fn document(head: &str, body: &str) -> Tree {
    parse(&format!(
        "<html><head>{head}</head><body>{body}</body></html>"
    ))
}

fn paragraphs(tree: &Tree, root: NodeId, call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    collect_nodes(tree, root, call, |node| tree.tag(node) == Some("p"))
}

fn title_of(head: &str, body: &str) -> Option<String> {
    let mut tree = document(head, body);
    let root = tree.root();
    extract_title(&mut tree, root, &Call::default()).unwrap()
}

fn url_of(head: &str, default_url: Option<&str>) -> Option<String> {
    let tree = document(head, "");
    extract_url(&tree, tree.root(), default_url, &Call::default()).unwrap()
}

fn sitename_of(head: &str) -> Option<String> {
    let tree = document(head, "");
    extract_sitename(&tree, tree.root(), &Call::default()).unwrap()
}

fn catstags_of(metatype: &str, head: &str, body: &str) -> Vec<String> {
    let mut tree = document(head, body);
    let root = tree.root();
    extract_catstags(metatype, &mut tree, root, &Call::default()).unwrap()
}

fn license_of(body: &str) -> Option<String> {
    let mut tree = document("", body);
    let root = tree.root();
    extract_license(&mut tree, root, &Call::default()).unwrap()
}

fn owned(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

#[test]
fn title_single_h1_wins() {
    // oracle: extract_title(parse_document(html))
    assert_eq!(
        title_of(
            "<title>Site | Page</title>",
            "<h1>Only  Heading</h1><h2>Two</h2>"
        )
        .as_deref(),
        Some("Only Heading")
    );
}

#[test]
fn title_from_title_element_parts() {
    // oracle: extract_title(parse_document(html)) and examine_title_element(parse_document(html))
    for (title, expected) in [
        ("Some Article - Example.com", "Some Article"),
        ("Example.com - Fine Title", "Fine Title"),
        ("A.b - C.d", "A.b - C.d"),
        ("Plain Title", "Plain Title"),
    ] {
        assert_eq!(
            title_of(&format!("<title>{title}</title>"), "<p>x</p>").as_deref(),
            Some(expected),
            "{title}"
        );
    }
    let tree = document("<title>Some Article - Example.com</title>", "");
    assert_eq!(
        examine_title_element(&tree, tree.root(), &Call::default()).unwrap(),
        (
            "Some Article - Example.com".to_owned(),
            Some("Some Article".to_owned()),
            Some("Example.com".to_owned())
        )
    );
    let tree = document("<title>Plain Title</title>", "");
    assert_eq!(
        examine_title_element(&tree, tree.root(), &Call::default()).unwrap(),
        ("Plain Title".to_owned(), None, None)
    );
}

#[test]
fn title_falls_back_to_first_h2() {
    let head = "<title>x.y - z.w</title>";
    // oracle: extract_title(parse_document(html))
    assert_eq!(
        title_of(
            head,
            "<h1></h1><h1> </h1><h2>Second  Head</h2><h2>Other</h2>"
        )
        .as_deref(),
        Some("Second Head")
    );
    assert_eq!(title_of(head, "<h1></h1><h1> </h1>"), None);
    assert_eq!(title_of(head, "<h1></h1>"), None);
    assert_eq!(
        title_of(head, "<h1></h1><h1>Second H</h1>").as_deref(),
        Some("Second H")
    );
    assert_eq!(
        title_of("<title>Ti Tle</title>", "<h1>First H</h1><h1>Second H</h1>").as_deref(),
        Some("Ti Tle")
    );
}

#[test]
fn metainfo_length_bounds() {
    // oracle: extract_metainfo(parse_document(html), [XPath(".//p")]) with an 'é' and n-1 'a'
    for (length, expected) in [(2, false), (3, true), (199, true), (200, false)] {
        let text = format!("é{}", "a".repeat(length - 1));
        let tree = document("", &format!("<p>{text}</p>"));
        let found =
            extract_metainfo(&tree, tree.root(), &[paragraphs], 200, &Call::default()).unwrap();
        assert_eq!(found, expected.then_some(text), "length {length}");
    }
    // oracle: extract_metainfo(tree, [XPath(".//p")], len_limit=120) for 120 and 119 'a'
    for (length, expected) in [(120, false), (119, true)] {
        let text = "a".repeat(length);
        let tree = document("", &format!("<p>{text}</p>"));
        let found =
            extract_metainfo(&tree, tree.root(), &[paragraphs], 120, &Call::default()).unwrap();
        assert_eq!(
            found,
            expected.then_some(text),
            "limit 120, length {length}"
        );
    }
}

#[test]
fn metainfo_joins_the_text_pieces_with_spaces() {
    // oracle: extract_metainfo(parse_document("<p>ab<i></i>c</p>"), [XPath(".//p")])
    let tree = document("", "<p>ab<i></i>c</p>");
    assert_eq!(
        extract_metainfo(&tree, tree.root(), &[paragraphs], 200, &Call::default())
            .unwrap()
            .as_deref(),
        Some("ab c")
    );
}

fn short_bold(tree: &Tree, root: NodeId, call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    collect_nodes(tree, root, call, |node| tree.tag(node) == Some("b"))
}

thread_local! {
    // A selector is a fn pointer and cannot capture the test's token; each test thread has its own.
    static STOP_AFTER_SELECT: CancelToken = CancelToken::new();
}

/// Selects the paragraphs with the call still live, then cancels it. The stop lands after the
/// selection, so the only check that can see it is the one before the match is joined.
fn cancel_after_paragraphs(
    tree: &Tree,
    root: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    let found = paragraphs(tree, root, call)?;
    STOP_AFTER_SELECT.with(CancelToken::cancel);
    Ok(found)
}

#[test]
fn metainfo_checks_the_call_before_each_match_is_joined() {
    // The first match is too short, so the loop moves on to the second expression; its match is
    // joined only after the call was cancelled.
    let tree = document("", "<b>a</b><p>valid text</p>");
    let token = STOP_AFTER_SELECT.with(|token| token.clone());
    let call = Call::default().cancel(&token);
    assert_eq!(
        extract_metainfo(
            &tree,
            tree.root(),
            &[short_bold, cancel_after_paragraphs],
            200,
            &call
        ),
        Err(ExtractError::Cancelled)
    );
}

#[test]
fn author_discards_comment_sections() {
    let tree = document(
        "",
        r#"<div class="comments"><span class="author">Troll Person</span></div>
        <time class="author">Time Author</time><p class="byline">By Real Author</p>"#,
    );
    let (before, size) = (tree.tostring(tree.root(), false, true), tree.len());
    // oracle: extract_author(tree); tostring(tree) is the same afterwards
    assert_eq!(
        extract_author(&tree, tree.root(), &Call::default())
            .unwrap()
            .as_deref(),
        Some("Real Author")
    );
    assert_eq!(tree.tostring(tree.root(), false, true), before);
    assert_eq!(tree.len(), size);

    let tree = document(
        "",
        r#"<div class="comments"><span class="author">Troll Person</span></div>
        <time class="author">Time Author</time>"#,
    );
    // oracle: extract_author(tree)
    assert_eq!(
        extract_author(&tree, tree.root(), &Call::default()).unwrap(),
        None
    );
}

#[test]
fn url_canonical_relative_and_protocol_relative() {
    // oracle: extract_url(parse_document(html), default_url)
    assert_eq!(
        url_of(
            r#"<meta property="og:url" content="https://example.org/base"><link rel="canonical" href="/p">"#,
            None
        )
        .as_deref(),
        Some("https://example.org/p")
    );
    assert_eq!(
        url_of(
            r#"<meta name="author" content="https://x.example/a"><meta name="twitter:site" content="https://tw.example/b"><link rel="canonical" href="/p?utm_source=a&b=1">"#,
            None
        )
        .as_deref(),
        Some("https://tw.example/p?b=1")
    );
    assert_eq!(
        url_of(
            r#"<link rel="canonical" href="/p">"#,
            Some("https://dflt.example/d")
        )
        .as_deref(),
        Some("https://dflt.example/d")
    );
    assert_eq!(
        url_of(r#"<link rel="canonical" href="//h.example.org/p">"#, None).as_deref(),
        Some("https://h.example.org/p")
    );
}

#[test]
fn url_invalid_canonical_returns_the_default_unvalidated() {
    let head = r#"<link rel="canonical" href="nope">"#;
    // oracle: extract_url(parse_document(html), default_url)
    assert_eq!(
        url_of(head, Some("not valid")).as_deref(),
        Some("not valid")
    );
    assert_eq!(url_of(head, None), None);
    assert_eq!(url_of("", None), None);
    assert_eq!(url_of("", Some("")).as_deref(), Some(""));
}

#[test]
fn url_selectors_in_order() {
    // oracle: extract_url(parse_document(html))
    assert_eq!(
        url_of(
            r#"<link rel="canonical" href=""><base href="https://b.example.org/x">
            <link rel="alternate" hreflang="x-default" href="https://alt.example.org/">"#,
            None
        )
        .as_deref(),
        Some("https://b.example.org/x")
    );
    assert_eq!(
        url_of(
            r#"<link rel="alternate" hreflang="x-default" href="https://alt.example.org/y">"#,
            None
        )
        .as_deref(),
        Some("https://alt.example.org/y")
    );
}

#[test]
fn sitename_needs_a_dot() {
    // oracle: extract_sitename(parse_document(html))
    for (title, expected) in [
        ("Page - example.org", Some("example.org")),
        ("Page - Example", None),
        ("example.org: Page", None),
        ("a.b - c.d", Some("a.b")),
        ("No separator.com", None),
    ] {
        assert_eq!(
            sitename_of(&format!("<title>{title}</title>")).as_deref(),
            expected,
            "{title}"
        );
    }
    assert_eq!(sitename_of(""), None);
}

#[test]
fn catstags_href_regex_and_dedupe() {
    // oracle: extract_catstags(metatype, parse_document(html))
    let links = r#"<div class="entry-meta"><a href="/category/news">News</a> <a href="/categories/Tech">Tech</a>
        <a href="/categories/news">News</a> <a href="/tags/x">Tag X</a> <a href="/category/empty"> </a></div>"#;
    assert_eq!(catstags_of("category", "", links), owned(&["News", "Tech"]));
    assert_eq!(
        catstags_of(
            "category",
            "",
            r#"<div class="entry-meta"><a href="/categorys/a">A</a><a href="/categoryies/b">B</a>
            <a href="/categor/c">C</a><a href="/other/d">D</a></div>"#
        ),
        owned(&["C"])
    );
    assert_eq!(
        catstags_of(
            "tag",
            "",
            r#"<div class="meta"><a href="/tag/a">A</a><a href="/tags/b">B</a><a href="/tagy/c">C</a>
            <a href="/tagies/d">D</a><a href="/tags/b">B</a></div>"#
        ),
        owned(&["A", "B", "C", "D"])
    );
}

#[test]
fn catstags_article_section_fallback_is_for_categories_only() {
    // oracle: extract_catstags(metatype, parse_document(html))
    let head = r#"<meta property="article:section" content="Sec"><meta name="my-subject" content="Subj">
        <meta name="subject"><meta property="article:section" content="Sec">"#;
    assert_eq!(
        catstags_of("category", head, "<p>x</p>"),
        owned(&["Sec", "Subj"])
    );
    assert_eq!(
        catstags_of(
            "tag",
            r#"<meta property="article:section" content="Sec">"#,
            "<p>x</p>"
        ),
        Vec::<String>::new()
    );
}

#[test]
fn license_rel_and_footer() {
    // oracle: extract_license(parse_document(html))
    for (body, expected) in [
        (
            r#"<a rel="nofollow License" href="https://creativecommons.org/licenses/by-nc/4.0/">x</a>"#,
            Some("CC BY-NC 4.0"),
        ),
        (
            r#"<a rel="license" href="/terms"><b>Our</b> Terms</a>"#,
            Some("Our Terms"),
        ),
        (
            r#"<a rel="licensed" href="/terms">Nope</a><a rel="license" href="/t"> </a>"#,
            None,
        ),
        (
            r#"<footer><a href="/licenses/by-sa/3.0/">x</a></footer>"#,
            Some("CC BY-SA 3.0"),
        ),
        (
            r#"<a rel="license" href="/publicdomain/zero/1.0/">x</a>"#,
            Some("CC ZERO 1.0"),
        ),
        (
            r#"<footer><a href="/x">Terms</a><a href="/y">Licensed under Creative Commons BY-SA 4.0 here</a></footer>"#,
            Some("Creative Commons BY-SA 4.0"),
        ),
        (
            r#"<div class="page-footer"><a href="/y">cc by-nc</a></div>"#,
            Some("cc by-nc"),
        ),
        (
            r#"<div id="myfooter"><a href="/y">CC BY 2.5</a></div>"#,
            Some("CC BY 2.5"),
        ),
        (
            r#"<footer><a href="/x">Terms</a></footer><div class="foot"><a href="/y">CC BY</a></div>"#,
            None,
        ),
        (
            r#"<footer><a href="/y">CC BY 2.5</a></footer><a rel="license" href="/t">Plain</a>"#,
            Some("Plain"),
        ),
    ] {
        assert_eq!(license_of(body).as_deref(), expected, "{body}");
    }
}

#[test]
fn tree_readers_stop_when_cancelled() {
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    let mut tree = document("<title>a</title>", "<h1>Heading</h1><p>x</p>");
    let root = tree.root();
    assert_eq!(
        extract_title(&mut tree, root, &call),
        Err(ExtractError::Cancelled)
    );
    assert_eq!(
        extract_sitename(&tree, root, &call),
        Err(ExtractError::Cancelled)
    );
    assert_eq!(
        extract_url(&tree, root, None, &call),
        Err(ExtractError::Cancelled)
    );
    assert_eq!(
        extract_author(&tree, root, &call),
        Err(ExtractError::Cancelled)
    );
    assert_eq!(
        extract_catstags("tag", &mut tree, root, &call),
        Err(ExtractError::Cancelled)
    );
    assert_eq!(
        extract_license(&mut tree, root, &call),
        Err(ExtractError::Cancelled)
    );
}

#[test]
fn tree_field_patterns_compile() {
    assert!(HTMLTITLE_REGEX.is_some());
    assert!(LICENSE_REGEX.is_some());
    assert!(TEXT_LICENSE_REGEX.is_some());
    assert!(META_URL.is_some());
}

#[test]
fn title_element_separators() {
    // oracle: HTMLTITLE_REGEX.pattern class code points; examine_title_element(parse_document(html))[1:]
    for code in [
        0x2013, 0x2022, 0xb7, 0x2014, 0x7c, 0x2044, 0x2a, 0x22c6, 0x7e, 0x2039, 0xab, 0x3c, 0x203a,
        0xbb, 0x3e, 0x3a, 0x2d,
    ] {
        let separator = char::from_u32(code).unwrap();
        let tree = document(&format!("<title>Aa {separator} Bb</title>"), "");
        assert_eq!(
            examine_title_element(&tree, tree.root(), &Call::default())
                .unwrap()
                .1
                .as_deref(),
            Some("Aa"),
            "{code:x}"
        );
    }
    for code in [0x2f, 0x2b, 0x2c, 0x5f, 0x2212, 0x2015] {
        let separator = char::from_u32(code).unwrap();
        let tree = document(&format!("<title>Aa {separator} Bb</title>"), "");
        assert_eq!(
            examine_title_element(&tree, tree.root(), &Call::default())
                .unwrap()
                .1,
            None,
            "{code:x}"
        );
    }
}

#[test]
fn title_element_parts_follow_the_last_separator_and_need_both_sides() {
    // oracle: examine_title_element(parse_document(html))
    for (title, expected) in [
        (
            "Aa - Bb - Cc",
            ("Aa - Bb - Cc", Some("Aa - Bb"), Some("Cc")),
        ),
        ("- Bb", ("- Bb", None, None)),
        ("Aa -", ("Aa -", None, None)),
    ] {
        let tree = document(&format!("<title>{title}</title>"), "");
        let (full, first, second) =
            examine_title_element(&tree, tree.root(), &Call::default()).unwrap();
        assert_eq!(
            (full.as_str(), first.as_deref(), second.as_deref()),
            expected,
            "{title}"
        );
    }
}

#[test]
fn title_element_outside_the_head_is_ignored() {
    // oracle: examine_title_element(parse_document(html))
    let tree = document("", "<title>Aa - Bb</title>");
    assert_eq!(
        examine_title_element(&tree, tree.root(), &Call::default()).unwrap(),
        (String::new(), None, None)
    );
}

fn run_extract(html: &str, date_config: Option<&DateParams>) -> Document {
    let mut tree = parse(html);
    let root = tree.root();
    extract_metadata(
        &mut tree,
        root,
        None,
        date_config,
        false,
        &HashSet::new(),
        &Call::default(),
    )
    .unwrap()
}

#[test]
fn extract_metadata_sets_filedate_from_max_date() {
    let config = DateParams {
        original_date: true,
        extensive_search: false,
        max_date: Some("2024-01-31".to_owned()),
    };
    // oracle: extract_metadata(tree, date_config={"max_date": "2024-01-31", ...}).filedate
    let document = run_extract("<html><head></head><body></body></html>", Some(&config));
    assert_eq!(document.filedate.as_deref(), Some("2024-01-31"));
}

#[test]
fn extract_metadata_without_date_config_takes_today_utc() {
    // D-15: without a date config, max_date is today's UTC date, and filedate is that date
    let before = today_utc();
    let document = run_extract("<html><head></head><body></body></html>", None);
    let after = today_utc();
    let filedate = document.filedate.unwrap();
    assert!(filedate == before || filedate == after, "{filedate}");
}

#[test]
fn extract_metadata_stops_on_cancellation() {
    let token = CancelToken::new();
    token.cancel();
    let mut tree =
        parse(r#"<html><head><script type="application/ld+json">{}</script></head></html>"#);
    let root = tree.root();
    let call = Call::default().cancel(&token);
    assert_eq!(
        extract_metadata(&mut tree, root, None, None, false, &HashSet::new(), &call).err(),
        Some(ExtractError::Cancelled)
    );
}

#[test]
fn extract_metadata_stops_inside_the_json_readers() {
    // The JSON-LD reader makes checks of its own; a trip there must end the call at that check,
    // so the sweep fails if the Stop arm swallows the error and a later reader checks again.
    let article = r#"{"@context":"https://schema.org","@type":"Article","headline":"A","author":{"name":"Jane Doe"}}"#;
    let total = assert_every_check_stops(|call| {
        let mut tree = scripts_page(&[article]);
        let root = tree.root();
        extract_metadata(&mut tree, root, None, None, false, &HashSet::new(), call)
    });
    assert!(total >= 2, "checks of the run: {total}");
}

#[test]
fn url_base_comes_from_meta_elements_only() {
    // oracle: extract_url(parse_document(html), None): the `link` with a `content` and an og: name is not a meta
    assert_eq!(
        url_of(
            r#"<link name="og:x" content="https://bad.example/a"><meta property="og:url" content="https://good.example/b"><link rel="canonical" href="/p">"#,
            None
        )
        .as_deref(),
        Some("https://good.example/p")
    );
}

#[test]
fn title_after_closing_html() {
    // oracle: scripts/oracle.sh 'extract_metadata(load_html(page)).title' gives 'Big Title' (lxml 6.1.3,
    // libxml2 2.14.6): the h1 after the closing html is still in the body
    let html =
        r#"<html><head></head><body><p>x</p></body></html><h1 class="post-title">Big Title</h1>"#;
    let mut tree = crate::utils::load_html(
        crate::utils::HtmlInput::Str(html),
        crate::settings::MAX_FILE_SIZE,
        &ParseLimits::default(),
        &Call::default(),
    )
    .unwrap();
    let root = tree.root();
    let document = extract_metadata(
        &mut tree,
        root,
        None,
        None,
        false,
        &HashSet::new(),
        &Call::default(),
    )
    .unwrap();
    assert_eq!(document.title.as_deref(), Some("Big Title"));
}
