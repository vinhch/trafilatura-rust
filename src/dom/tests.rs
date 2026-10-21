use std::time::{Duration, Instant};

use super::*;
use crate::cancel::{Call, CancelToken};
use crate::error::{ExtractError, Rejection};

fn parse(html: &str) -> Tree {
    parse_document(html, &ParseLimits::default(), &Call::default()).unwrap()
}

fn find(tree: &Tree, name: &str) -> NodeId {
    let mut stack = vec![tree.root()];
    while let Some(id) = stack.pop() {
        if &*tree.node(id).name == name {
            return id;
        }
        stack.extend(tree.children(id));
    }
    panic!("no element named {name}");
}

fn max_depth(tree: &Tree) -> usize {
    let mut deepest = 0;
    let mut stack = vec![(tree.root(), 1)];
    while let Some((id, depth)) = stack.pop() {
        deepest = deepest.max(depth);
        stack.extend(tree.children(id).map(|child| (child, depth + 1)));
    }
    deepest
}

#[test]
fn dump_matches_python_format() {
    // Input of tools/parity test_dump_tree_format; the comment is not a node and html5ever adds
    // an empty head, so the expected text below has that one extra line.
    let html = r#"<html><body><div b="2" a="1">x<!--c--><p> </p>tail</div><svg><path d="M0"/></svg></body></html>"#;
    let python = concat!(
        "html\n",
        "  body\n",
        "    div a=\"1\" b=\"2\"\n",
        "      #text \"x\"\n",
        "      p\n",
        "        #text \" \"\n",
        "      #text \"tail\"\n",
        "    svg@svg\n",
        "      path@svg d=\"M0\"\n",
    );
    let expected = python.replacen("html\n", "html\n  head\n", 1);
    assert_eq!(dump(&parse(html), DumpMode::Exact), (expected, false));

    // The same tree built by hand, without the implied head, gives the Python string verbatim.
    let name = |n: &str| LocalName::from(n);
    let attr = |k: &str, v: &str| (name(k), v.to_string());
    let mut tree = Tree::new(name("html"), Ns::Html, vec![]);
    let body = tree.add_child(tree.root(), name("body"), Ns::Html, vec![]);
    let div = tree.add_child(
        body,
        name("div"),
        Ns::Html,
        vec![attr("b", "2"), attr("a", "1")],
    );
    tree.add_text(div, "x");
    let p = tree.add_child(div, name("p"), Ns::Html, vec![]);
    tree.add_text(p, " ");
    tree.add_text(div, "tail");
    let svg = tree.add_child(body, name("svg"), Ns::Svg, vec![]);
    tree.add_child(svg, name("path"), Ns::Svg, vec![attr("d", "M0")]);
    assert_eq!(dump(&tree, DumpMode::Exact), (python.to_string(), false));
}

#[test]
fn text_and_tail_follow_lxml() {
    let tree = parse("<p>a<b>b</b>c</p>");
    let p = tree.node(find(&tree, "p"));
    let b = tree.node(find(&tree, "b"));
    assert_eq!(p.text.as_deref(), Some("a"));
    assert_eq!(b.text.as_deref(), Some("b"));
    assert_eq!(b.tail.as_deref(), Some("c"));
}

#[test]
fn implied_tbody_and_head() {
    let tree = parse("<table><tr><td>x</td></tr></table>");
    let (text, _) = dump(&tree, DumpMode::Exact);
    assert!(text.contains("\n  head\n"), "{text}");
    assert!(
        text.contains("    table\n      tbody\n        tr\n"),
        "{text}"
    );
}

#[test]
fn depth_limit_cuts_and_drops_rest() {
    let html = format!("{}<p>after</p>", "<div>".repeat(300));
    let tree = parse(&html);
    assert_eq!(max_depth(&tree), 256);
    let (text, _) = dump(&tree, DumpMode::Exact);
    assert!(!text.contains("after"), "{text}");
}

#[test]
fn max_nodes_rejects() {
    let html = "<span></span>".repeat(10_000);
    let limits = ParseLimits {
        max_nodes: Some(1000),
        ..ParseLimits::default()
    };
    let err = parse_document(&html, &limits, &Call::default()).unwrap_err();
    assert_eq!(err, ExtractError::Rejected(Rejection::TreeTooLarge));
    let tree = parse_document(&html, &ParseLimits::default(), &Call::default()).unwrap();
    assert!(tree.len() > 10_000);
}

#[test]
fn cancelled_before_parse() {
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    let err = parse_document("<p>x</p>", &ParseLimits::default(), &call).unwrap_err();
    assert_eq!(err, ExtractError::Cancelled);

    let past = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
    let call = Call::default().deadline(past);
    let err = parse_fragment("<p>x</p>", &ParseLimits::default(), &call).unwrap_err();
    assert_eq!(err, ExtractError::DeadlineExceeded);

    // Cancel wins when both apply.
    let call = Call::default().cancel(&token).deadline(past);
    assert_eq!(call.check(), Err(ExtractError::Cancelled));
}

#[test]
fn fragment_root_is_the_div_context() {
    let tree = parse_fragment("a<b>b</b>c", &ParseLimits::default(), &Call::default()).unwrap();
    let root = tree.node(tree.root());
    assert_eq!(&*root.name, "div");
    assert_eq!(root.text.as_deref(), Some("a"));
    let b = tree.node(tree.children(tree.root()).next().unwrap());
    assert_eq!(b.tail.as_deref(), Some("c"));
}

#[test]
fn dump_normalized_drops_invalid_names() {
    let tree = parse("<div @click=\"x\" :href=\"y\" id=\"z\">a\u{1}b</div>");
    let (exact, flag) = dump(&tree, DumpMode::Exact);
    assert!(!flag);
    assert!(
        exact.contains("div :href=\"y\" @click=\"x\" id=\"z\"\n"),
        "{exact}"
    );
    assert!(exact.contains("#text \"a\\u0001b\""), "{exact}");

    let (normalized, flag) = dump(&tree, DumpMode::Normalized);
    assert!(flag);
    assert!(normalized.contains("div id=\"z\"\n"), "{normalized}");
    assert!(normalized.contains("#text \"ab\""), "{normalized}");
}

#[test]
fn dump_normalized_unchanged_tree_is_not_flagged() {
    let tree = parse("<p class=\"a\">x<b>y</b>z</p>");
    let (exact, _) = dump(&tree, DumpMode::Exact);
    assert_eq!(dump(&tree, DumpMode::Normalized), (exact, false));
}

#[test]
fn tree_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Tree>();
    assert_send_sync::<CancelToken>();
    assert_send_sync::<ExtractError>();
}
