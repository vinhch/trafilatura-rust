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
fn dump_from_detached_copy() {
    let mut tree = parse("<div id=\"x\">t<p class=\"a\">a<b>b</b>c</p>tail<i>i</i></div>tail2");
    let div = find(&tree, "div");
    let copy = tree.copy(div);
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
    let copy = tree.copy(div);
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

// Walks, strip and copy. Expected values come from lxml 6.1.3 / libxml2 2.14.6 through
// `scripts/oracle.sh`; the trees there are `etree.fromstring` of the same markup, rooted in the
// fragment's `div` here.

fn fragment(html: &str) -> Tree {
    parse_fragment(html, &ParseLimits::default(), &Call::default()).unwrap()
}

/// `<a>text<b/>tail</a>` of `id` and its descendants, without the tail of `id` itself.
fn xml(tree: &Tree, id: NodeId) -> String {
    let node = tree.node(id);
    let name = &*node.name;
    if node.text.is_none() && tree.first_child(id).is_none() {
        return format!("<{name}/>");
    }
    let mut out = format!("<{name}>{}", node.text.as_deref().unwrap_or(""));
    for child in tree.children(id) {
        out.push_str(&xml(tree, child));
        out.push_str(tree.node(child).tail.as_deref().unwrap_or(""));
    }
    out.push_str(&format!("</{name}>"));
    out
}

fn tags(tree: &Tree, ids: impl IntoIterator<Item = NodeId>) -> Vec<String> {
    ids.into_iter()
        .map(|id| tree.node(id).name.to_string())
        .collect()
}

/// Runs a live iteration that removes every yielded node; returns the yielded ids.
fn live_remove_all(tree: &mut Tree, start: NodeId, tags: &'static [&'static str]) -> Vec<NodeId> {
    let mut live = LiveIter::new(tree, start, tags);
    let mut seen = Vec::new();
    while let Some(id) = live.next(tree) {
        seen.push(id);
        tree.remove(id);
    }
    seen
}

#[test]
fn iter_live_skips_after_deleted_nested() {
    // oracle: b = etree.fromstring("<body><aside><aside/></aside><aside/></body>"); seen = []
    //   for e in b.iter("aside"): seen.append(e.tag); e.getparent().remove(e)
    //   -> len(seen) == 2, etree.tostring(b) == b"<body><aside/></body>"
    let mut tree = fragment("<aside><aside></aside></aside><aside></aside>");
    let root = tree.root();
    let last = tree.last_child(root).unwrap();
    let seen = live_remove_all(&mut tree, root, &["aside"]);
    assert_eq!(seen.len(), 2);
    assert_eq!(tree.children(root).collect::<Vec<_>>(), [last]);
    assert_eq!(xml(&tree, root), "<div><aside/></div>");
}

#[test]
fn iter_live_accepts_a_runtime_tag_list() {
    let tree = fragment("<q></q><i></i><q></q>");
    let root = tree.root();
    let wanted: Vec<&str> = vec!["q"];
    let mut live = LiveIter::new(&tree, root, &wanted);
    let mut seen = 0;
    while live.next(&tree).is_some() {
        seen += 1;
    }
    assert_eq!(seen, 2);
}

