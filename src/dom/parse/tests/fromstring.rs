//! `fromstring` and `fragment_fromstring` against the trees lxml builds from the snippets of `tree_cases.rs`,
//! and the root they leave for the selectors and the serializer.

use super::super::ParseLimits;
use super::super::fromstring::{fragment_fromstring_div, fromstring};
use super::super::tree_cases::{TREE_CASES, TreeCase};
use crate::cancel::Call;
use crate::dom::{NodeId, Tree, dump};
use crate::error::{ExtractError, Rejection};
use crate::xpaths::TITLE_XPATHS;

/// `html` as `fromstring` parses it, with the default limits.
fn from_string(html: &str) -> Tree {
    fromstring(html, &ParseLimits::default(), &Call::default()).unwrap()
}

/// What differs between the treedump `expected` and the tree `result` holds, `None` when they match.
/// `!empty` and `!error` expect `EmptyTree`.
fn compare(expected: &str, result: Result<Tree, ExtractError>) -> Option<String> {
    if expected == "!empty" || expected == "!error" {
        return match result {
            Err(ExtractError::Rejected(Rejection::EmptyTree)) => None,
            Ok(tree) => Some(format!("expected: lxml raises\nactual:\n{}", dump(&tree))),
            Err(error) => Some(format!("expected: EmptyTree\nactual error: {error:?}")),
        };
    }
    match result {
        Ok(tree) => {
            let actual = dump(&tree);
            (actual != expected).then(|| format!("expected:\n{expected}actual:\n{actual}"))
        }
        Err(error) => Some(format!("expected a tree\nactual error: {error:?}")),
    }
}

/// The failures of one check over every case, one block per case.
fn failures(cases: &[TreeCase], mismatch: impl Fn(&TreeCase) -> Option<String>) -> Vec<String> {
    cases
        .iter()
        .filter_map(|case| {
            mismatch(case).map(|diff| format!("{} ({:?}):\n{diff}", case.name, case.html))
        })
        .collect()
}

#[test]
fn fromstring_trees_match_lxml() {
    let failures = failures(TREE_CASES, |case| {
        let result = fromstring(case.html, &ParseLimits::default(), &Call::default());
        compare(case.fromstring, result)
    });
    assert!(
        failures.is_empty(),
        "{} of {} cases differ from lxml:\n{}",
        failures.len(),
        TREE_CASES.len(),
        failures.join("\n")
    );
}

#[test]
fn fragment_trees_match_lxml() {
    let failures = failures(TREE_CASES, |case| {
        let result = fragment_fromstring_div(case.html, &ParseLimits::default(), &Call::default());
        compare(case.fragment, result)
    });
    assert!(
        failures.is_empty(),
        "{} of {} cases differ from lxml:\n{}",
        failures.len(),
        TREE_CASES.len(),
        failures.join("\n")
    );
}

#[test]
fn div_root_keeps_html_parent() {
    let tree = from_string("<p>a</p><div>b</div>");
    let root = tree.root();
    assert_eq!(&*tree.node(root).name, "div");
    let parent = tree.node(root).parent().unwrap();
    assert_eq!(&*tree.node(parent).name, "html");
}

#[test]
fn absolute_selector_sees_above_the_root() {
    let tree = from_string(r#"<body class="entry-title"><div><p>a</p><p>b</p></div></body>"#);
    let root = tree.root();
    assert_eq!(&*tree.node(root).name, "div");
    let body = tree.node(root).parent().unwrap();
    // TITLE_XPATHS[1] matches the class `entry-title` or `post-title`
    let found = TITLE_XPATHS[1](&tree, root, &Call::default()).unwrap();
    assert_eq!(found, vec![body]);
}

#[test]
fn absolute_selector_sees_after_html() {
    let tree = from_string(
        r#"<html><head></head><body><p>x</p></body></html><h1 class="post-title">Big Title</h1>"#,
    );
    let found = TITLE_XPATHS[1](&tree, tree.root(), &Call::default()).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(&*tree.node(found[0]).name, "h1");
    assert_eq!(tree.text_content(found[0]), "Big Title");
}

#[test]
fn fragment_without_body_is_empty_tree() {
    let result = fragment_fromstring_div(
        "<html><head><title>t</title></head></html>",
        &ParseLimits::default(),
        &Call::default(),
    );
    assert_eq!(
        result.err(),
        Some(ExtractError::Rejected(Rejection::EmptyTree))
    );
}

#[test]
fn valueless_attribute_serializes_bare() {
    // the lone div is the root: fromstring returns the body's only element
    let tree = from_string(r#"<div hidden data-x="" defer>t</div>"#);
    let div = tree.root();
    assert_eq!(&*tree.node(div).name, "div");
    // lxml 2.14.6: lxml.html.tostring and etree.tostring of the same element
    assert_eq!(
        tree.tostring_html(div, false),
        r#"<div hidden data-x="" defer>t</div>"#
    );
    assert_eq!(
        tree.tostring(div, false, false),
        r#"<div hidden="" data-x="" defer="defer">t</div>"#
    );
}

#[test]
fn valueless_attribute_is_the_shared_sentinel() {
    let tree = from_string(r#"<div hidden data-x="" defer>t</div>"#);
    let div: NodeId = tree.root();
    assert_eq!(tree.get(div, "hidden"), Some(""));
    assert!(tree.is_valueless(div, "hidden"));
    assert!(!tree.is_valueless(div, "data-x"));
    assert!(!tree.is_valueless(div, "defer"));
}
