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

/// `.//div[@class='widget']`
fn widget_divs(tree: &Tree, context: NodeId, call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    descendants(tree, context, call, |tree, id| {
        tree.tag(id) == Some("div") && tree.get(id, "class") == Some("widget")
    })
}

/// Whether `prune_unwanted_nodes(body, [widget_divs], with_backup=True)` removes the widgets of
/// `<body><p>{kept}</p>{widgets}</body>`.
fn backup_prunes(kept: usize, widgets: &str) -> bool {
    let html = format!(
        "<html><body><p>{}</p>{widgets}</body></html>",
        "k".repeat(kept)
    );
    let call = Call::default();
    let mut tree = parse_document(&html, &ParseLimits::default(), &call).unwrap();
    let root = tree.root();
    let body = tree.find(root, "body").unwrap();
    let before = tree.text_content(body);
    prune_unwanted_nodes(&mut tree, body, &[widget_divs], true, &call).unwrap();
    tree.text_content(body) != before
}

#[test]
fn backup_guard_boundary() {
    // pruned only when `old_len - removed > old_len / 7`, a float division: old_len 70 gives 10.0
    // (kept == 10 is not enough), old_len 71 gives 10.14 (kept == 10 is not enough either)
    // oracle: prune_unwanted_nodes(body, [XPath(".//div[@class='widget']")], with_backup=True) over
    // parse_document("<html><body><p>" + "k" * kept + "</p><div class='widget'>" + "w" * widget
    // + "</div></body></html>"), true when body.text_content() changed
    for (kept, widget, pruned) in [
        (10, 60, false), // old_len 70: exactly old_len - old_len / 7 removed
        (11, 59, true),  // one character less removed
        (9, 61, false),  // one character more removed
        (10, 61, false), // old_len 71
        (11, 60, true),
        (12, 59, true),
    ] {
        let widgets = format!("<div class='widget'>{}</div>", "w".repeat(widget));
        assert_eq!(
            backup_prunes(kept, &widgets),
            pruned,
            "kept {kept}, widget {widget}"
        );
    }
}

#[test]
fn backup_counts_nested_match_once() {
    // A widget inside a widget is deleted with its outer match, so its text counts once: 59 removed
    // of 70 leaves 11 > 10.0 and prunes; counting the inner text twice would remove 89 and keep.
    // oracle: same expression as backup_guard_boundary with the widget
    // "<div class='widget'>" + "o" * outer + "<div class='widget'>" + "n" * inner + "</div></div>"
    for (kept, outer, inner, pruned) in [(11, 30, 29, true), (11, 0, 59, true), (10, 30, 30, false)]
    {
        let widgets = format!(
            "<div class='widget'>{}<div class='widget'>{}</div></div>",
            "o".repeat(outer),
            "n".repeat(inner)
        );
        assert_eq!(
            backup_prunes(kept, &widgets),
            pruned,
            "kept {kept}, outer {outer}, inner {inner}"
        );
    }
    // the outer match is the only deletion: nothing of the nested one is left to find
    let call = Call::default();
    let html = "<html><body><p>keep keep</p><div class='widget'>a<div class='widget'>b</div></div></body></html>";
    let mut tree = parse_document(html, &ParseLimits::default(), &call).unwrap();
    let root = tree.root();
    let body = tree.find(root, "body").unwrap();
    prune_unwanted_nodes(&mut tree, body, &[widget_divs], true, &call).unwrap();
    assert_eq!(tree.text_content(body), "keep keep");
    assert!(tree.find(body, "div").is_none());
}

/// Runs `handle_textnode` on the `tag` child of `<p>` given `text` and `tail`; returns whether it
/// is kept, then the child's tag, text and tail afterwards.
fn textnode_case(
    tag: &str,
    attrs_list: &[(&str, &str)],
    text: Option<&str>,
    tail: Option<&str>,
    comments_fix: bool,
    preserve_spaces: bool,
) -> (bool, String, Option<String>, Option<String>) {
    let name = |n: &str| LocalName::from(n);
    let mut tree = Tree::new(name("p"), Ns::Html, vec![]);
    let child = tree.add_child(tree.root(), name(tag), Ns::Html, attrs(attrs_list));
    tree.node_mut(child).text = text.map(str::to_string);
    tree.node_mut(child).tail = tail.map(str::to_string);
    let options = options();
    let call = Call::default();
    let ctx = Ctx {
        options: Cow::Borrowed(&options),
        call: &call,
    };
    let kept = handle_textnode(&mut tree, child, &ctx, comments_fix, preserve_spaces).is_some();
    let node = tree.node(child);
    (
        kept,
        node.name.to_string(),
        node.text.clone(),
        node.tail.clone(),
    )
}

#[test]
fn textnode_empty_tail_stays_empty_string() {
    // oracle: run('<p><x/> </p>'): handle_textnode(n, Extractor()) is None, n.text None, n.tail ''
    let (kept, _, text, tail) = textnode_case("x", &[], None, Some(" "), true, false);
    assert!(!kept);
    assert_eq!(text, None);
    assert_eq!(tail.as_deref(), Some(""));
    // oracle: run('<p><x/> a </p>'): kept, n.text 'a', n.tail ''
    let (kept, _, text, tail) = textnode_case("x", &[], None, Some(" a "), true, false);
    assert!(kept);
    assert_eq!(text.as_deref(), Some("a"));
    assert_eq!(tail.as_deref(), Some(""));
}

