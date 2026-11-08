use super::*;
use crate::cancel::{CHECK_INTERVAL, assert_every_check_stops};

/// A cancelled call and a `p` of `2 * CHECK_INTERVAL + 1` elements, `p` included.
fn cancelled_copy_fixture() -> (Tree, NodeId, CancelToken) {
    let items = "<i>x</i>".repeat(2 * CHECK_INTERVAL);
    let tree = fragment(&format!("<p>{items}</p>"));
    let p = tree.first_child(tree.root()).unwrap();
    assert_eq!(tree.iter(p, &[]).count(), 2 * CHECK_INTERVAL + 1);
    let token = CancelToken::new();
    token.cancel();
    (tree, p, token)
}

#[test]
fn copy_stops_when_cancelled() {
    let (mut tree, p, token) = cancelled_copy_fixture();
    assert_eq!(
        tree.copy(p, &Call::default().cancel(&token)),
        Err(ExtractError::Cancelled)
    );
}

#[test]
fn copy_into_stops_when_cancelled() {
    let (tree, p, token) = cancelled_copy_fixture();
    let mut dst = Tree::new(LocalName::from("body"), Ns::Html, vec![]);
    assert_eq!(
        tree.copy_into(p, &mut dst, &Call::default().cancel(&token)),
        Err(ExtractError::Cancelled)
    );
}

/// A `p` with `2 * CHECK_INTERVAL + 1` descendants, so a copy loop checks at its 0th, 256th and
/// 512th node.
fn mid_walk_fragment() -> Tree {
    fragment(&format!(
        "<p>{}</p>",
        "<i>x</i>".repeat(2 * CHECK_INTERVAL + 1)
    ))
}

#[test]
fn copy_stops_mid_walk() {
    let total = assert_every_check_stops(|call| {
        let mut tree = mid_walk_fragment();
        let p = tree.first_child(tree.root()).unwrap();
        tree.copy(p, call)
    });
    assert_eq!(total, 6);
}

#[test]
fn copy_into_stops_mid_walk() {
    let total = assert_every_check_stops(|call| {
        let tree = mid_walk_fragment();
        let p = tree.first_child(tree.root()).unwrap();
        let mut dst = Tree::new(LocalName::from("body"), Ns::Html, vec![]);
        tree.copy_into(p, &mut dst, call)
    });
    assert_eq!(total, 6);
}

