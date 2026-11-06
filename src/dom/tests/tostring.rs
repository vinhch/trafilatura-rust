use super::*;
use crate::dom::html_cases::{
    TOSTRING_HTML_CASES, TOSTRING_HTML_ELEMENT_CASES, TOSTRING_HTML_TAIL_CASES,
};

/// A detached `x` with a child per `(text, tail)` of `children`, named `c0`, `c1`, ...
fn x_with(text: Option<&str>, children: &[(Option<&str>, Option<&str>)]) -> (Tree, Vec<NodeId>) {
    let mut tree = Tree::new(LocalName::from("x"), Ns::Html, Vec::new());
    let root = tree.root();
    tree.node_mut(root).text = text.map(str::to_string);
    let ids = children
        .iter()
        .enumerate()
        .map(|(index, &(text, tail))| {
            let id = tree.add_child(root, LocalName::from(format!("c{index}")), Ns::Html, vec![]);
            tree.node_mut(id).text = text.map(str::to_string);
            tree.node_mut(id).tail = tail.map(str::to_string);
            id
        })
        .collect();
    (tree, ids)
}

fn pieces(tree: &Tree) -> Vec<&str> {
    tree.itertext_lxml(tree.root()).collect()
}

#[test]
fn itertext_lxml_matches_lxml() {
    // Each expected list is `list(x.itertext())` from the oracle on the same lxml tree
    // (`e.text = ""` and `e.tail = ""` store an empty text node).
    // oracle: x = etree.Element("x"); x.text = ""
    let (tree, _) = x_with(Some(""), &[]);
    assert_eq!(pieces(&tree), [""]);
    assert_eq!(tree.itertext(tree.root()).count(), 0);

    // oracle: x = etree.fromstring("<x><a/><b/></x>"); x[0].tail = ""
    let (tree, _) = x_with(None, &[(None, Some("")), (None, None)]);
    assert_eq!(pieces(&tree), [""]);

    // oracle: x.text = ""; x[0].tail = ""  (on <x><a/></x>)
    let (tree, _) = x_with(Some(""), &[(None, Some(""))]);
    assert_eq!(pieces(&tree), ["", ""]);

    // handle_textnode: a.text, a.tail = a.tail, ""
    // oracle: x = etree.fromstring("<x><a/><b/></x>"); a = x[0]; a.tail = "t"
    //   a.text, a.tail = a.tail, ""; list(x.itertext()) == ["t", ""]
    let (mut tree, ids) = x_with(None, &[(None, Some("t")), (None, None)]);
    let node = tree.node_mut(ids[0]);
    node.text = node.tail.take();
    node.tail = Some(String::new());
    assert_eq!(pieces(&tree), ["t", ""]);

    // strip_tags: `join` of None plus Some("") stays an empty piece, the empty text node lxml
    // moves up when it has nothing to merge with.
    // oracle: x = etree.fromstring("<x><s/></x>"); x[0].text = ""; etree.strip_tags(x, "s")
    //   list(x.itertext()) == [""]
    let (mut tree, _) = x_with(None, &[(Some(""), None)]);
    tree.strip_tags(tree.root(), &["c0"]);
    assert_eq!(pieces(&tree), [""]);

    // oracle: x = etree.fromstring("<x><a/><s/></x>"); x[1].text = ""; etree.strip_tags(x, "s")
    let (mut tree, _) = x_with(None, &[(None, None), (Some(""), None)]);
    tree.strip_tags(tree.root(), &["c1"]);
    assert_eq!(pieces(&tree), [""]);

    // oracle: x = etree.fromstring("<x><s/></x>"); x[0].tail = ""; etree.strip_tags(x, "s")
    let (mut tree, _) = x_with(None, &[(None, Some(""))]);
    tree.strip_tags(tree.root(), &["c0"]);
    assert_eq!(pieces(&tree), [""]);

    // oracle: x = etree.fromstring("<x><s><c/></s></x>"); x[0].tail = ""; etree.strip_tags(x, "s")
    let (mut tree, ids) = x_with(None, &[(None, Some(""))]);
    let inner = tree.add_child(ids[0], LocalName::from("c"), Ns::Html, vec![]);
    tree.strip_tags(tree.root(), &["c0"]);
    assert_eq!(tree.node(inner).tail.as_deref(), Some(""));
    assert_eq!(pieces(&tree), [""]);

    // An empty text node merges into the text next to it.
    // oracle: x = etree.fromstring("<x>t<s/></x>"); x[0].text = ""; etree.strip_tags(x, "s")
    //   list(x.itertext()) == ["t"]
    let (mut tree, _) = x_with(Some("t"), &[(Some(""), None)]);
    tree.strip_tags(tree.root(), &["c0"]);
    assert_eq!(pieces(&tree), ["t"]);
}