#[test]
fn iter_live_removing_current_still_visits_later_siblings() {
    // oracle: b = etree.fromstring("<body><a/><a/><a/></body>"); n = 0
    //   for e in b.iter("a"): n += 1; b.remove(e)   -> n == 3, tostring(b) == b"<body/>"
    let mut tree = fragment("<a></a><a></a><a></a>");
    let root = tree.root();
    assert_eq!(live_remove_all(&mut tree, root, &["a"]).len(), 3);
    assert_eq!(tree.child_count(root), 0);

    // oracle: b = etree.fromstring("<body><div><aside><x><aside/></x></aside><aside/></div><aside/></body>")
    //   for e in b.iter("aside"): seen.append(e.tag); e.getparent().remove(e)
    //   -> len(seen) == 2, tostring(b) == b"<body><div><aside/></div><aside/></body>"
    let mut tree =
        fragment("<div><aside><x><aside></aside></x></aside><aside></aside></div><aside></aside>");
    let root = tree.root();
    assert_eq!(live_remove_all(&mut tree, root, &["aside"]).len(), 2);
    assert_eq!(xml(&tree, root), "<div><div><aside/></div><aside/></div>");

    // oracle: b = etree.fromstring("<body><div><q><q/></q></div><q/></body>")
    //   for e in b.iter("q"): seen.append(e.tag)
    //     if e.getparent().tag == "div": e.getparent().remove(e)
    //   -> len(seen) == 2, tostring(b) == b"<body><div/><q/></body>"
    let mut tree = fragment("<div><q><q></q></q></div><q></q>");
    let root = tree.root();
    let div = tree.first_child(root).unwrap();
    let mut live = LiveIter::new(&tree, root, &["q"]);
    let mut seen = 0;
    while let Some(id) = live.next(&tree) {
        seen += 1;
        if tree.node(id).parent() == Some(div) {
            tree.remove(id);
        }
    }
    assert_eq!(seen, 2);
    assert_eq!(xml(&tree, root), "<div><div/><q/></div>");
}

#[test]
fn iter_live_revisits_a_node_moved_ahead() {
    // oracle: b = etree.fromstring("<body><a/><b/><c/></body>"); seen = []
    //   for e in b.iter():
    //     seen.append(e.tag)
    //     if e.tag == "a": b.append(e)
    //   -> seen == ["body", "a", "b", "c", "a"], tostring(b) == b"<body><b/><c/><a/></body>"
    let mut tree = fragment("<a></a><b></b><c></c>");
    let root = tree.root();
    let mut live = LiveIter::new(&tree, root, &[]);
    let mut seen = Vec::new();
    while let Some(id) = live.next(&tree) {
        seen.push(id);
        if &*tree.node(id).name == "a" {
            tree.append(root, id);
        }
    }
    assert_eq!(tags(&tree, seen), ["div", "a", "b", "c", "a"]);
    assert_eq!(xml(&tree, root), "<div><b/><c/><a/></div>");
}

#[test]
fn iter_includes_self_and_filters_tags() {
    // oracle: b = etree.fromstring("<body><q><i/><q/></q><i/></body>")
    //   [e.tag for e in b.iter("i")] == ["i", "i"]; [e.tag for e in b.iter("q", "i")] == ["q", "i", "q", "i"]
    let tree = fragment("<q><i></i><q></q></q><i></i>");
    let root = tree.root();
    assert_eq!(tags(&tree, tree.iter(root, &["i"])), ["i", "i"]);
    assert_eq!(
        tags(&tree, tree.iter(root, &["q", "i"])),
        ["q", "i", "q", "i"]
    );
    assert_eq!(
        tags(&tree, tree.iter(root, &[])),
        ["div", "q", "i", "q", "i"]
    );
    // The start matches the filter: it is yielded first.
    assert_eq!(tree.iter(root, &["div"]).collect::<Vec<_>>(), [root]);
    assert_eq!(tree.iter(root, &["nope"]).count(), 0);
}

#[test]
fn iter_on_a_detached_subtree_stays_inside_it() {
    // oracle: b = etree.fromstring("<body><div><p/></div></body>"); d = b[0]; b.remove(d)
    //   [e.tag for e in d.iter()] == ["div", "p"]
    let mut tree = fragment("<div><p></p></div>");
    let div = tree.first_child(tree.root()).unwrap();
    tree.remove(div);
    assert_eq!(tags(&tree, tree.iter(div, &[])), ["div", "p"]);
}

#[test]
fn iterdescendants_excludes_self() {
    // oracle: b = etree.fromstring("<body><div><p/><p/></div><p/></body>"); d = b[0]
    //   [e.tag for e in d.iter()] == ["div", "p", "p"]; [e.tag for e in d.iterdescendants()] == ["p", "p"]
    let tree = fragment("<div><p></p><p></p></div><p></p>");
    let div = tree.first_child(tree.root()).unwrap();
    assert_eq!(tags(&tree, tree.iter(div, &[])), ["div", "p", "p"]);
    assert_eq!(tags(&tree, tree.iterdescendants(div, &[])), ["p", "p"]);
    assert_eq!(tree.iterdescendants(div, &["div"]).count(), 0);
    assert_eq!(tree.iterdescendants(div, &["p"]).count(), 2);
    assert_eq!(tree.find(div, "p"), tree.child_at(div, 0));
    assert_eq!(tree.find(div, "div"), None);
}

