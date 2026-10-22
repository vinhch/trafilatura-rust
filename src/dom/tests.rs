use std::time::{Duration, Instant};

use super::*;
use crate::cancel::{Call, CancelToken};
use crate::error::{ExtractError, Rejection};
use crate::settings::DEFAULT_MAX_NODES;

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
    assert_eq!(err, ExtractError::Rejected(Rejection::TreeTooLarge));
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
    assert_eq!(err, ExtractError::Rejected(Rejection::TreeTooLarge));
}

// Tree edits. Expected values come from lxml 6.1.3 / libxml2 2.14.6 through `scripts/oracle.sh`.

fn names(tree: &Tree, id: NodeId) -> Vec<String> {
    tree.children(id)
        .map(|child| tree.node(child).name.to_string())
        .collect()
}

fn attr_list(tree: &Tree, id: NodeId) -> Vec<(String, String)> {
    tree.node(id)
        .attrs
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect()
}

fn pair(key: &str, value: &str) -> (String, String) {
    (key.to_string(), value.to_string())
}

/// `<div><a/><b/></div>` with the ids of `div`, `a`, `b`.
fn div_with_a_b() -> (Tree, NodeId, NodeId, NodeId) {
    let mut tree = Tree::new(LocalName::from("div"), Ns::Html, vec![]);
    let div = tree.root();
    let a = tree.new_element("a");
    let b = tree.new_element("b");
    tree.append(div, a);
    tree.append(div, b);
    (tree, div, a, b)
}

#[test]
fn append_moves_tail_with_node() {
    // oracle: div = html.fromstring("<div><b>x</b>t<c/></div>"); b, c = div; c.append(b)
    //   -> div.text None, [e.tag for e in div] == ["c"], [e.tag for e in c] == ["b"], b.tail == "t"
    let mut tree = Tree::new(LocalName::from("div"), Ns::Html, vec![]);
    let div = tree.root();
    let b = tree.new_element("b");
    let c = tree.new_element("c");
    tree.append(div, b);
    tree.append(div, c);
    tree.node_mut(b).tail = Some("t".into());
    tree.append(c, b);
    assert_eq!(tree.node(div).text, None);
    assert_eq!(tree.children(div).collect::<Vec<_>>(), [c]);
    assert_eq!(tree.children(c).collect::<Vec<_>>(), [b]);
    assert_eq!(tree.node(b).tail.as_deref(), Some("t"));
    assert_eq!(tree.node(b).parent(), Some(c));
    assert_eq!(tree.node(c).next_sibling(), None);
}

#[test]
fn remove_drops_tail() {
    // oracle: div = html.fromstring("<div><b/>t</div>"); b = div[0]; div.remove(b)
    //   -> div.text None, len(div) == 0, b.getparent() None, b.tail == "t"
    let mut tree = Tree::new(LocalName::from("div"), Ns::Html, vec![]);
    let div = tree.root();
    let b = tree.new_element("b");
    tree.append(div, b);
    tree.node_mut(b).tail = Some("t".into());
    tree.remove(b);
    assert_eq!(tree.node(div).text, None);
    assert_eq!(tree.child_count(div), 0);
    assert_eq!(tree.first_child(div), None);
    assert_eq!(tree.last_child(div), None);
    assert_eq!(tree.node(b).parent(), None);
    assert_eq!(tree.node(b).tail.as_deref(), Some("t"));
    // A detached node has nothing to remove from.
    tree.remove(b);
    assert_eq!(tree.node(b).tail.as_deref(), Some("t"));
}

#[test]
fn insert_index_past_end_appends() {
    // oracle: div = html.fromstring("<div><a/><b/></div>"); div.insert(99, x) -> ["a", "b", "x"]
    let (mut tree, div, a, b) = div_with_a_b();
    let x = tree.new_element("x");
    tree.insert(div, 99, x);
    assert_eq!(names(&tree, div), ["a", "b", "x"]);
    let y = tree.new_element("y");
    tree.insert(div, 0, y);
    assert_eq!(names(&tree, div), ["y", "a", "b", "x"]);
    let z = tree.new_element("z");
    tree.insert(div, 2, z);
    assert_eq!(names(&tree, div), ["y", "a", "z", "b", "x"]);
    assert_eq!(tree.child_at(div, 1), Some(a));
    assert_eq!(tree.child_at(div, 3), Some(b));
    assert_eq!(tree.child_at(div, 5), None);
    assert_eq!(tree.child_count(div), 5);
}

