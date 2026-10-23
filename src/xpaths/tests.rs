use std::borrow::Cow;
use std::sync::LazyLock;

use regex::Regex;

use super::cases::{CASES, SELECTOR_NAMES};
use super::select::*;
use super::*;
use crate::dom::{DumpMode, ParseLimits, dump, parse_document};
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

fn snippet(html: &str) -> String {
    html.chars().take(120).collect()
}

#[test]
fn case_trees_match_html5lib() {
    for case in CASES {
        let tree = parse(case.html);
        let (dumped, changed) = dump(&tree, DumpMode::Normalized);
        assert!(
            !changed,
            "the copy rule changed the tree of {}",
            snippet(case.html)
        );
        assert_eq!(
            dumped,
            case.dump,
            "tree differs from html5lib for {}",
            snippet(case.html)
        );
    }
}

#[test]
fn case_contexts_exist() {
    for case in CASES {
        let tree = parse(case.html);
        let count = tree.iter(tree.root(), &[]).count();
        assert!(
            case.context < count,
            "context out of range for {}",
            snippet(case.html)
        );
        for (name, positions) in case.hits {
            assert!(
                positions.iter().all(|&position| position < count),
                "{name} position out of range for {}",
                snippet(case.html)
            );
        }
    }
}

#[test]
fn comments_lookahead_matches_python() {
    // oracle: re.search at the pin, `(?!ary)` form
    let id = Regex::new(&format!("^(?:{COMMENTS_ID})")).unwrap();
    let class = Regex::new(COMMENTS_CLASS).unwrap();
    let table = [
        ("comment", true, true),
        ("commentary", false, false),
        ("commentar", true, true),
        ("commenta", true, true),
        ("comment\n", true, true),
        ("commenta\n", true, true),
        ("commentar\n", true, true),
        ("commentary\n", false, false),
        ("Comments", true, true),
        ("comol", true, false),
        ("xcomment", false, false),
    ];
    for (input, id_hit, class_hit) in table {
        assert_eq!(id.is_match(input), id_hit, "id pattern on {input:?}");
        assert_eq!(
            class.is_match(input),
            class_hit,
            "class pattern on {input:?}"
        );
    }
}

/// Every selector ported so far, named as in `SELECTOR_NAMES`.
fn ported() -> Vec<(String, Selector)> {
    let lists: [(&str, &[Selector]); 5] = [
        ("BODY_XPATH", BODY_XPATH),
        ("COMMENTS_XPATH", COMMENTS_XPATH),
        ("REMOVE_COMMENTS_XPATH", REMOVE_COMMENTS_XPATH),
        (
            "REMOVE_COMMENTS_AND_LISTS_XPATH",
            REMOVE_COMMENTS_AND_LISTS_XPATH,
        ),
        ("RAW_TREE_PRUNE_XPATH", RAW_TREE_PRUNE_XPATH),
    ];
    lists
        .iter()
        .flat_map(|(name, list)| {
            list.iter()
                .enumerate()
                .map(move |(i, &selector)| (format!("{name}[{i}]"), selector))
        })
        .collect()
}

#[test]
fn ported_selectors_are_named_in_the_oracle() {
    for (name, _) in ported() {
        assert!(SELECTOR_NAMES.contains(&name.as_str()), "unknown {name}");
    }
}

#[test]
fn cases_match_upstream() {
    let selectors = ported();
    for case in CASES {
        let tree = parse(case.html);
        let order: Vec<NodeId> = tree.iter(tree.root(), &[]).collect();
        let context = order[case.context];
        for (name, selector) in &selectors {
            let expected: &[usize] = case
                .hits
                .iter()
                .find(|(hit, _)| hit == name)
                .map_or(&[], |(_, positions)| positions);
            let found = selector(&tree, context, &Call::default()).unwrap();
            let actual: Vec<usize> = found
                .iter()
                .map(|id| order.iter().position(|node| node == id).unwrap())
                .collect();
            assert_eq!(
                actual,
                expected,
                "{name} on {}: expected {expected:?}, got {actual:?}",
                snippet(case.html)
            );
        }
    }
}

#[test]
fn patterns_compile() {
    for pattern in [
        &ARTICLE_CONTENT_ID,
        &ARTICLE_CONTENT_CLASS,
        &STORY_ID,
        &STORY_CLASS,
        &MAIN_CONTENT_ID,
        &MAIN_CONTENT_CLASS,
        &FULLTEXT,
        &COMMENT_LIST,
        &COMMENT_PAGE,
        &COMMENT_PREFIX,
        &COMMENTS_PREFIX,
        &COMMENT_SERVICE_ID,
        &REMOVE_COMMENTS_ID,
        &REMOVE_COMMENTS_LIST_ID,
        &REMOVE_COMMENTS_CLASS,
        &MVP_POST_ADD_OR_SCROLL,
        &MVP_POST_ADD,
    ] {
        assert!(pattern.is_some());
    }
}