#[test]
fn iterancestors_order_is_parent_first() {
    // oracle: b = etree.fromstring("<body><div><p/><p/></div><p/></body>"); d = b[0]
    //   [e.tag for e in d[1].iterancestors()] == ["div", "body"]
    let tree = fragment("<div><p></p><p></p></div><p></p>");
    let root = tree.root();
    let div = tree.first_child(root).unwrap();
    let p = tree.child_at(div, 1).unwrap();
    assert_eq!(tree.iterancestors(p, &[]).collect::<Vec<_>>(), [div, root]);
    assert_eq!(tree.iterancestors(p, &["div"]).count(), 2);
    assert_eq!(tree.iterancestors(p, &["p"]).count(), 0);
    assert_eq!(tree.iterancestors(root, &[]).count(), 0);
}

#[test]
fn itertext_excludes_own_tail() {
    // oracle: w = etree.fromstring("<w><p>a<b>b</b>c</p>tail</w>"); p = w[0]
    //   list(p.itertext()) == ["a", "b", "c"]; p.tail == "tail"
    let tree = fragment("<p>a<b>b</b>c</p>tail");
    let p = tree.first_child(tree.root()).unwrap();
    assert_eq!(tree.itertext(p).collect::<Vec<_>>(), ["a", "b", "c"]);
    assert_eq!(tree.text_content(p), "abc");
    assert_eq!(tree.node(p).tail.as_deref(), Some("tail"));
    // Every descendant's tail counts.
    assert_eq!(
        tree.itertext(tree.root()).collect::<Vec<_>>(),
        ["a", "b", "c", "tail"]
    );
}

#[test]
fn strip_tags_splices_text_children_and_tail() {
    // oracle: d = etree.fromstring("<div>x<span>a<b>b</b>c</span>y</div>"); etree.strip_tags(d, "span")
    //   -> d.text == "xa", [(e.tag, e.text, e.tail) for e in d] == [("b", "b", "cy")]
    let mut tree = fragment("x<span>a<b>b</b>c</span>y");
    let root = tree.root();
    tree.strip_tags(root, &["span"]);
    let b = tree.first_child(root).unwrap();
    assert_eq!(tree.node(root).text.as_deref(), Some("xa"));
    assert_eq!(tree.child_count(root), 1);
    assert_eq!(&*tree.node(b).name, "b");
    assert_eq!(tree.node(b).text.as_deref(), Some("b"));
    assert_eq!(tree.node(b).tail.as_deref(), Some("cy"));
    assert_eq!(tree.node(b).parent(), Some(root));

    // oracle: d = etree.fromstring("<div>x<a/>y<span>s<b/>t</span>z<c/>w</div>"); strip_tags(d, "span")
    //   -> b"<div>x<a/>ys<b/>tz<c/>w</div>"
    let mut tree = fragment("x<a></a>y<span>s<b></b>t</span>z<c></c>w");
    let root = tree.root();
    tree.strip_tags(root, &["span"]);
    assert_eq!(xml(&tree, root), "<div>x<a/>ys<b/>tz<c/>w</div>");
}