#[test]
fn insert_of_a_child_resolves_the_index_before_the_move() {
    // oracle: for i in range(4): div = html.fromstring("<div><a/><b/><c/></div>"); div.insert(i, div[0])
    //   -> [a b c], [a b c], [b a c], [b c a]
    for (index, expected) in [
        (0, ["a", "b", "c"]),
        (1, ["a", "b", "c"]),
        (2, ["b", "a", "c"]),
        (3, ["b", "c", "a"]),
    ] {
        let (mut tree, div, a, _) = div_with_a_b();
        let c = tree.new_element("c");
        tree.append(div, c);
        tree.insert(div, index, a);
        assert_eq!(names(&tree, div), expected, "index {index}");
    }
}

#[test]
fn add_next_and_previous_order() {
    // oracle: div = html.fromstring("<div><a/><b/></div>"); x appended; div[0].addnext(y) ->
    //   ["a", "y", "b", "x"]; div[0].addprevious(z) -> ["z", "a", "y", "b", "x"]
    let (mut tree, div, a, b) = div_with_a_b();
    let x = tree.new_element("x");
    tree.append(div, x);
    let y = tree.new_element("y");
    tree.add_next(a, y);
    assert_eq!(names(&tree, div), ["a", "y", "b", "x"]);
    let z = tree.new_element("z");
    tree.add_previous(a, z);
    assert_eq!(names(&tree, div), ["z", "a", "y", "b", "x"]);
    assert_eq!(tree.node(b).prev_sibling(), Some(y));
    assert_eq!(tree.node(z).prev_sibling(), None);
    assert_eq!(tree.first_child(div), Some(z));
    assert_eq!(tree.last_child(div), Some(x));
    assert_eq!(tree.node(y).parent(), Some(div));
}

#[test]
fn add_next_and_previous_keep_the_anchor_tail_and_carry_the_node_tail() {
    // oracle: div = html.fromstring("<div><a/>T<b/>U</div>"); y.tail = "Y"; div[0].addnext(y)
    //   -> [("a", "T"), ("y", "Y"), ("b", "U")]; addprevious(y) on a: [("y", "Y"), ("a", "T"), ...]
    let (mut tree, div, a, _) = div_with_a_b();
    tree.node_mut(a).tail = Some("T".into());
    let y = tree.new_element("y");
    tree.node_mut(y).tail = Some("Y".into());
    tree.add_next(a, y);
    let tails: Vec<_> = tree
        .children(div)
        .map(|id| tree.node(id).tail.clone())
        .collect();
    assert_eq!(tails, [Some("T".into()), Some("Y".into()), None]);
    tree.add_previous(a, y);
    assert_eq!(names(&tree, div), ["y", "a", "b"]);
    assert_eq!(tree.node(y).tail.as_deref(), Some("Y"));
    assert_eq!(tree.node(a).tail.as_deref(), Some("T"));
}

#[test]
fn add_next_without_parent_or_onto_itself_is_noop() {
    // oracle: div[0].addnext(div[0]) leaves [a, b] unchanged
    let (mut tree, div, a, b) = div_with_a_b();
    tree.add_next(a, a);
    tree.add_previous(b, b);
    assert_eq!(names(&tree, div), ["a", "b"]);
    let loose = tree.new_element("loose");
    let other = tree.new_element("other");
    tree.add_next(loose, other);
    tree.add_previous(loose, other);
    assert_eq!(tree.node(loose).parent(), None);
    assert_eq!(tree.node(other).parent(), None);
    // The root is an ancestor of every node, so it is never moved.
    tree.add_next(a, div);
    assert_eq!(tree.node(div).parent(), None);
    assert_eq!(names(&tree, div), ["a", "b"]);
}

