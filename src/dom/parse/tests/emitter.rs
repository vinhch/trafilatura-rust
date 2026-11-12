use std::sync::Arc;

use html5ever::LocalName;
use html5gum::Emitter as _;

use crate::dom::parse::MAX_TAG_ATTRIBUTES;
use crate::dom::parse::ParseLimits;
use crate::dom::parse::emitter::{Emitter, StartTag, Token, tokens};
use crate::error::{ExtractError, Rejection, TreeLimit};

const CAP_PAGE_TAGS: usize = 350_000;

fn toks(html: &str) -> Vec<Token> {
    let mut emitter = Emitter::new(&ParseLimits::default(), false);
    tokens(html, &mut emitter).collect()
}

/// ` a0 a1 … a{count-1}`, every name distinct.
fn attr_list(count: usize) -> String {
    (0..count).map(|i| format!(" a{i}")).collect()
}

fn tag(name: &str, attrs: &[(&str, &str)], valueless: &[&str]) -> Token {
    Token::Start(StartTag {
        name: LocalName::from(name),
        attrs: attrs
            .iter()
            .map(|&(name, value)| (LocalName::from(name), Arc::from(value)))
            .collect(),
        valueless: valueless
            .iter()
            .map(|&name| LocalName::from(name))
            .collect(),
        self_closing: false,
    })
}

fn start(name: &str) -> Token {
    tag(name, &[], &[])
}

fn self_closing(name: &str) -> Token {
    Token::Start(StartTag {
        name: LocalName::from(name),
        attrs: Vec::new(),
        valueless: Vec::new(),
        self_closing: true,
    })
}

fn end(name: &str) -> Token {
    Token::End(LocalName::from(name))
}

fn text(value: &str) -> Token {
    Token::Text(value.to_owned())
}

fn cut(name: &str) -> Token {
    Token::CutStart(LocalName::from(name))
}

fn too_large(limit: TreeLimit) -> Token {
    Token::Abort(ExtractError::Rejected(Rejection::TreeTooLarge(limit)))
}

