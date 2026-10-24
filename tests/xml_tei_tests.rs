#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Ports of upstream `tests/xml_tei_tests.py`, case by case with the upstream names. The rest of
//! that file calls `check_tei` and `write_fullheader`, which Phase 6 ports.

use trafilatura_rust::__internals::{LocalName, NodeId, Ns, Tree, replace_element_text};
use trafilatura_rust::Call;

/// `etree.Element(name)` with its text and attributes, in a tree of its own.
fn element(name: &str, text: &str, attributes: &[(&str, &str)]) -> (Tree, NodeId) {
    let mut tree = Tree::new(LocalName::from(name), Ns::Html, Vec::new());
    let root = tree.root();
    tree.node_mut(root).text = Some(text.to_owned());
    for (key, value) in attributes {
        tree.set(root, key, value);
    }
    (tree, root)
}

#[test]
fn test_replace_element_text() {
    let call = Call::default();
    let render = |name, text, attributes: &[(&str, &str)]| {
        let (mut tree, root) = element(name, text, attributes);
        replace_element_text(&mut tree, root, true, false, &call).unwrap()
    };
    assert_eq!(render("head", "Title", &[("rend", "h1")]), "# Title");
    assert_eq!(render("hi", "Text", &[("rend", "#b")]), "**Text**");
    // the list marker is added by the serializer
    assert_eq!(render("item", "Test text", &[]), "Test text");
    assert_eq!(
        render("ref", "Link", &[("target", "https://example.com")]),
        "[Link](https://example.com)"
    );
    assert_eq!(render("ref", "Link", &[]), "[Link]");
}