#[test]
fn strip_tags_text_goes_to_previous_tail_or_parent_text() {
    // oracle: d = etree.fromstring("<div><span>s</span>z<a/></div>"); strip_tags(d, "span")
    //   -> b"<div>sz<a/></div>"
    let mut tree = fragment("<span>s</span>z<a></a>");
    let root = tree.root();
    tree.strip_tags(root, &["span"]);
    assert_eq!(xml(&tree, root), "<div>sz<a/></div>");

    // oracle: d = etree.fromstring("<div>p<span><b/>q</span>z</div>"); strip_tags(d, "span")
    //   -> b"<div>p<b/>qz</div>"
    let mut tree = fragment("p<span><b></b>q</span>z");
    let root = tree.root();
    tree.strip_tags(root, &["span"]);
    assert_eq!(xml(&tree, root), "<div>p<b/>qz</div>");

    // oracle: d = etree.fromstring("<div>p<span>s</span>z</div>"); strip_tags(d, "span")
    //   -> b"<div>psz</div>"
    let mut tree = fragment("p<span>s</span>z");
    let root = tree.root();
    tree.strip_tags(root, &["span"]);
    assert_eq!(xml(&tree, root), "<div>psz</div>");

    // oracle: d = etree.fromstring("<div><p>a</p><span><b>x</b></span></div>"); strip_tags(d, "span", "b")
    //   -> b"<div><p>a</p>x</div>"
    let mut tree = fragment("<p>a</p><span><b>x</b></span>");
    let root = tree.root();
    tree.strip_tags(root, &["span", "b"]);
    assert_eq!(xml(&tree, root), "<div><p>a</p>x</div>");
}

#[test]
fn strip_tags_nested_same_tag() {
    // oracle: d = etree.fromstring("<div><i>a<i>b</i>c</i>d</div>"); etree.strip_tags(d, "i")
    //   -> d.text == "abcd", len(d) == 0
    let mut tree = fragment("<i>a<i>b</i>c</i>d");
    let root = tree.root();
    tree.strip_tags(root, &["i"]);
    assert_eq!(tree.node(root).text.as_deref(), Some("abcd"));
    assert_eq!(tree.child_count(root), 0);
}

#[test]
fn strip_tags_keeps_the_node_itself_and_does_nothing_without_tags() {
    // oracle: d = etree.fromstring("<w><div>x<div>y</div>t</div>u</w>")[0]; strip_tags(d, "div")
    //   -> b"<div>xyt</div>", d.tail == "u"
    let mut tree = fragment("<div>x<div>y</div>t</div>u");
    let div = tree.first_child(tree.root()).unwrap();
    tree.strip_tags(div, &["div"]);
    assert_eq!(xml(&tree, div), "<div>xyt</div>");
    assert_eq!(tree.node(div).tail.as_deref(), Some("u"));
    assert_eq!(tree.node(div).parent(), Some(tree.root()));

    // oracle: d = etree.fromstring("<div>x<a>1</a>y<s/>z</div>"); strip_tags(d); strip_elements(d)
    //   -> both leave b"<div>x<a>1</a>y<s/>z</div>"
    let mut tree = fragment("x<a>1</a>y<s></s>z");
    let root = tree.root();
    tree.strip_tags(root, &[]);
    tree.strip_elements(root, &[], true);
    assert_eq!(xml(&tree, root), "<div>x<a>1</a>y<s/>z</div>");
}

#[test]
fn strip_elements_takes_tail() {
    // oracle: d = etree.fromstring("<div>a<s>1</s>t1<p>b<s>2</s>t2</p><s/>t3</div>")
    //   strip_elements(d, "s", with_tail=True)  -> b"<div>a<p>b</p></div>"
    //   strip_elements(d, "s", with_tail=False) -> b"<div>at1<p>bt2</p>t3</div>"
    let html = "a<s>1</s>t1<p>b<s>2</s>t2</p><s></s>t3";
    let mut tree = fragment(html);
    let root = tree.root();
    tree.strip_elements(root, &["s"], true);
    assert_eq!(xml(&tree, root), "<div>a<p>b</p></div>");
    let mut tree = fragment(html);
    tree.strip_elements(root, &["s"], false);
    assert_eq!(xml(&tree, root), "<div>at1<p>bt2</p>t3</div>");

    // oracle: d = etree.fromstring("<div><s>1</s>t1<p/></div>")
    //   with_tail=True -> b"<div><p/></div>"; with_tail=False -> b"<div>t1<p/></div>"
    let html = "<s>1</s>t1<p></p>";
    let mut tree = fragment(html);
    tree.strip_elements(root, &["s"], true);
    assert_eq!(xml(&tree, root), "<div><p/></div>");
    let mut tree = fragment(html);
    tree.strip_elements(root, &["s"], false);
    assert_eq!(xml(&tree, root), "<div>t1<p/></div>");
}

