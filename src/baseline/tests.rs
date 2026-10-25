#![allow(clippy::unwrap_used)]

use std::time::Instant;

use super::*;
use crate::cancel::CancelToken;
use crate::pyjson::loads;

/// A page whose only content is one JSON-LD script.
fn page(payload: &str) -> String {
    format!(
        r#"<html><head><script type="application/ld+json">{payload}</script></head><body></body></html>"#
    )
}

fn text_of(doc: &str) -> String {
    baseline(
        HtmlInput::Str(doc),
        &ParseLimits::default(),
        &Call::default(),
    )
    .unwrap()
    .2
}

fn lines(text: &str) -> Vec<&str> {
    text.lines().collect()
}

/// `html.escape(s, quote=True)` for the characters an attribute value needs.
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// `{"articleBody": "Body 1", "@graph": {"articleBody": "Body 2", "@graph": ...}}`, `n` levels.
fn chain(n: usize) -> String {
    let mut chain = format!(r#"{{"articleBody": "Body {n}"}}"#);
    for level in (1..n).rev() {
        chain = format!(r#"{{"articleBody": "Body {level}", "@graph": {chain}}}"#);
    }
    chain
}

fn bodies_up_to(n: usize) -> Vec<String> {
    (1..=n).map(|level| format!("Body {level}")).collect()
}

fn texts(text: &str) -> Vec<String> {
    lines(text).into_iter().map(str::to_string).collect()
}

const SIBLING: &str =
    "SIB sibling body text that follows the chain, long enough to count on its own for the gate.";

#[test]
fn walk_json_cap_keeps_partial_bodies() {
    // oracle: len(re.findall(r"Body \d+", extract(page(chain(1000)), fast=True))) == 992, with
    // baseline(page(chain(1000))) giving 996 on its shallower stack (D-10 takes 992)
    let text = text_of(&page(&chain(1000)));
    assert_eq!(texts(&text), bodies_up_to(992));
}

#[test]
fn walk_json_cap_on_leaf_dict_drops_sibling() {
    // oracle: extract(page('[%s, {"articleBody": "SIB..."}]' % chain(992)), fast=True) has 992
    // bodies and no "SIB"; with chain(991) it has 991 bodies and "SIB"
    let text = text_of(&page(&format!(
        r#"[{}, {{"articleBody": "{SIBLING}"}}]"#,
        chain(992)
    )));
    assert_eq!(texts(&text), bodies_up_to(992));

    let text = text_of(&page(&format!(
        r#"[{}, {{"articleBody": "{SIBLING}"}}]"#,
        chain(991)
    )));
    let mut expected = bodies_up_to(991);
    expected.push(SIBLING.to_string());
    assert_eq!(texts(&text), expected);
}

fn first() -> String {
    format!("FIRST {}", "a".repeat(114))
}

fn next() -> String {
    format!("NEXT {}", "b".repeat(114))
}

#[test]
fn type_search_cap() {
    let (first, next) = (first(), next());
    // `@type` of `k` lists around "Product", in the first of two items
    let around = |k: usize| {
        let value = format!(r#"{}"Product"{}"#, "[".repeat(k), "]".repeat(k));
        text_of(&page(&format!(
            r#"[{{"articleBody": "{first}", "@type": {value}, "description": "DESC"}}, {{"articleBody": "{next}"}}]"#
        )))
    };
    // oracle: baseline(page(...))[1] for k = 9995 and 9996: (FIRST, NEXT) kept, then FIRST only
    assert_eq!(texts(&around(9995)), [first.clone(), next.clone()]);
    assert_eq!(texts(&around(9996)), std::slice::from_ref(&first));

    // `["Product", <k nested lists>]`: the needle comes first, the whole `str` is built anyway
    let list_first = |k: usize| {
        let value = format!(r#"["Product", {}{}]"#, "[".repeat(k), "]".repeat(k));
        text_of(&page(&format!(
            r#"{{"articleBody": "{first}", "@type": {value}, "description": "DESC", "@graph": {{"articleBody": "{next}"}}}}"#
        )))
    };
    // oracle: baseline(page(...))[1] for k = 9995 and 9996, as above
    assert_eq!(texts(&list_first(9995)), [first.clone(), next.clone()]);
    assert_eq!(texts(&list_first(9996)), std::slice::from_ref(&first));

    // the teaser is lost with the cut
    let desc = format!("DESC {}", "c".repeat(120));
    let teaser = |k: usize| {
        let value = format!(r#"{}"Product"{}"#, "[".repeat(k), "]".repeat(k));
        text_of(&page(&format!(
            r#"{{"@type": {value}, "description": "{desc}"}}"#
        )))
    };
    // oracle: baseline(page(...))[1] for k = 9995 and 9996: the teaser, then ""
    assert_eq!(teaser(9995), desc);
    assert_eq!(teaser(9996), "");
}

const PARA: &str = "Real paragraph content that should be extracted by the paragraph strategy, comfortably long enough for the gate.";

/// A Discourse page with one topic whose `posts` is the JSON `posts`, and a paragraph.
fn discourse_page(posts: &str) -> String {
    let topic = format!(r#"{{"post_stream": {{"posts": {posts}}}}}"#);
    let preloaded = escape(&format!(r#"{{"topic_12": {}}}"#, serde_json_string(&topic)));
    format!(
        r#"<html><body><div id="data-preloaded" data-preloaded="{preloaded}"></div><p>{PARA}</p></body></html>"#
    )
}

/// `json.dumps(s)` for ASCII text without control characters.
fn serde_json_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

#[test]
fn discourse_posts_not_iterable_is_invalid_value() {
    // oracle: baseline(page) raises TypeError for posts null, 5, true, 1.5 and false (extract
    // gives None); for "ab", {"a": 1} and [] it returns the paragraph text
    for posts in ["null", "5", "true", "1.5", "false"] {
        let result = baseline(
            HtmlInput::Str(&discourse_page(posts)),
            &ParseLimits::default(),
            &Call::default(),
        );
        assert_eq!(
            result.map(|found| found.2),
            Err(ExtractError::Rejected(Rejection::InvalidValue)),
            "posts {posts}"
        );
    }
    for posts in [r#""ab""#, r#"{"a": 1}"#, "[]"] {
        assert_eq!(text_of(&discourse_page(posts)), PARA, "posts {posts}");
    }
}

#[test]
fn discourse_surrogate_topic_keys_both_count() {
    // a lone surrogate escape in two keys keeps them distinct, as Python does
    let escape_u = |hex: &str| format!("{}u{hex}", '\\');
    let first_post = "First forum post with a good amount of substantive discussion content in it.";
    let second_post =
        "Second forum post continuing the thread with more useful information for everyone.";
    let topic = |post: &str| {
        serde_json_string(&format!(
            r#"{{"post_stream": {{"posts": [{{"cooked": "<p>{post}</p>"}}]}}}}"#
        ))
    };
    let preloaded = format!(
        r#"{{"topic_{}": {}, "topic_{}": {}}}"#,
        escape_u("d800"),
        topic(first_post),
        escape_u("d801"),
        topic(second_post)
    );
    let doc = format!(
        r#"<html><body><div id="data-preloaded" data-preloaded="{}"></div></body></html>"#,
        escape(&preloaded)
    );
    let (_, _, text, len) = baseline(
        HtmlInput::Str(&doc),
        &ParseLimits::default(),
        &Call::default(),
    )
    .unwrap();
    // oracle: baseline(doc)[1] and its length 159; extract(doc, fast=True) gives the same text
    assert_eq!(text, format!("{first_post}\n{second_post}"));
    assert_eq!(len, 159);
}

fn has_markup(raw: &str) -> bool {
    HTML_MARKUP
        .as_ref()
        .is_some_and(|markup| markup.is_match(raw))
}

#[test]
fn html_markup_folds_like_python() {
    let ch = |code: u32| char::from_u32(code).unwrap();
    // oracle: bool(_HTML_MARKUP.search(row))
    let rows = [
        (format!("<d{}v>", ch(0x130)), true),
        (format!("<d{}v>", ch(0x131)), true),
        (format!("<{}pan>", ch(0x17f)), true),
        (format!("<{}bd>", ch(0x212a)), true),
        (format!("<D{0}V>x</D{0}V>", ch(0x130)), true),
        ("<p>x</p>".to_string(), true),
        ("</P>".to_string(), true),
        ("<br/>".to_string(), true),
        (r#"<a href="x">"#.to_string(), true),
        (format!("<a{}href=x>", ch(0x1c)), true),
        (format!("<p{}=>", ch(0x1c)), true),
        ("<p class=x>".to_string(), true),
        ("a<b and c>d".to_string(), false),
        ("x<y and z>0".to_string(), false),
        ("<p foo>".to_string(), false),
        ("</kbd >".to_string(), false),
    ];
    for (raw, expected) in rows {
        assert_eq!(has_markup(&raw), expected, "{raw:?}");
    }
}

#[test]
fn json_hooks_follow_walked_keys() {
    // upstream builds the hooks from the keys `walk_json` reads
    let mut expected: Vec<String> = JSON_TEXT_KEYS.iter().map(|key| key.to_string()).collect();
    expected.extend(["recipeInstructions", "acceptedAnswer"].map(String::from));
    expected.extend(
        DESCRIPTION_TYPES
            .iter()
            .chain(&["HowTo"])
            .map(|kind| format!("\"{kind}\"")),
    );
    assert_eq!(JSON_HOOKS.map(String::from).to_vec(), expected);
}

#[test]
fn walk_json_rows() {
    // oracle: b, t = [], []; _walk_json(json.loads(row), b, t) gives (b, t)
    let rows: [(&str, &[&str], &[&str]); 19] = [
        (r#"{"@type":"Product","description":"d"}"#, &[], &["d"]),
        (
            r#"{"@type":["Thing","VideoObject"],"description":"d"}"#,
            &[],
            &["d"],
        ),
        (
            r#"{"@type":{"k":"xProductx"},"description":"d"}"#,
            &[],
            &["d"],
        ),
        (r#"{"@type":{"Product":1},"description":"d"}"#, &[], &["d"]),
        (r#"{"@type":5,"description":"d"}"#, &[], &[]),
        (r#"{"@type":null,"description":"d"}"#, &[], &[]),
        (r#"{"@type":"Product","description":5}"#, &[], &[]),
        (r#"{"@type":"Product"}"#, &[], &[]),
        (
            r#"{"articleBody":"A","mainEntity":{"articleBody":"M"},"@graph":{"articleBody":"G"}}"#,
            &["A", "G", "M"],
            &[],
        ),
        (
            r#"{"recipeInstructions":["s1","",{"text":"s2","itemListElement":[{"text":"s3"},{"text":3},"x"]},5]}"#,
            &["s1", "", "s2", "s3"],
            &[],
        ),
        (
            r#"{"step":{"text":"t1","itemListElement":{"text":"t2"}}}"#,
            &["t1", "t2"],
            &[],
        ),
        (
            r#"{"acceptedAnswer":{"text":"ans"},"@type":"Question"}"#,
            &["ans"],
            &[],
        ),
        (r#"{"acceptedAnswer":[{"text":"ans"}]}"#, &[], &[]),
        (r#"{"articleBody":"","reviewBody":"r"}"#, &["r"], &[]),
        (
            r#"[1,{"articleBody":"a"},[{"articleBody":"nested list"}],null]"#,
            &["a"],
            &[],
        ),
        (
            r#"{"articleBody":"first","articleBody":"last"}"#,
            &["last"],
            &[],
        ),
        (r#"{"articleBody":5,"reviewBody":["x"]}"#, &[], &[]),
        (
            r#"{"@graph":[{"articleBody":"g1","mainEntity":{"articleBody":"m1"}},{"articleBody":"g2"}],"mainEntity":[{"articleBody":"m2"}]}"#,
            &["g1", "m1", "g2", "m2"],
            &[],
        ),
        (r#""just a string""#, &[], &[]),
    ];
    let call = Call::default();
    for (row, expected_bodies, expected_teasers) in rows {
        let json = loads(row, false, &call).unwrap().unwrap();
        let (mut bodies, mut teasers) = (Vec::new(), Vec::new());
        walk_json(&json, Some(json.root()), &mut bodies, &mut teasers, &call).unwrap();
        assert_eq!(bodies, expected_bodies, "{row}");
        assert_eq!(teasers, expected_teasers, "{row}");
    }
}

#[test]
fn render_text_rows() {
    // oracle: _render_text(raw) on the html5 parse
    let rows = [
        ("  plain   text ", "plain text"),
        ("<p>Hello</p><p>World</p>", "Hello World"),
        ("Opening<br/>more<div>in div</div>", "Opening more in div"),
        ("&lt;p&gt;esc&lt;/p&gt;", "esc"),
        ("x<y and z>0", "x<y and z>0"),
        ("a&#1;b", "ab"),
        ("&#128;<b>x</b>", "\u{20ac}x"),
        ("<b>bold</b> and <i>it</i>", "bold and it"),
        ("line1\n<p>l2</p>\ttail", "line1 l2 tail"),
        ("&#x1;<p>x</p>", "x"),
    ];
    let call = Call::default();
    let limits = ParseLimits::default();
    for (raw, expected) in rows {
        assert_eq!(
            render_text(raw, &limits, &call).unwrap(),
            expected,
            "{raw:?}"
        );
    }
    // oracle: _render_text("<p>a</p>&#" + "0" * 4299 + "65;") raises ValueError (4301 digits)
    let long_reference = format!("<p>a</p>&#{}65;", "0".repeat(4299));
    assert_eq!(
        render_text(&long_reference, &limits, &call),
        Err(ExtractError::Rejected(Rejection::InvalidValue))
    );
}

#[test]
fn render_text_falls_back_on_tree_too_large() {
    let raw = "<p>x</p>\n".repeat(12);
    let call = Call::default();
    // oracle: _render_text(raw) is the 12 "x" joined by spaces: the fragment parses upstream
    assert_eq!(
        render_text(&raw, &ParseLimits::default(), &call).unwrap(),
        ["x"; 12].join(" ")
    );
    // deviation: D-5
    // oracle: trim(raw), what upstream's `except Exception` falls back to on a failed parse
    let limits = ParseLimits {
        max_nodes: Some(10),
        ..ParseLimits::default()
    };
    assert_eq!(
        render_text(&raw, &limits, &call).unwrap(),
        ["<p>x</p>"; 12].join(" ")
    );
}

#[test]
fn render_text_propagates_cancel() {
    let token = CancelToken::new();
    token.cancel();
    let limits = ParseLimits::default();
    assert_eq!(
        render_text("<p>a</p>", &limits, &Call::default().cancel(&token)),
        Err(ExtractError::Cancelled)
    );
    assert_eq!(
        render_text(
            "<p>a</p>",
            &limits,
            &Call::default().deadline(Instant::now())
        ),
        Err(ExtractError::DeadlineExceeded)
    );
}

#[test]
fn build_body_checks_call_per_text() {
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    let mut tree = Tree::new(LocalName::from("html"), Ns::Html, Vec::new());
    let texts = vec!["one".to_string(), "two".to_string(), "three".to_string()];
    assert_eq!(
        build_body(&mut tree, texts, false, DEDUPE_SCAN_CAP, &call).map(|found| found.1),
        Err(ExtractError::Cancelled)
    );
    // nothing to check: no text, no call
    let (body, text) = build_body(&mut tree, Vec::new(), false, DEDUPE_SCAN_CAP, &call).unwrap();
    assert_eq!(tree.child_count(body), 0);
    assert_eq!(text, "");
}

#[test]
fn html2txt_and_baseline_on_empty_load() {
    let limits = ParseLimits::default();
    let call = Call::default();
    // oracle: html2txt("") == "" and baseline("") == (Element("body"), "", 0); for "123" the plain
    // lxml values are the same (`html5_parsers` would parse it: see the Global Constraints)
    for doc in ["", "123"] {
        assert_eq!(
            html2txt(HtmlInput::Str(doc), true, &limits, &call).unwrap(),
            ""
        );
        let (tree, body, text, len) = baseline(HtmlInput::Str(doc), &limits, &call).unwrap();
        assert_eq!(tree.tag(body), Some("body"));
        assert_eq!(text, "");
        assert_eq!(len, 0);
        assert_eq!(tree.child_count(body), 0);
    }
}

#[test]
fn deep_json_on_small_stack() {
    // oracle: baseline(page(...))[1] == body for the 9998-deep document, whose hook matches, so
    // it is parsed, walked and dropped
    let body = format!(
        "Deep JSON body text that the hook matches, long enough to pass the gate: {}",
        "x".repeat(60)
    )
    .chars()
    .take(120)
    .collect::<String>();
    let payload = format!(
        r#"[{{"articleBody": "{body}", "x": {}{}}}]"#,
        "[".repeat(9996),
        "]".repeat(9996)
    );
    let doc = page(&payload);
    let text = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(move || text_of(&doc))
        .unwrap()
        .join()
        .unwrap();
    assert_eq!(text, body);
}