/// Adds a child element named `name` under `parent` with the given text and tail.
fn add(
    tree: &mut Tree,
    parent: NodeId,
    name: &str,
    text: Option<&str>,
    tail: Option<&str>,
) -> NodeId {
    let id = tree.new_element(name);
    tree.append(parent, id);
    tree.node_mut(id).text = text.map(str::to_string);
    tree.node_mut(id).tail = tail.map(str::to_string);
    id
}

fn root_named(name: &str, text: Option<&str>) -> Tree {
    let mut tree = Tree::new(LocalName::from(name), Ns::Html, vec![]);
    let root = tree.root();
    tree.node_mut(root).text = text.map(str::to_string);
    tree
}

#[test]
fn tostring_matches_lxml() {
    // Each expected string is `etree.tostring(e, encoding="unicode")` from the oracle.
    // oracle: etree.Element("x")
    let tree = root_named("x", None);
    assert_eq!(tree.tostring(tree.root(), false, true), "<x/>");

    // oracle: e = etree.Element("x"); e.text = ""
    let tree = root_named("x", Some(""));
    assert_eq!(tree.tostring(tree.root(), false, true), "<x></x>");

    // oracle: e = etree.Element("x"); e.set("b", "1"); e.set("a", "2")
    let mut tree = root_named("x", None);
    let root = tree.root();
    tree.set(root, "b", "1");
    tree.set(root, "a", "2");
    assert_eq!(tree.tostring(root, false, true), r#"<x b="1" a="2"/>"#);

    // oracle: x.text = "a & b < c > d"; c = etree.SubElement(x, "c"); c.tail = "e & f < g > h"
    //   (etree.tostring(c, ...), and with with_tail=False)
    let mut tree = root_named("x", Some("a & b < c > d"));
    let root = tree.root();
    let c = add(&mut tree, root, "c", None, Some("e & f < g > h"));
    assert_eq!(
        tree.tostring(root, false, true),
        "<x>a &amp; b &lt; c &gt; d<c/>e &amp; f &lt; g &gt; h</x>"
    );
    assert_eq!(tree.tostring(c, false, true), "<c/>e &amp; f &lt; g &gt; h");
    assert_eq!(tree.tostring(c, false, false), "<c/>");

    // oracle: e.set("v", 'q"\n\t\r<&>')
    let mut tree = root_named("x", None);
    let root = tree.root();
    tree.set(root, "v", "q\"\n\t\r<&>");
    assert_eq!(
        tree.tostring(root, false, true),
        r#"<x v="q&quot;&#10;&#9;&#13;&lt;&amp;&gt;"/>"#
    );

    // oracle: e = etree.Element("x"); e.text = "a\rb"
    let tree = root_named("x", Some("a\rb"));
    assert_eq!(tree.tostring(tree.root(), false, true), "<x>a&#13;b</x>");

    // oracle: x.text = "t"; c = SubElement(x, "c"); c.text = ""; c.tail = ""; d = SubElement(x, "d"); d.tail = "z"
    let mut tree = root_named("x", Some("t"));
    let root = tree.root();
    add(&mut tree, root, "c", Some(""), Some(""));
    add(&mut tree, root, "d", None, Some("z"));
    assert_eq!(tree.tostring(root, false, true), "<x>t<c></c><d/>z</x>");
}

#[test]
fn tostring_html_matches_lxml() {
    // Each page is `lxml.html.tostring(html, encoding="unicode")` of the page parsed by html5lib.
    for &(page, expected) in TOSTRING_HTML_CASES {
        let tree = parse(page);
        assert_eq!(
            tree.tostring_html(tree.root(), true),
            expected,
            "page {page:?}"
        );
    }
}

#[test]
fn tostring_html_elements_match_lxml() {
    // Each row is `lxml.html.tostring` of `etree.Element(name)` with the given text.
    for &(name, text, expected) in TOSTRING_HTML_ELEMENT_CASES {
        let tree = root_named(name, text);
        assert_eq!(
            tree.tostring_html(tree.root(), true),
            expected,
            "element {name}"
        );
    }
}

#[test]
fn tostring_html_tail() {
    // `with_tail` decides whether the first `p` writes its own tail.
    for &(page, with_tail, without_tail) in TOSTRING_HTML_TAIL_CASES {
        let tree = parse(page);
        let p = find(&tree, "p");
        assert_eq!(tree.tostring_html(p, true), with_tail, "page {page:?}");
        assert_eq!(tree.tostring_html(p, false), without_tail, "page {page:?}");
    }
}

#[test]
fn tostring_pretty_matches_lxml() {
    // oracle: etree.tostring(body, encoding="unicode", pretty_print=True) for each tree below
    let mut tree = root_named("body", None);
    let body = tree.root();
    add(&mut tree, body, "p", Some("a"), None);
    add(&mut tree, body, "p", None, None);
    let div = add(&mut tree, body, "div", None, None);
    add(&mut tree, div, "p", Some("x"), None);
    assert_eq!(
        tree.tostring(body, true, true),
        "<body>\n  <p>a</p>\n  <p/>\n  <div>\n    <p>x</p>\n  </div>\n</body>\n"
    );

    let mut tree = root_named("body", None);
    let body = tree.root();
    let p = add(&mut tree, body, "p", Some("a"), None);
    add(&mut tree, p, "b", Some("c"), Some("d"));
    let list = add(&mut tree, body, "list", None, None);
    add(&mut tree, list, "item", Some("i"), None);
    assert_eq!(
        tree.tostring(body, true, true),
        "<body>\n  <p>a<b>c</b>d</p>\n  <list>\n    <item>i</item>\n  </list>\n</body>\n"
    );

    let mut tree = root_named("body", Some("t"));
    let body = tree.root();
    add(&mut tree, body, "p", Some("a"), None);
    assert_eq!(tree.tostring(body, true, true), "<body>t<p>a</p></body>\n");

    let mut tree = root_named("body", None);
    let body = tree.root();
    let first = add(&mut tree, body, "p", Some("a"), Some("tail"));
    add(&mut tree, body, "p", Some("b"), None);
    assert_eq!(
        tree.tostring(body, true, true),
        "<body><p>a</p>tail<p>b</p></body>\n"
    );
    // A tail of `""` is a text node too.
    tree.node_mut(first).tail = Some(String::new());
    assert_eq!(
        tree.tostring(body, true, true),
        "<body><p>a</p><p>b</p></body>\n"
    );

    let mut tree = root_named("body", None);
    let body = tree.root();
    let div = add(&mut tree, body, "div", Some(" "), None);
    add(&mut tree, div, "p", Some("x"), None);
    assert_eq!(
        tree.tostring(body, true, true),
        "<body>\n  <div> <p>x</p></div>\n</body>\n"
    );

    let tree = root_named("p", Some(""));
    assert_eq!(tree.tostring(tree.root(), true, true), "<p></p>\n");

    // oracle: x.text = "a & b < c > d\r"; x.set("v", 'q"\n\t\r<&>'); c = SubElement(x, "c"); c.tail = "e"
    let mut tree = root_named("x", Some("a & b < c > d\r"));
    let x = tree.root();
    tree.set(x, "v", "q\"\n\t\r<&>");
    add(&mut tree, x, "c", None, Some("e"));
    assert_eq!(
        tree.tostring(x, true, true),
        "<x v=\"q&quot;&#10;&#9;&#13;&lt;&amp;&gt;\">a &amp; b &lt; c &gt; d&#13;<c/>e</x>\n"
    );

    // oracle: etree.tostring(b, encoding="unicode", pretty_print=True, with_tail=True)
    let mut tree = root_named("a", None);
    let a = tree.root();
    let b = add(&mut tree, a, "b", Some("x"), Some("t"));
    assert_eq!(tree.tostring(b, true, true), "<b>x</b>t\n");
    assert_eq!(tree.tostring(b, true, false), "<b>x</b>\n");

    let mut tree = root_named("a", None);
    let a = tree.root();
    let b = add(&mut tree, a, "b", None, Some("t"));
    add(&mut tree, b, "c", Some("x"), None);
    assert_eq!(tree.tostring(b, true, true), "<b>\n  <c>x</c>\n</b>t\n");
}

#[test]
fn tostring_pretty_caps_indent_at_60() {
    // oracle: 35 nested div around a p, etree.tostring(root, pretty_print=True): lines at depth 28,
    //   29, 30 and 31 start with 56, 58, 60 and 60 spaces
    let mut tree = root_named("div", None);
    let mut parent = tree.root();
    for _ in 1..35 {
        parent = add(&mut tree, parent, "div", None, None);
    }
    add(&mut tree, parent, "p", Some("x"), None);
    let out = tree.tostring(tree.root(), true, false);
    let lines: Vec<&str> = out.lines().collect();
    let indent = |line: &str| line.len() - line.trim_start().len();
    let indents: Vec<usize> = lines.iter().take(32).map(|line| indent(line)).collect();
    assert_eq!(indents.get(28..32), Some(&[56, 58, 60, 60][..]));
    // The closing tags climb back down the same way.
    assert_eq!(lines.len(), 71);
    assert_eq!(lines.get(70).copied(), Some("</div>"));
    assert_eq!(indent(lines.get(36).copied().unwrap_or("")), 60);
}

#[test]
fn tostring_foreign_by_local_name() {
    let tree = parse("<html><body><svg><title>t</title></svg></body></html>");
    let title = find(&tree, "title");
    assert_eq!(tree.node(title).ns, Ns::Svg);
    assert_eq!(tree.tostring(title, false, false), "<title>t</title>");
    let svg = find(&tree, "svg");
    assert_eq!(
        tree.tostring(svg, true, false),
        "<svg>\n  <title>t</title>\n</svg>\n"
    );
}

#[test]
fn copy_into_matches_copy() {
    let mut tree =
        fragment("<p id=\"x\" class=\"c\">a<b k=\"1\">b</b>c<i>d<u></u>e</i></p>tail<q></q>qt");
    let root = tree.root();
    let p = tree.first_child(root).unwrap();
    let before = tree.tostring(root, false, true);
    let size = tree.iter(p, &[]).count();

    let mut dst = Tree::new(LocalName::from("body"), Ns::Html, vec![]);
    let loose = dst.new_element("loose");
    let len = dst.len();
    let into = tree.copy_into(p, &mut dst, &Call::default()).unwrap();
    assert_eq!(dst.len(), len + size);
    assert_eq!(dst.node(into).parent(), None);
    assert_eq!(dst.node(loose).parent(), None);

    let in_place = tree.copy(p, &Call::default()).unwrap();
    assert_eq!(
        dst.tostring(into, false, true),
        tree.tostring(in_place, false, true)
    );
    assert_eq!(
        dst.tostring(into, false, true),
        "<p id=\"x\" class=\"c\">a<b k=\"1\">b</b>c<i>d<u/>e</i></p>tail"
    );
    assert_eq!(dst.get(into, "id"), Some("x"));

    // The source is unchanged, and the copy does not share nodes with it.
    assert_eq!(tree.tostring(root, false, true), before);
    dst.strip_tags(into, &["b"]);
    assert_eq!(tree.tostring(root, false, true), before);
}