#[test]
fn append_into_own_descendant_is_noop() {
    // oracle: div = html.fromstring("<div><p/></div>"); div[0].append(div) raises
    //   ValueError("cannot append parent to itself")
    let mut tree = Tree::new(LocalName::from("html"), Ns::Html, vec![]);
    let html = tree.root();
    let div = tree.new_element("div");
    let p = tree.new_element("p");
    tree.append(html, div);
    tree.append(div, p);
    tree.append(p, div);
    tree.append(div, div);
    assert_eq!(tree.children(html).collect::<Vec<_>>(), [div]);
    assert_eq!(tree.children(div).collect::<Vec<_>>(), [p]);
    assert_eq!(tree.child_count(p), 0);
    assert_eq!(tree.node(div).parent(), Some(html));
    tree.insert(p, 0, html);
    assert_eq!(tree.node(html).parent(), None);
    tree.add_next(p, div);
    tree.add_previous(p, html);
    assert_eq!(tree.children(div).collect::<Vec<_>>(), [p]);
    assert!(tree.is_ancestor(html, p));
    assert!(tree.is_ancestor(div, p));
    assert!(!tree.is_ancestor(p, div));
    assert!(!tree.is_ancestor(p, p));
}

#[test]
fn clear_drops_text_tail_attrs_children() {
    // oracle: e = html.fromstring("<div id='x' class='y'>t<b/>u</div>"); b = e[0]; e.clear()
    //   -> e.text None, e.tail None, dict(e.attrib) == {}, len(e) == 0, b.getparent() None
    let mut tree = Tree::new(LocalName::from("html"), Ns::Html, vec![]);
    let root = tree.root();
    let div = tree.new_element("div");
    tree.append(root, div);
    tree.set(div, "id", "x");
    tree.set(div, "class", "y");
    tree.node_mut(div).text = Some("t".into());
    tree.node_mut(div).tail = Some("u".into());
    let b = tree.new_element("b");
    tree.append(div, b);
    tree.node_mut(b).tail = Some("bt".into());
    tree.clear(div);
    let node = tree.node(div);
    assert_eq!((&node.text, &node.tail), (&None, &None));
    assert!(node.attrs.is_empty());
    assert_eq!(tree.child_count(div), 0);
    assert_eq!(tree.first_child(div), None);
    assert_eq!(tree.node(b).parent(), None);
    assert_eq!(tree.node(b).prev_sibling(), None);
    assert_eq!(tree.node(b).tail.as_deref(), Some("bt"));
    // The cleared element stays in its parent.
    assert_eq!(tree.node(div).parent(), Some(root));
}

#[test]
fn set_replaces_existing_attribute_in_place() {
    // oracle: e = html.fromstring("<div id='x' class='y'/>"); e.set("id", "z")
    //   -> list(e.attrib.items()) == [("id", "z"), ("class", "y")]
    let mut tree = Tree::new(LocalName::from("div"), Ns::Html, vec![]);
    let div = tree.root();
    tree.set(div, "id", "x");
    tree.set(div, "class", "y");
    tree.set(div, "id", "z");
    assert_eq!(attr_list(&tree, div), [pair("id", "z"), pair("class", "y")]);
    tree.set(div, "data-new", "n");
    assert_eq!(tree.get(div, "data-new"), Some("n"));
    assert_eq!(tree.get(div, "id"), Some("z"));
    assert_eq!(tree.get(div, "missing"), None);
    assert_eq!(tree.remove_attr(div, "id").as_deref(), Some("z"));
    assert_eq!(tree.remove_attr(div, "id"), None);
    assert_eq!(
        attr_list(&tree, div),
        [pair("class", "y"), pair("data-new", "n")]
    );
}

#[test]
fn set_name_renames_in_place() {
    let (mut tree, _, a, _) = div_with_a_b();
    tree.set_name(a, "section");
    assert_eq!(&*tree.node(a).name, "section");
    assert_eq!(tree.node(a).ns, Ns::Html);
}