#[test]
fn textnode_graphic_with_src_is_kept() {
    // oracle: run('<p><graphic src="a.jpg"/></p>'): kept, n.text None, n.tail None
    let (kept, _, text, tail) =
        textnode_case("graphic", &[("src", "a.jpg")], None, None, true, false);
    assert!(kept);
    assert_eq!((text, tail), (None, None));
    // oracle: run('<p><graphic/>t</p>'): kept, n.text 't', n.tail ''
    let (kept, _, text, tail) = textnode_case("graphic", &[], None, Some("t"), true, false);
    assert!(kept);
    assert_eq!(text.as_deref(), Some("t"));
    assert_eq!(tail.as_deref(), Some(""));
}

#[test]
fn textnode_done_is_dropped() {
    // oracle: run('<p><done>x</done></p>'): None, n.text 'x', n.tail None, untouched
    let (kept, _, text, tail) = textnode_case("done", &[], Some("x"), None, true, false);
    assert!(!kept);
    assert_eq!(text.as_deref(), Some("x"));
    assert_eq!(tail, None);
}

#[test]
fn textnode_lb_bypass() {
    // oracle: run('<p><lb/> a   b </p>', comments_fix=False): kept, n.text None, n.tail 'a b'
    let (kept, tag, text, tail) = textnode_case("lb", &[], None, Some(" a   b "), false, false);
    assert!(kept);
    assert_eq!(tag, "lb");
    assert_eq!(text, None);
    assert_eq!(tail.as_deref(), Some("a b"));
    // oracle: run('<p><lb/> a   b </p>', comments_fix=False, preserve_spaces=True): n.tail ' a   b '
    let (kept, _, text, tail) = textnode_case("lb", &[], None, Some(" a   b "), false, true);
    assert!(kept);
    assert_eq!(text, None);
    assert_eq!(tail.as_deref(), Some(" a   b "));
    // oracle: run('<p><lb/>   </p>', comments_fix=False): kept, n.tail None
    let (kept, _, _, tail) = textnode_case("lb", &[], None, Some("   "), false, false);
    assert!(kept);
    assert_eq!(tail, None);
}

#[test]
fn textnode_lb_becomes_p_with_comments_fix() {
    // oracle: run('<p><lb/>tail</p>', comments_fix=True): kept, n.tag 'p', n.text 'tail', n.tail ''
    let (kept, tag, text, tail) = textnode_case("lb", &[], None, Some("tail"), true, false);
    assert!(kept);
    assert_eq!(tag, "p");
    assert_eq!(text.as_deref(), Some("tail"));
    assert_eq!(tail.as_deref(), Some(""));
    // oracle: run('<p><lb/>tail</p>', comments_fix=True, preserve_spaces=True): same
    let (kept, tag, text, tail) = textnode_case("lb", &[], None, Some("tail"), true, true);
    assert!(kept);
    assert_eq!(tag, "p");
    assert_eq!(text.as_deref(), Some("tail"));
    assert_eq!(tail.as_deref(), Some(""));
}

/// Runs `handle_textnode` (`light` false) or `process_node` (`light` true) on a child named `tag`
/// in namespace `ns`; returns whether it is kept, then its text and tail afterwards.
fn foreign_case(
    ns: Ns,
    tag: &str,
    text: Option<&str>,
    tail: Option<&str>,
    light: bool,
) -> (bool, Option<String>, Option<String>) {
    let mut tree = Tree::new(LocalName::from("p"), Ns::Html, vec![]);
    let child = tree.add_child(tree.root(), LocalName::from(tag), ns, vec![]);
    tree.node_mut(child).text = text.map(str::to_string);
    tree.node_mut(child).tail = tail.map(str::to_string);
    let options = options();
    let call = Call::default();
    let ctx = Ctx {
        options: Cow::Borrowed(&options),
        call: &call,
    };
    let kept = if light {
        process_node(&mut tree, child, &ctx)
    } else {
        handle_textnode(&mut tree, child, &ctx, true, false)
    }
    .is_some();
    let node = tree.node(child);
    (kept, node.text.clone(), node.tail.clone())
}

#[test]
fn foreign_namespace_elements_are_kept_as_text() {
    let s = |value: &str| Some(value.to_string());
    // oracle: p = etree.Element('p'); c = etree.SubElement(p, '{http://www.w3.org/1998/Math/MathML}math')
    // with c.text, c.tail = 'x', None / None, ' t ' / 'x', ' t ': handle_textnode(c, Extractor()) is kept,
    // leaving (text, tail) = ('x', None) / ('t', '') / ('x', 't')
    assert_eq!(
        foreign_case(Ns::MathMl, "math", Some("x"), None, false),
        (true, s("x"), None)
    );
    assert_eq!(
        foreign_case(Ns::MathMl, "math", None, Some(" t "), false),
        (true, s("t"), s(""))
    );
    assert_eq!(
        foreign_case(Ns::MathMl, "math", Some("x"), Some(" t "), false),
        (true, s("x"), s("t"))
    );
    // oracle: same elements through process_node(c, Extractor()): kept, ('x', None) / ('t', None) / ('x', 't')
    assert_eq!(
        foreign_case(Ns::MathMl, "math", Some("x"), None, true),
        (true, s("x"), None)
    );
    assert_eq!(
        foreign_case(Ns::MathMl, "math", None, Some(" t "), true),
        (true, s("t"), None)
    );
    assert_eq!(
        foreign_case(Ns::MathMl, "math", Some("x"), Some(" t "), true),
        (true, s("x"), s("t"))
    );
    // oracle: an element tagged '{http://www.w3.org/2000/svg}done' with text 'x' is kept by
    // handle_textnode, because its tag is not 'done'
    assert_eq!(
        foreign_case(Ns::Svg, "done", Some("x"), None, false),
        (true, s("x"), None)
    );
    assert_eq!(
        foreign_case(Ns::Svg, "done", Some("x"), None, true),
        (true, s("x"), None)
    );
}
