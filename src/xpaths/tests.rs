use std::borrow::Cow;
use std::sync::LazyLock;

use regex::Regex;

use super::select::*;
use super::*;
use crate::dom::{ParseLimits, parse_document};
use crate::{Call, CancelToken, ExtractError};

fn parse(html: &str) -> Tree {
    parse_document(html, &ParseLimits::default(), &Call::default()).unwrap()
}

fn by_id(tree: &Tree, id: &str) -> NodeId {
    tree.iter(tree.root(), &[])
        .find(|&node| attr(tree, node, "id") == id)
        .unwrap()
}

fn texts(tree: &Tree, nodes: &[NodeId]) -> Vec<String> {
    nodes.iter().map(|&node| tree.text_content(node)).collect()
}

const POSTS: &str = r#"<div id="c"><div class="post">1</div><div class="post">2</div><section id="s"><div class="x"></div><div class="post">3</div></section></div>"#;

fn is_post(tree: &Tree, id: NodeId) -> bool {
    attr(tree, id, "class") == "post"
}

#[test]
fn alt_joins_and_rejects_empty() {
    assert_eq!(alt(&["a", "b"]), Some("a|b".into()));
    assert_eq!(alt(&[]), None);
}

#[test]
fn attr_missing_is_empty() {
    let tree = parse(r#"<div id="a"></div>"#);
    let div = by_id(&tree, "a");
    assert_eq!(attr(&tree, div, "id"), "a");
    assert_eq!(attr(&tree, div, "class"), "");
    assert!(has_attr(&tree, div, "id"));
    assert!(!has_attr(&tree, div, "class"));
}

#[test]
fn first_attr_takes_source_order() {
    let first = |html: &str, names: &[&str]| {
        let tree = parse(html);
        let div = tree.find(tree.root(), "div").unwrap();
        first_attr(&tree, div, names).to_string()
    };
    let names = ["id", "style"];
    assert_eq!(first(r#"<div class="b" id="hidden">"#, &names), "hidden");
    assert_eq!(first(r#"<div id="a" class="hidden">"#, &names), "a");
    assert_eq!(first(r#"<div class="x" id="y">"#, &["id", "class"]), "x");
    assert_eq!(first("<div>", &["id", "class"]), "");
}

#[test]
fn translate_maps_listed_chars_only() {
    assert_eq!(translate("Main-Content", "CM", "cm"), "main-content");
    assert_eq!(translate("MAIN-CONTENT", "CM", "cm"), "mAIN-cONTENT");
    assert!(matches!(translate("abc", "CM", "cm"), Cow::Borrowed(_)));
}

#[test]
fn is_html_rejects_svg() {
    let tree = parse("<svg></svg><p></p>");
    let svg = tree
        .iter(tree.root(), &[])
        .find(|&n| &*tree.node(n).name == "svg")
        .unwrap();
    assert!(!is_html(&tree, svg, &["svg"]));
    let p = tree.find(tree.root(), "p").unwrap();
    assert!(is_html(&tree, p, &["p", "div"]));
    assert!(!is_html(&tree, p, &["div"]));
}

#[test]
fn re_test_is_false_for_a_pattern_that_did_not_compile() {
    static GOOD: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new("^a").ok());
    static BAD: LazyLock<Option<Regex>> = LazyLock::new(|| None);
    assert!(re_test("ab", &GOOD));
    assert!(!re_test("ba", &GOOD));
    assert!(!re_test("ab", &BAD));
}

#[test]
fn descendants_exclude_context_in_order() {
    let tree = parse(POSTS);
    let c = by_id(&tree, "c");
    let found = descendants(&tree, c, &Call::default(), |_, _| true).unwrap();
    assert_eq!(found.len(), 5);
    assert!(!found.contains(&c));
    let expected: Vec<NodeId> = tree.iterdescendants(c, &[]).collect();
    assert_eq!(found, expected);
    let posts = descendants(&tree, c, &Call::default(), is_post).unwrap();
    assert_eq!(texts(&tree, &posts), ["1", "2", "3"]);
}

#[test]
fn descendants_stop_when_cancelled() {
    let tree = parse(POSTS);
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    let found = descendants(&tree, tree.root(), &call, |_, _| true);
    assert_eq!(found, Err(ExtractError::Cancelled));
}

#[test]
fn first_among_siblings_is_per_parent() {
    let tree = parse(POSTS);
    let call = Call::default();
    // oracle: BODY_XPATH[0] positions [4, 8]; from section [8]
    let from_root = first_among_siblings(&tree, tree.root(), &call, is_post).unwrap();
    assert_eq!(texts(&tree, &from_root), ["1", "3"]);
    let from_section = first_among_siblings(&tree, by_id(&tree, "s"), &call, is_post).unwrap();
    assert_eq!(texts(&tree, &from_section), ["3"]);
}

#[test]
fn first_descendant_is_first_in_document() {
    let tree = parse(POSTS);
    let call = Call::default();
    let first = first_descendant(&tree, tree.root(), &call, is_post)
        .unwrap()
        .unwrap();
    assert_eq!(tree.text_content(first), "1");
    assert_eq!(
        first_descendant(&tree, tree.root(), &call, |_, _| false),
        Ok(None)
    );
}

#[test]
fn union_follows_tree_order_after_move() {
    let mut tree = parse(r#"<div id="d"><p id="a"></p><p id="b"></p></div>"#);
    let (d, a, b) = (by_id(&tree, "d"), by_id(&tree, "a"), by_id(&tree, "b"));
    tree.add_previous(a, b);
    let call = Call::default();
    assert_eq!(union(&tree, d, &call, &[a, b]).unwrap(), [b, a]);
    assert_eq!(union(&tree, d, &call, &[a, a]).unwrap(), [a]);
}