#[test]
fn strip_elements_drops_nested_matches_with_their_parent() {
    // oracle: d = etree.fromstring("<div><s>1<s>x</s></s>t1<p/></div>"); strip_elements(d, "s")
    //   -> b"<div><p/></div>"
    let mut tree = fragment("<s>1<s>x</s></s>t1<p></p>");
    let root = tree.root();
    tree.strip_elements(root, &["s"], true);
    assert_eq!(xml(&tree, root), "<div><p/></div>");

    // oracle: d = etree.fromstring("<w><s>1</s>t</w>")[0]; strip_elements(d, "s")
    //   -> b"<s>1</s>" unchanged, d.tail == "t"
    let mut tree = fragment("<s>1</s>t");
    let s = tree.first_child(tree.root()).unwrap();
    tree.strip_elements(s, &["s"], true);
    assert_eq!(xml(&tree, s), "<s>1</s>");
    assert_eq!(tree.node(s).tail.as_deref(), Some("t"));
    assert_eq!(tree.node(s).parent(), Some(tree.root()));
}

#[test]
fn copy_pairs_by_document_order() {
    // oracle: w = etree.fromstring("<w><p>a<b>b</b>c</p>tail<q/>qt</w>"); c = copy.deepcopy(w[0])
    //   -> tostring(c) == b"<p>a<b>b</b>c</p>tail", c.tail == "tail", c.getparent() is None
    //   c = copy.deepcopy(w[1]) -> b"<q/>qt", c.tail == "qt"
    let mut tree = fragment("<p>a<b>b</b>c</p>tail<q></q>qt");
    let root = tree.root();
    let p = tree.first_child(root).unwrap();
    let copy = tree.copy(p);
    assert_eq!(xml(&tree, copy), "<p>a<b>b</b>c</p>");
    assert_eq!(tree.node(copy).tail.as_deref(), Some("tail"));
    assert_eq!(tree.node(copy).parent(), None);
    assert_eq!(tree.node(copy).next_sibling(), None);
    let q = tree.copy(tree.last_child(root).unwrap());
    assert_eq!(xml(&tree, q), "<q/>");
    assert_eq!(tree.node(q).tail.as_deref(), Some("qt"));

    // A detached node and an earlier copy sit in the arena: the copy block still pairs with the
    // original by document order.
    let mut tree = fragment("<header>h</header><div id=\"x\">t<p>a<b>b</b>c</p>d<i>i</i></div>");
    let root = tree.root();
    let loose = tree.new_element("loose");
    let header = tree.first_child(root).unwrap();
    let early = tree.copy(header);
    tree.node_mut(root).tail = Some("root tail".into());
    let before = tree.len();
    let copy = tree.copy(root);
    let size = tree.iter(root, &[]).count();
    assert_eq!(size, 6);
    assert_eq!(tree.len(), before + size);
    let originals: Vec<_> = tree.iter(root, &[]).collect();
    let copies: Vec<_> = tree.iter(copy, &[]).collect();
    assert_eq!(copies.len(), size);
    for (orig, new) in originals.iter().zip(&copies) {
        assert_ne!(orig, new);
        let (orig, new) = (tree.node(*orig), tree.node(*new));
        assert_eq!(orig.name, new.name);
        assert_eq!(orig.text, new.text);
        assert_eq!(orig.tail, new.tail);
        assert_eq!(orig.attrs.len(), new.attrs.len());
    }
    assert_eq!(tree.node(copy).tail.as_deref(), Some("root tail"));
    assert_eq!(tree.node(copy).parent(), None);
    assert_eq!(tree.get(tree.child_at(copy, 1).unwrap(), "id"), Some("x"));
    assert_eq!(tree.node(early).parent(), None);
    assert_eq!(tree.node(loose).parent(), None);
    // The original is untouched and the copy is independent of it.
    let expected = "<div><header>h</header><div>t<p>a<b>b</b>c</p>d<i>i</i></div></div>";
    assert_eq!(xml(&tree, root), expected);
    tree.strip_tags(copy, &["p"]);
    assert_eq!(xml(&tree, root), expected);
}