#[test]
fn copy_pairs_by_document_order() {
    // oracle: w = etree.fromstring("<w><p>a<b>b</b>c</p>tail<q/>qt</w>"); c = copy.deepcopy(w[0])
    //   -> tostring(c) == b"<p>a<b>b</b>c</p>tail", c.tail == "tail", c.getparent() is None
    //   c = copy.deepcopy(w[1]) -> b"<q/>qt", c.tail == "qt"
    let mut tree = fragment("<p>a<b>b</b>c</p>tail<q></q>qt");
    let root = tree.root();
    let p = tree.first_child(root).unwrap();
    let copy = tree.copy(p, &Call::default()).unwrap();
    assert_eq!(xml(&tree, copy), "<p>a<b>b</b>c</p>");
    assert_eq!(tree.node(copy).tail.as_deref(), Some("tail"));
    assert_eq!(tree.node(copy).parent(), None);
    assert_eq!(tree.node(copy).next_sibling(), None);
    let q = tree
        .copy(tree.last_child(root).unwrap(), &Call::default())
        .unwrap();
    assert_eq!(xml(&tree, q), "<q/>");
    assert_eq!(tree.node(q).tail.as_deref(), Some("qt"));

    // A detached node and an earlier copy sit in the arena: the copy block still pairs with the
    // original by document order.
    let mut tree = fragment("<header>h</header><div id=\"x\">t<p>a<b>b</b>c</p>d<i>i</i></div>");
    let root = tree.root();
    let loose = tree.new_element("loose");
    let header = tree.first_child(root).unwrap();
    let early = tree.copy(header, &Call::default()).unwrap();
    tree.node_mut(root).tail = Some("root tail".into());
    let before = tree.len();
    let copy = tree.copy(root, &Call::default()).unwrap();
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

#[test]
fn live_descendants_excludes_start() {
    // oracle: i = etree.fromstring("<item><item/><item/></item>")
    //   len(list(i.iterdescendants("item"))) == 2, len(list(i.iter("item"))) == 3
    let tree = fragment("<item><item></item><item></item></item>");
    let start = tree.first_child(tree.root()).unwrap();
    let mut live = LiveIter::descendants(&tree, start, &["item"]);
    let mut seen = Vec::new();
    while let Some(id) = live.next(&tree) {
        seen.push(id);
    }
    assert_eq!(seen, tree.children(start).collect::<Vec<_>>());
}

#[test]
fn live_descendants_ends_at_stripped_successor() {
    // oracle: p = etree.fromstring("<p><a/><foo>x</foo><b/></p>"); it = p.iterdescendants()
    //   seen = [next(it).tag]; etree.strip_tags(p, "foo"); seen += [e.tag for e in it]
    //   -> seen == ["a", "foo"]
    let mut tree = fragment("<p><a></a><foo>x</foo><b></b></p>");
    let p = tree.first_child(tree.root()).unwrap();
    let (a, foo) = (tree.child_at(p, 0).unwrap(), tree.child_at(p, 1).unwrap());
    let mut live = LiveIter::descendants(&tree, p, &[]);
    assert_eq!(live.next(&tree), Some(a));
    tree.strip_tags(p, &["foo"]);
    assert_eq!(live.next(&tree), Some(foo));
    assert_eq!(live.next(&tree), None);
}

#[test]
fn root_of_detached_and_attached() {
    let mut tree = fragment("<div><p></p></div>");
    let root = tree.root();
    let div = tree.first_child(root).unwrap();
    let p = tree.first_child(div).unwrap();
    assert_eq!(tree.root_of(p), root);
    assert_eq!(tree.root_of(root), root);
    tree.remove(div);
    assert_eq!(tree.root_of(p), div);
    assert_eq!(tree.root_of(div), div);
    let lone = tree.new_element("x");
    assert_eq!(tree.root_of(lone), lone);
}

#[test]
fn new_element_like_keeps_namespace() {
    let mut tree = parse("<html><body><svg><title>t</title></svg></body></html>");
    let title = find(&tree, "title");
    assert_eq!(tree.node(title).ns, Ns::Svg);
    tree.set(title, "id", "x");
    tree.node_mut(title).tail = Some("tail".to_string());
    let copy = tree.new_element_like(title);
    let node = tree.node(copy);
    assert_eq!((&*node.name, node.ns), ("title", Ns::Svg));
    assert!(node.attrs.is_empty() && node.text.is_none() && node.tail.is_none());
    assert_eq!(node.parent(), None);
}

#[test]
fn try_new_element_like_follows_lxml_tag_rule() {
    // oracle: etree.Element(name) and etree.SubElement(etree.Element("x"), name) agree on every name:
    //   "p", "x-y", "a.b", "_x", "Àx" and "{http://www.w3.org/2000/svg}title" are accepted;
    //   "o:p", "http:" and 'a"b' raise ValueError("Invalid tag name ...")
    let mut tree = parse("<html><body><svg><title>t</title></svg></body></html>");
    let svg_title = find(&tree, "title");
    let accepted = ["p", "x-y", "a.b", "_x", "\u{C0}x"];
    for name in accepted {
        let source = tree.new_element(name);
        assert!(tree.try_new_element_like(source).is_ok(), "{name}");
    }
    let copy = tree.try_new_element_like(svg_title).unwrap();
    assert_eq!(
        (&*tree.node(copy).name, tree.node(copy).ns),
        ("title", Ns::Svg)
    );
    let invalid = Err(ExtractError::Rejected(Rejection::InvalidValue));
    for name in ["o:p", "http:", "a\"b"] {
        let source = tree.new_element(name);
        assert_eq!(tree.try_new_element_like(source), invalid, "{name}");
    }
}