fn texts(tokens: &[Token]) -> Vec<&str> {
    tokens
        .iter()
        .filter_map(|token| match token {
            Token::Text(value) => Some(value.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn split_utf8_decodes_whole() {
    let mut emitter = Emitter::new(&ParseLimits::default(), false);
    let mut sink = &mut emitter;
    sink.emit_string(&[0xC3]);
    sink.emit_string(&[0xA9]);
    sink.init_start_tag();
    assert_eq!(sink.pop_token(), Some(text("é")));

    let mut emitter = Emitter::new(&ParseLimits::default(), false);
    let mut sink = &mut emitter;
    sink.init_start_tag();
    sink.init_attribute();
    sink.push_attribute_name(b"t");
    sink.init_attribute_value();
    sink.push_attribute_value(&[0xC3]);
    sink.push_attribute_value(&[0xA9]);
    let _ = sink.emit_current_tag();
    assert_eq!(sink.pop_token(), Some(tag("", &[("t", "é")], &[])));
}

#[test]
fn duplicate_attribute_keeps_first() {
    assert_eq!(
        toks("<p a=1 a=2 b>"),
        vec![tag("p", &[("a", "1"), ("b", "")], &["b"])]
    );
}

#[test]
fn explicit_empty_value_is_not_valueless() {
    assert_eq!(
        toks(r#"<p a="" b c=>"#),
        vec![tag("p", &[("a", ""), ("b", ""), ("c", "")], &["b"])]
    );
}

#[test]
fn attribute_cap_rejects() {
    let page = "<p>".repeat(CAP_PAGE_TAGS);
    let over = format!("<x{}>{page}", attr_list(MAX_TAG_ATTRIBUTES + 1));
    assert_eq!(toks(&over), vec![too_large(TreeLimit::TagAttributes)]);

    let at = format!("<x{}>{page}", attr_list(MAX_TAG_ATTRIBUTES));
    let tokens = toks(&at);
    assert_eq!(tokens.len(), 1 + CAP_PAGE_TAGS);
    assert!(!tokens.iter().any(|token| matches!(token, Token::Abort(_))));
}

#[test]
fn end_tag_attributes_count_toward_the_cap() {
    let page = "<p>".repeat(CAP_PAGE_TAGS);
    let html = format!("</p{}>{page}", attr_list(MAX_TAG_ATTRIBUTES + 1));
    assert_eq!(toks(&html), vec![too_large(TreeLimit::TagAttributes)]);
}

#[test]
fn cut_start_tag_is_reported() {
    assert_eq!(toks("<div class=\"a"), vec![cut("div")]);
    assert_eq!(toks("<div"), vec![cut("div")]);
    assert!(toks("</div").is_empty());
}

#[test]
fn names_cut_at_100_bytes() {
    let long = "a".repeat(120);
    assert_eq!(toks(&format!("<{long}>")), vec![start(&"a".repeat(100))]);

    let attr_name = format!("data-{}", "a".repeat(95));
    let html = format!(
        r#"<p data-{}="1" data-{}{}="2">"#,
        "a".repeat(120),
        "a".repeat(100),
        "b".repeat(20)
    );
    assert_eq!(toks(&html), vec![tag("p", &[(&attr_name, "1")], &[])]);
}

#[test]
fn name_cut_keeps_whole_characters() {
    // 99 bytes of `a` and `é` (2 bytes) is 101 bytes: the character does not fit.
    let name = format!("{}é", "a".repeat(99));
    assert_eq!(toks(&format!("<{name}>")), vec![start(&"a".repeat(99))]);
}

#[test]
fn name_cut_skips_characters_that_do_not_fit() {
    // The `é` does not fit after 99 bytes; the `b` after it does.
    let name = format!("{}éb", "a".repeat(99));
    assert_eq!(
        toks(&format!("<{name}>")),
        vec![start(&format!("{}b", "a".repeat(99)))]
    );
}

#[test]
fn name_nul_is_kept_as_replacement_and_costs_three_bytes() {
    // Tag names reach the emitter with NUL already mapped by html5gum; this pins the rule for a raw NUL.
    let mut emitter = Emitter::new(&ParseLimits::default(), false);
    let mut sink = &mut emitter;
    sink.init_start_tag();
    sink.push_tag_name(b"a\0b");
    let _ = sink.emit_current_tag();
    assert_eq!(sink.pop_token(), Some(start("a\u{fffd}b")));

    // 99 bytes of `a` leave one byte: the three-byte replacement is skipped, the `c` after it fits.
    let mut emitter = Emitter::new(&ParseLimits::default(), false);
    let mut sink = &mut emitter;
    sink.init_start_tag();
    sink.push_tag_name("a".repeat(99).as_bytes());
    sink.push_tag_name(b"\0c");
    let _ = sink.emit_current_tag();
    assert_eq!(
        sink.pop_token(),
        Some(start(&format!("{}c", "a".repeat(99))))
    );
}

#[test]
fn attribute_budget_rejects() {
    let limits = ParseLimits {
        max_nodes: Some(1),
        ..ParseLimits::default()
    };

    let mut emitter = Emitter::new(&limits, false);
    let html = format!("<p{}>", attr_list(11));
    let out: Vec<Token> = tokens(&html, &mut emitter).collect();
    assert_eq!(out, vec![too_large(TreeLimit::MaxNodes)]);

    let mut emitter = Emitter::new(&limits, false);
    let html = format!("<p{}>", attr_list(10));
    let out: Vec<Token> = tokens(&html, &mut emitter).collect();
    assert_eq!(out.len(), 1);
    assert!(matches!(out.first(), Some(Token::Start(_))));
}

#[test]
fn raw_text_switch_by_name() {
    assert_eq!(
        toks("<title><b>x</b></title>"),
        vec![start("title"), text("<b>x</b>"), end("title")]
    );
    assert_eq!(
        toks("<noscript><b>x</b></noscript>"),
        vec![
            start("noscript"),
            start("b"),
            text("x"),
            end("b"),
            end("noscript"),
        ]
    );
    assert_eq!(
        toks("<script/><b>"),
        vec![self_closing("script"), start("b")]
    );
    assert_eq!(
        toks("<plaintext></p>"),
        vec![start("plaintext"), text("</p>")]
    );
}

#[test]
fn unread_text_dropped_only_when_asked() {
    let html = "<script>x</script><script type=\"application/ld+json\">{}</script><style>y</style>";

    let mut dropping = Emitter::new(&ParseLimits::default(), true);
    let out: Vec<Token> = tokens(html, &mut dropping).collect();
    assert_eq!(texts(&out), vec!["{}"]);
    assert!(dropping.dropped_text());

    let mut keeping = Emitter::new(&ParseLimits::default(), false);
    let out: Vec<Token> = tokens(html, &mut keeping).collect();
    assert_eq!(texts(&out), vec!["x", "{}", "y"]);
    assert!(!keeping.dropped_text());

    let mut quoted = Emitter::new(&ParseLimits::default(), true);
    let out: Vec<Token> = tokens(r#"<script src="x"/>Hello"#, &mut quoted).collect();
    assert_eq!(texts(&out), vec!["Hello"]);
    assert!(!quoted.dropped_text());
}

#[test]
fn comments_doctype_pi_skipped() {
    assert_eq!(
        toks("<!doctype html><!--c--><?xml x?>a"),
        vec![Token::Skipped, Token::Skipped, Token::Skipped, text("a")]
    );
}

#[test]
fn leading_bom_stripped_once() {
    assert_eq!(toks("\u{feff}\u{feff}a"), vec![text("\u{feff}a")]);
}