#[test]
fn deep_chain_walks_iteratively() {
    // Recursion that follows the depth would overflow 64 KiB long before 256 levels.
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(|| {
            let mut tree = Tree::new(LocalName::from("div"), Ns::Html, vec![]);
            let mut parent = tree.root();
            tree.node_mut(parent).text = Some("x".into());
            for _ in 1..256 {
                let child = tree.new_element("div");
                tree.node_mut(child).text = Some("x".into());
                tree.append(parent, child);
                parent = child;
            }
            assert_eq!(tree.len(), 256);

            let copy = tree.copy(tree.root());
            assert_eq!(tree.len(), 512);
            assert_eq!(tree.iter(copy, &[]).count(), 256);
            assert_eq!(tree.iterdescendants(copy, &["div"]).count(), 255);
            assert_eq!(tree.iterancestors(parent, &[]).count(), 255);
            assert_eq!(tree.itertext(copy).count(), 256);
            assert_eq!(tree.text_content(tree.root()).len(), 256);

            let root = tree.root();
            tree.strip_tags(root, &["div"]);
            assert_eq!(tree.child_count(root), 0);
            assert_eq!(tree.node(root).text.as_deref().map(str::len), Some(256));
            tree.strip_elements(copy, &["div"], true);
            assert_eq!(tree.child_count(copy), 0);
        })
        .unwrap()
        .join()
        .unwrap();
}

const SVG_PAGE: &str = "<html><body><svg><title>t</title><a>x</a></svg><a>y</a></body></html>";

#[test]
fn svg_elements_skip_name_tests() {
    let mut tree = parse(SVG_PAGE);
    let root = tree.root();
    let all = tree.iter(root, &[]).count();
    // oracle: [e.tag for e in r.iter('a')] == ['a']
    let anchors: Vec<NodeId> = tree.iter(root, &["a"]).collect();
    assert_eq!(anchors.len(), 1);
    assert_eq!(tree.text_content(anchors[0]), "y");
    assert_eq!(tree.find(root, "title"), None);
    // oracle: svg title still present after etree.strip_elements(r, 'title')
    tree.strip_elements(root, &["title"], true);
    assert_eq!(tree.iter(root, &[]).count(), all);
    // lxml's strip_tags compares the same `{ns}a` tag, so the SVG `a` stays
    tree.strip_tags(root, &["a"]);
    assert_eq!(tree.iter(root, &[]).count(), all - 1);
    assert!(
        tree.iter(root, &[])
            .any(|id| tree.node(id).ns == Ns::Svg && &*tree.node(id).name == "a")
    );
    assert!(tree.iter(root, &[]).any(|id| tree.node(id).ns == Ns::Svg));
}

#[test]
fn set_name_drops_namespace() {
    let mut tree = parse(SVG_PAGE);
    let root = tree.root();
    let a = tree
        .iter(root, &[])
        .find(|&id| tree.node(id).ns == Ns::Svg && &*tree.node(id).name == "a")
        .unwrap();
    tree.set_name(a, "ref");
    // oracle: x.tag = 'ref'; r.xpath('.//ref') finds it
    assert_eq!(tree.node(a).ns, Ns::Html);
    assert_eq!(tree.iter(root, &["ref"]).collect::<Vec<_>>(), vec![a]);
}

#[test]
fn clear_attrs_keeps_text_tail_children() {
    let mut tree = parse("<div><p id=\"a\" class=\"b\">text<b>x</b>tail</p>after</div>");
    let p = find(&tree, "p");
    tree.clear_attrs(p);
    assert!(tree.node(p).attrs.is_empty());
    assert_eq!(tree.node(p).text.as_deref(), Some("text"));
    assert_eq!(tree.node(p).tail.as_deref(), Some("after"));
    let b = find(&tree, "b");
    assert_eq!(tree.first_child(p), Some(b));
    assert_eq!(tree.node(b).tail.as_deref(), Some("tail"));
}
