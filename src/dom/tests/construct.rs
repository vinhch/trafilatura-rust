use super::*;
use crate::error::TreeLimit;

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
    let attr = |k: &str, v: &str| (name(k), Arc::<str>::from(v));
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
fn dump_tags_plain_svg_by_ancestor() {
    let name = |n: &str| LocalName::from(n);
    let mut tree = Tree::new(name("html"), Ns::Html, vec![]);
    let body = tree.add_child(tree.root(), name("body"), Ns::Html, vec![]);
    let svg = tree.add_child(body, name("svg"), Ns::Html, vec![]);
    let path = tree.add_child(svg, name("path"), Ns::Html, vec![]);
    let math = tree.add_child(body, name("math"), Ns::Html, vec![]);
    tree.add_child(math, name("mi"), Ns::Html, vec![]);
    let outer = tree.add_child(body, name("svg"), Ns::Html, vec![]);
    let inner = tree.add_child(outer, name("math"), Ns::Html, vec![]);
    tree.add_child(inner, name("mn"), Ns::Html, vec![]);
    // The outermost name wins, as `foreign = foreign or ...` in treedump.
    let expected = concat!(
        "html\n  body\n",
        "    svg@svg\n      path@svg\n",
        "    math@math\n      mi@math\n",
        "    svg@svg\n      math@svg\n        mn@svg\n",
    );
    assert_eq!(dump(&tree, DumpMode::Exact).0, expected);
    // The dump root starts with no inherited tag; ancestors above it are not looked at.
    assert_eq!(dump_from(&tree, path, DumpMode::Exact).0, "path\n");
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

fn root_with_child() -> (Tree, NodeId, NodeId) {
    let mut tree = Tree::new(LocalName::from("a"), Ns::Html, vec![]);
    let root = tree.root();
    let child = tree.add_child(root, LocalName::from("b"), Ns::Html, vec![]);
    (tree, root, child)
}

#[test]
fn add_text_owned_moves_into_empty_target() {
    let mut tree = Tree::new(LocalName::from("a"), Ns::Html, vec![]);
    let root = tree.root();
    let text = String::from("abc");
    let ptr = text.as_ptr();
    tree.add_text_owned(root, text);
    let stored = tree.node(root).text.as_ref().unwrap();
    assert_eq!(stored, "abc");
    assert_eq!(stored.as_ptr(), ptr);
}

#[test]
fn add_text_owned_appends_to_last_child_tail() {
    let (mut tree, root, child) = root_with_child();
    tree.add_text(root, "a");
    tree.add_text_owned(root, "b".into());
    assert_eq!(tree.node(child).tail.as_deref(), Some("ab"));
    assert_eq!(tree.node(root).text, None);
}

#[test]
fn add_text_owned_ignores_empty_text() {
    let mut tree = Tree::new(LocalName::from("a"), Ns::Html, vec![]);
    let root = tree.root();
    tree.add_text_owned(root, String::new());
    assert_eq!(tree.node(root).text, None);
}

#[test]
fn add_text_owned_drops_spare_capacity() {
    let mut tree = Tree::new(LocalName::from("a"), Ns::Html, vec![]);
    let root = tree.root();
    let mut text = String::with_capacity(64);
    text.push_str("abc");
    tree.add_text_owned(root, text);
    let stored = tree.node(root).text.as_ref().unwrap();
    assert_eq!(stored, "abc");
    assert!(stored.capacity() < 64);
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
    assert_eq!(
        err,
        ExtractError::Rejected(Rejection::TreeTooLarge(TreeLimit::MaxNodes))
    );
    let unlimited = ParseLimits {
        max_nodes: None,
        ..ParseLimits::default()
    };
    let tree = parse_document(&html, &unlimited, &Call::default()).unwrap();
    assert!(tree.len() > 10_000);
}

#[test]
fn default_limit_is_the_default_max_nodes() {
    assert_eq!(ParseLimits::default().max_nodes, Some(DEFAULT_MAX_NODES));
}

#[test]
fn default_limits_reject_formatting_element_amplification() {
    // Each `<p>` re-opens the 200 formatting elements: 10000 paragraphs would make ~2M elements.
    let mut html = String::from("<html><body><p>");
    for i in 0..200 {
        html.push_str(&format!("<b id={i}>"));
    }
    html.push_str(&"<p>x".repeat(10_000));
    let err = parse_document(&html, &ParseLimits::default(), &Call::default()).unwrap_err();
    assert_eq!(
        err,
        ExtractError::Rejected(Rejection::TreeTooLarge(TreeLimit::MaxNodes))
    );
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
fn dump_from_detached_copy() {
    let mut tree = parse("<div id=\"x\">t<p class=\"a\">a<b>b</b>c</p>tail<i>i</i></div>tail2");
    let div = find(&tree, "div");
    let copy = tree.copy(div, &Call::default()).unwrap();
    assert_eq!(tree.node(copy).parent(), None);
    // the copy is the root of its dump and its tail is left out
    let expected = concat!(
        "div id=\"x\"\n",
        "  #text \"t\"\n",
        "  p class=\"a\"\n",
        "    #text \"a\"\n",
        "    b\n",
        "      #text \"b\"\n",
        "    #text \"c\"\n",
        "  #text \"tail\"\n",
        "  i\n",
        "    #text \"i\"\n",
    );
    for mode in [DumpMode::Exact, DumpMode::Normalized] {
        assert_eq!(dump_from(&tree, copy, mode), (expected.to_string(), false));
        assert_eq!(dump_from(&tree, div, mode), (expected.to_string(), false));
    }
    // the root as `id` is `dump`
    assert_eq!(
        dump_from(&tree, tree.root(), DumpMode::Exact),
        dump(&tree, DumpMode::Exact)
    );
    // the copy rule runs on the subtree only: the invalid name outside it is not counted
    let mut tree = parse("<p @a=\"1\">x</p><div id=\"y\">z</div>");
    let div = find(&tree, "div");
    let copy = tree.copy(div, &Call::default()).unwrap();
    assert!(dump(&tree, DumpMode::Normalized).1);
    assert_eq!(
        dump_from(&tree, copy, DumpMode::Normalized),
        (
            "div id=\"y\"
  #text \"z\"
"
            .to_string(),
            false
        )
    );
}

#[test]
fn tree_is_send_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Tree>();
    assert_send_sync::<CancelToken>();
    assert_send_sync::<ExtractError>();
}

#[test]
fn max_nodes_rejects_formatting_element_amplification() {
    // Each `<p>` re-opens the 50 formatting elements, so 2000 paragraphs make ~100k elements.
    let mut html = String::from("<html><body><p>");
    for i in 0..50 {
        html.push_str(&format!("<b id={i}>"));
    }
    html.push_str(&"<p>x".repeat(2000));
    let limits = ParseLimits {
        max_nodes: Some(5_000),
        ..ParseLimits::default()
    };
    let err = parse_document(&html, &limits, &Call::default()).unwrap_err();
    assert_eq!(
        err,
        ExtractError::Rejected(Rejection::TreeTooLarge(TreeLimit::MaxNodes))
    );
}
