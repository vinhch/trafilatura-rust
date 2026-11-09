use super::*;

// Tree edits. Expected values come from lxml (pinned in `tools/parity/uv.lock`, its libxml2 named in
// `src/dom/html_cases.rs`) through `scripts/oracle.sh`.

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
