use std::borrow::Cow;
use std::sync::Arc;

use super::*;
use crate::dom::{DumpMode, LocalName, Ns, ParseLimits, dump, parse_document};
use crate::settings::Extractor;
use crate::settings::{Config, Format};

fn options() -> Extractor {
    Extractor::new(
        Config::default(),
        Format::Python,
        false,
        false,
        None,
        false,
        false,
        Default::default(),
        Default::default(),
    )
}

fn attrs(pairs: &[(&str, &str)]) -> Vec<(LocalName, Arc<str>)> {
    pairs
        .iter()
        .map(|(key, value)| (LocalName::from(*key), Arc::from(*value)))
        .collect()
}

/// `<html><body><p>a <math ...>...</math>{tail}</p></body></html>` with every element in the HTML
/// namespace, the shape libxml2 gives upstream; returns the tree and the `p`.
fn math_page(math_attrs: &[(&str, &str)], tex: Option<&str>, tail: &str) -> (Tree, NodeId) {
    let name = |n: &str| LocalName::from(n);
    let mut tree = Tree::new(name("html"), Ns::Html, vec![]);
    let body = tree.add_child(tree.root(), name("body"), Ns::Html, vec![]);
    let p = tree.add_child(body, name("p"), Ns::Html, vec![]);
    tree.add_text(p, "a ");
    let math = tree.add_child(p, name("math"), Ns::Html, attrs(math_attrs));
    match tex {
        Some(tex) => {
            let semantics = tree.add_child(math, name("semantics"), Ns::Html, vec![]);
            let annotation = tree.add_child(
                semantics,
                name("annotation"),
                Ns::Html,
                attrs(&[("encoding", "application/x-tex")]),
            );
            tree.add_text(annotation, tex);
        }
        None => {
            let mi = tree.add_child(math, name("mi"), Ns::Html, vec![]);
            tree.add_text(mi, "q");
        }
    }
    tree.add_text(p, tail);
    (tree, p)
}

/// The text of the paragraph of `math_page` after `tree_cleaning`, and whether a `math` is left.
fn cleaned_paragraph(math_attrs: &[(&str, &str)], tex: Option<&str>, tail: &str) -> (String, bool) {
    let (mut tree, p) = math_page(math_attrs, tex, tail);
    let options = options();
    let call = Call::default();
    let ctx = Ctx {
        options: Cow::Borrowed(&options),
        call: &call,
    };
    let root = tree.root();
    tree_cleaning(&mut tree, root, &ctx).unwrap();
    (tree.text_content(p), tree.find(root, "math").is_some())
}

#[test]
fn recover_math_on_html_namespace_math() {
    // the annotation holds the original LaTeX and takes precedence over alttext
    let (text, has_math) = cleaned_paragraph(&[("alttext", "rendered")], Some("x^2"), " b");
    assert_eq!(text, "a \\(x^2\\) b");
    assert!(!has_math);
    // alttext is used when there is no LaTeX annotation
    let (text, _) = cleaned_paragraph(&[("alttext", "y_1")], None, " b");
    assert_eq!(text, "a \\(y_1\\) b");
    // display="block" yields block delimiters
    let (text, _) = cleaned_paragraph(&[("display", "block"), ("alttext", "z")], None, "");
    assert_eq!(text, "a \\[z\\]");
    // nothing to recover: the element is dropped
    let (text, has_math) = cleaned_paragraph(&[], None, " b");
    assert_eq!(text, "a  b");
    assert!(!has_math);
    // the tail of the formula survives alongside the recovered source
    let (text, _) = cleaned_paragraph(&[("alttext", "u")], None, " tail");
    assert_eq!(text, "a \\(u\\) tail");
}

#[test]
fn cleaning_loop_skips_like_lxml() {
    let mut tree = parse_document(
        "<aside><aside>in</aside></aside><aside>after</aside><p>x</p>",
        &ParseLimits::default(),
        &Call::default(),
    )
    .unwrap();
    let options = options();
    let call = Call::default();
    let ctx = Ctx {
        options: Cow::Borrowed(&options),
        call: &call,
    };
    let root = tree.root();
    tree_cleaning(&mut tree, root, &ctx).unwrap();
    // oracle: dump_tree(tree_cleaning(parse_document(html), Extractor())): deleting the outer aside
    // leaves the walk at the detached inner one, so the second aside is never visited
    let expected = "html\n  body\n    aside\n      #text \"after\"\n    p\n      #text \"x\"\n";
    assert_eq!(dump(&tree, DumpMode::Exact).0, expected);
}
