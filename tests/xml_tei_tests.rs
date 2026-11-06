#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Ports of upstream `tests/xml_tei_tests.py`, case by case with the upstream names. The
//! `etree.fromstring` snippets are read by `xml_tree`; `(tag, text, tail)` tuples stand for lxml's
//! `.tag`, `.text` and `.tail`, and an upstream bare tag is `(tag, None)`.

#[allow(dead_code)]
mod common;
#[allow(dead_code)]
mod trees;

use std::collections::BTreeMap;

use trafilatura_rust::__internals::{
    HeaderFields, LocalName, NodeId, Ns, PKG_VERSION, Tree, check_tei, handle_unwanted_tails,
    move_element_one_level_up, remove_blank_text, replace_element_text,
    wrap_unwanted_siblings_of_div, write_fullheader,
};
use trafilatura_rust::Call;
use trees::xml_tree;

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

/// The first descendant of `root` named `tag`, as lxml's `find(".//tag")`.
fn find(tree: &Tree, root: NodeId, tag: &str) -> Option<NodeId> {
    tree.iterdescendants(root, &[tag]).next()
}

/// The child of `id` named `tag`.
fn child_named(tree: &Tree, id: NodeId, tag: &str) -> Option<NodeId> {
    tree.children(id)
        .find(|&child| tree.tag(child) == Some(tag))
}

/// The tags of `ids`, in order.
fn tags(tree: &Tree, ids: impl Iterator<Item = NodeId>) -> Vec<&str> {
    ids.filter_map(|id| tree.tag(id)).collect()
}

/// The `(tag, text)` pairs of `ids`.
fn tag_text(tree: &Tree, ids: impl Iterator<Item = NodeId>) -> Vec<(&str, Option<&str>)> {
    ids.filter_map(|id| Some((tree.tag(id)?, tree.node(id).text.as_deref())))
        .collect()
}

/// The `(tag, text, tail)` triples of `ids`.
fn tag_text_tail(
    tree: &Tree,
    ids: impl Iterator<Item = NodeId>,
) -> Vec<(&str, Option<&str>, Option<&str>)> {
    ids.filter_map(|id| {
        let node = tree.node(id);
        Some((tree.tag(id)?, node.text.as_deref(), node.tail.as_deref()))
    })
    .collect()
}

/// The attributes of `id` as a map, the way a Python dict compares.
fn attrib(tree: &Tree, id: NodeId) -> BTreeMap<&str, &str> {
    tree.node(id)
        .attrs
        .iter()
        .map(|(key, value)| (&**key, &**value))
        .collect()
}

/// `tostring(id, encoding="unicode")` with the tail.
fn show(tree: &Tree, id: NodeId) -> String {
    tree.tostring(id, false, true)
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

#[test]
fn test_sanity() {
    let call = Call::default();
    let (mut tree, node) = element("p", "test", &[]);
    handle_unwanted_tails(&mut tree, node);
    assert_eq!(tree.node(node).text.as_deref(), Some("test"));
    tree.node_mut(node).tail = Some("tail".to_owned());
    handle_unwanted_tails(&mut tree, node);
    assert_eq!(tree.node(node).text.as_deref(), Some("test tail"));

    let mut div_tree = Tree::new(LocalName::from("div"), Ns::Html, Vec::new());
    let div = div_tree.root();
    let p = div_tree.add_child(div, LocalName::from("p"), Ns::Html, Vec::new());
    div_tree.node_mut(p).text = Some("test".to_owned());
    div_tree.node_mut(p).tail = Some("tail".to_owned());
    handle_unwanted_tails(&mut div_tree, p);
    assert_eq!(show(&div_tree, div), "<div><p>test tail</p></div>");

    let mut new_tree = Tree::new(LocalName::from("div"), Ns::Html, Vec::new());
    let new_elem = new_tree.root();
    wrap_unwanted_siblings_of_div(&mut new_tree, new_elem, &call).unwrap();
    let p_elem = new_tree.add_child(new_elem, LocalName::from("p"), Ns::Html, Vec::new());
    move_element_one_level_up(&mut new_tree, p_elem, &call).unwrap();
    assert_eq!(show(&new_tree, new_elem), "<div><p/></div>");

    let (mut head_tree, head) = element("head", "", &[]);
    let result = check_tei(&mut head_tree, head, Some(""), &call);
    assert_eq!(result, Ok(head));
}

#[test]
fn test_wrap_unwanted_siblings_of_div_runs() {
    // Each run of block siblings after a div gets its own wrapper, up to the next div.
    let mut tree = xml_tree("<body><div/><p>a</p><p>b</p><lb/><p>c</p><div/><p>d</p></body>");
    let body = tree.root();
    let div = tree.child_at(body, 0).unwrap();
    wrap_unwanted_siblings_of_div(&mut tree, div, &Call::default()).unwrap();
    assert_eq!(
        show(&tree, body),
        "<body><div/><div><p>a</p><p>b</p></div><lb/><div><p>c</p></div><div/><p>d</p></body>"
    );
}

/// The `publicationStmt` that `write_fullheader` writes for `fields`, with its tree.
fn publication_stmt(fields: &HeaderFields<'_>) -> (Tree, NodeId) {
    let mut tree = Tree::new(LocalName::from("TEI"), Ns::Html, Vec::new());
    let teidoc = tree.root();
    tree.set(teidoc, "xmlns", "http://www.tei-c.org/ns/1.0");
    let header = write_fullheader(&mut tree, teidoc, fields);
    let file_desc = child_named(&tree, header, "fileDesc").unwrap();
    let stmt = child_named(&tree, file_desc, "publicationStmt").unwrap();
    (tree, stmt)
}

#[test]
// the upstream name carries the TEI element name `publicationStmt`
#[allow(non_snake_case)]
fn test_publisher_added_before_availability_in_publicationStmt() {
    let cats = vec!["cat".to_owned()];
    let license = Some("CC BY-SA 4.0");
    let categories = Some(cats.as_slice());

    // add publisher string
    let (tree, stmt) = publication_stmt(&HeaderFields {
        sitename: Some("The Publisher"),
        license,
        categories,
        tags: categories,
        ..HeaderFields::default()
    });
    assert_eq!(child_names(&tree, stmt), ["publisher", "availability"]);
    assert_eq!(
        text_of(&tree, child_named(&tree, stmt, "publisher")),
        Some("The Publisher")
    );

    let (tree, stmt) = publication_stmt(&HeaderFields {
        hostname: Some("example.org"),
        license,
        categories,
        tags: categories,
        ..HeaderFields::default()
    });
    assert_eq!(child_names(&tree, stmt), ["publisher", "availability"]);
    assert_eq!(
        text_of(&tree, child_named(&tree, stmt, "publisher")),
        Some("example.org")
    );

    let (tree, stmt) = publication_stmt(&HeaderFields {
        hostname: Some("example.org"),
        sitename: Some("Example"),
        license,
        categories,
        tags: categories,
        ..HeaderFields::default()
    });
    assert_eq!(child_names(&tree, stmt), ["publisher", "availability"]);
    assert_eq!(
        text_of(&tree, child_named(&tree, stmt, "publisher")),
        Some("Example (example.org)")
    );

    // no publisher, add "N/A"
    let (tree, stmt) = publication_stmt(&HeaderFields {
        license,
        categories,
        tags: categories,
        ..HeaderFields::default()
    });
    assert_eq!(child_names(&tree, stmt), ["publisher", "availability"]);
    assert_eq!(
        text_of(&tree, child_named(&tree, stmt, "publisher")),
        Some("N/A")
    );

    // no license, add nothing
    let (tree, stmt) = publication_stmt(&HeaderFields {
        categories,
        tags: categories,
        ..HeaderFields::default()
    });
    assert_eq!(child_names(&tree, stmt), ["p"]);
}

/// The tags of the element children of `id`.
fn child_names(tree: &Tree, id: NodeId) -> Vec<&str> {
    tags(tree, tree.children(id))
}

/// The text of the element `id`, `None` when it has no element or no text.
fn text_of(tree: &Tree, id: Option<NodeId>) -> Option<&str> {
    tree.node(id?).text.as_deref()
}

#[test]
fn test_unwanted_siblings_of_div_removed() {
    let call = Call::default();
    let check = |source: &str| {
        let mut tree = xml_tree(source);
        let root = tree.root();
        check_tei(&mut tree, root, Some("fake_url"), &call).unwrap();
        tree
    };

    let tree = check(
        "<TEI><text><body><div><div><p>text1</p></div><p>text2</p></div></body></text></TEI>",
    );
    let div = find(&tree, tree.root(), "div").unwrap();
    assert_eq!(
        tags(&tree, tree.iter(div, &[])),
        ["div", "div", "p", "div", "p"]
    );
    let body = find(&tree, tree.root(), "body").unwrap();
    assert_eq!(
        show(&tree, body),
        "<body><div><div><p>text1</p></div><div><p>text2</p></div></div></body>"
    );

    let tree = check(
        "<TEI><text><body><div><div/><list><item>text</item></list></div></body></text></TEI>",
    );
    let div = find(&tree, tree.root(), "div").unwrap();
    assert_eq!(
        tags(&tree, tree.iter(div, &[])),
        ["div", "div", "div", "list", "item"]
    );

    let tree = check(
        "<TEI><text><body><div><div/><table><row><cell>text</cell></row></table></div></body></text></TEI>",
    );
    let div = find(&tree, tree.root(), "div").unwrap();
    assert_eq!(
        tags(&tree, tree.iter(div, &[])),
        ["div", "div", "div", "table", "row", "cell"]
    );

    let tree = check(
        "<TEI><text><body><div><p>text1</p><div/><div/><p>text2</p></div></body></text></TEI>",
    );
    let body = find(&tree, tree.root(), "body").unwrap();
    assert_eq!(
        show(&tree, body),
        "<body><div><p>text1</p><div/><div/><div><p>text2</p></div></div></body>"
    );

    let tree = check(
        "<TEI><text><body><div><div><p>text1</p></div><p>text2</p><p>text3</p></div></body></text></TEI>",
    );
    let div = find(&tree, tree.root(), "div").unwrap();
    assert_eq!(
        tags(&tree, tree.iter(div, &[])),
        ["div", "div", "p", "div", "p", "p"]
    );

    let tree = check(
        "<TEI><text><body><div><p>text1</p><div/><p>text2</p><div/><p>text3</p></div></body></text></TEI>",
    );
    let div = find(&tree, tree.root(), "div").unwrap();
    assert_eq!(
        tags(&tree, tree.iter(div, &[])),
        ["div", "p", "div", "div", "p", "div", "div", "p"]
    );

    let tree = check(
        "<TEI><text><body><div><p>text1</p><div/><p>text2</p><div/><list/></div></body></text></TEI>",
    );
    let div = find(&tree, tree.root(), "div").unwrap();
    assert_eq!(
        tags(&tree, tree.iter(div, &[])),
        ["div", "p", "div", "div", "p", "div", "div", "list"]
    );

    let tree = check(
        "<TEI><text><body><div><p/><div/><p>text1</p><ab/><p>text2</p></div></body></text></TEI>",
    );
    let body = find(&tree, tree.root(), "body").unwrap();
    assert_eq!(
        show(&tree, body),
        "<body><div><p/><div/><div><p>text1</p><ab/><p>text2</p></div></div></body>"
    );

    let tree = check("<TEI><text><body><div><div/><ab/></div></body></text></TEI>");
    let body = find(&tree, tree.root(), "body").unwrap();
    assert_eq!(
        show(&tree, body),
        "<body><div><div/><div><ab/></div></div></body>"
    );

    let tree = check("<TEI><text><body><div><div/><quote>text</quote></div></body></text></TEI>");
    let div = find(&tree, tree.root(), "div").unwrap();
    assert_eq!(
        tags(&tree, tree.iter(div, &[])),
        ["div", "div", "div", "quote"]
    );

    let tree = check("<TEI><text><body><div><div/><lb/></div></body></text></TEI>");
    let div = find(&tree, tree.root(), "div").unwrap();
    assert_eq!(tags(&tree, tree.iter(div, &[])), ["div", "div", "lb"]);

    let tree = check("<TEI><text><body><div><div/><ab/></div></body></text></TEI>");
    let div = find(&tree, tree.root(), "div").unwrap();
    assert_eq!(
        tags(&tree, tree.iter(div, &[])),
        ["div", "div", "div", "ab"]
    );

    let tree = check(
        "<TEI><text><body><div><div><p>text1</p></div><list/><ul><li>text2</li></ul></div></body></text></TEI>",
    );
    let body = find(&tree, tree.root(), "body").unwrap();
    assert_eq!(
        show(&tree, body),
        "<body><div><div><p>text1</p></div><div><list/></div></div></body>"
    );

    let tree = check(
        "<TEI><text><body><div><div><p>text1</p></div><ul><li>text2</li></ul><list/></div></body></text></TEI>",
    );
    let body = find(&tree, tree.root(), "body").unwrap();
    assert_eq!(
        show(&tree, body),
        "<body><div><div><p>text1</p></div><div><list/></div></div></body>"
    );

    let xml_str = "<TEI><text><body><div><p/><div/></div></body></text></TEI>";
    let tree = check(xml_str);
    assert_eq!(show(&tree, tree.root()), xml_str);

    let tree = check("<TEI><text><body><div><div/><lb/>tail</div></body></text></TEI>");
    let div = find(&tree, tree.root(), "div").unwrap();
    assert_eq!(tags(&tree, tree.iter(div, &[])), ["div", "div", "div", "p"]);
}

#[test]
fn test_tail_on_p_like_elements_removed() {
    let call = Call::default();
    let check = |source: &str| {
        let mut tree = xml_tree(source);
        let root = tree.root();
        check_tei(&mut tree, root, Some("fake_url"), &call).unwrap();
        tree
    };

    let tree = check(
        "
    <TEI><text><body>
      <div>
        <p>text</p>former link
        <p>more text</p>former span
        <p>even more text</p>another span
      </div>
    </body></text></TEI>",
    );
    let paragraphs: Vec<_> = tree
        .iter(tree.root(), &["p"])
        .map(|id| (tree.node(id).text.as_deref(), tree.node(id).tail.as_deref()))
        .collect();
    assert_eq!(
        paragraphs,
        [
            (Some("text former link"), None),
            (Some("more text former span"), None),
            (Some("even more text another span"), None),
        ]
    );

    let tree = check(
        "<TEI><text><body><div><head>title</head>some text<p>article</p></div></body></text></TEI>",
    );
    let div = find(&tree, tree.root(), "div").unwrap();
    assert_eq!(
        tag_text_tail(&tree, tree.iterdescendants(div, &[])),
        [
            ("ab", Some("title"), None),
            ("p", Some("some text"), None),
            ("p", Some("article"), None),
        ]
    );

    let tree =
        check("<TEI><text><body><div><ab>title</ab>tail<p>more text</p></div></body></text></TEI>");
    let div = find(&tree, tree.root(), "div").unwrap();
    assert_eq!(
        tag_text_tail(&tree, tree.iterdescendants(div, &[])),
        [
            ("ab", Some("title"), None),
            ("p", Some("tail"), None),
            ("p", Some("more text"), None),
        ]
    );

    let tree = check("<TEI><text><body><div><p>text</p><lb/>tail</div></body></text></TEI>");
    let div = find(&tree, tree.root(), "div").unwrap();
    assert_eq!(
        tag_text_tail(&tree, tree.iterdescendants(div, &[])),
        [("p", Some("text"), None), ("p", Some("tail"), None)]
    );

    let tree = check("<TEI><text><body><div><p/>tail</div></body></text></TEI>");
    let p = find(&tree, tree.root(), "p").unwrap();
    assert_eq!(
        tag_text_tail(&tree, tree.iter(p, &[])),
        [("p", Some("tail"), None)]
    );

    let tree =
        check("<TEI><text><body><div><p/>tail1<ab/>tail2<ab/>tail3</div></body></text></TEI>");
    let texts: Vec<&str> = tree
        .iter(tree.root(), &[])
        .filter_map(|id| tree.node(id).text.as_deref())
        .filter(|text| !text.is_empty())
        .collect();
    assert_eq!(texts, ["tail1", "tail2", "tail3"]);
}

#[test]
fn test_head_with_children_converted_to_ab() {
    let call = Call::default();
    let check = |source: &str| {
        let mut tree = xml_tree(source);
        let root = tree.root();
        check_tei(&mut tree, root, Some("fake_url"), &call).unwrap();
        tree
    };

    let tree = check("<text><head>heading</head><p>some text</p></text>");
    assert_eq!(
        tag_text(&tree, tree.iter(tree.root(), &[])),
        [
            ("text", None),
            ("ab", Some("heading")),
            ("p", Some("some text"))
        ]
    );

    let tree = check("<text><head><p>text</p></head></text>");
    assert_eq!(
        tag_text_tail(&tree, tree.iter(tree.root(), &[])),
        [("text", None, None), ("ab", Some("text"), None)]
    );

    let tree = check("<text><head><p>first</p><p>second</p><p>third</p></head></text>");
    assert_eq!(
        tag_text_tail(&tree, tree.iter(tree.root(), &[])),
        [
            ("text", None, None),
            ("ab", Some("first"), None),
            ("lb", None, Some("second")),
            ("lb", None, Some("third")),
        ]
    );

    let tree = check(
        "<text><head><p>first</p><list><item>text</item></list><p>second</p><p>third</p></head></text>",
    );
    assert_eq!(
        tag_text_tail(&tree, tree.iter(tree.root(), &[])),
        [
            ("text", None, None),
            ("ab", Some("first"), None),
            ("list", None, Some("second")),
            ("item", Some("text"), None),
            ("lb", None, Some("third")),
        ]
    );

    let tree = check("<text><head><list><item>text1</item></list><p>text2</p></head></text>");
    assert_eq!(
        tag_text_tail(&tree, tree.iter(tree.root(), &[])),
        [
            ("text", None, None),
            ("ab", None, None),
            ("list", None, Some("text2")),
            ("item", Some("text1"), None),
        ]
    );

    let tree = check("<text><head>heading</head><p>some text</p></text>");
    assert_eq!(
        attrib(&tree, tree.child_at(tree.root(), 0).unwrap()),
        BTreeMap::from([("type", "header")])
    );

    let tree = check("<text><head rend='h3'>heading</head><p>some text</p></text>");
    assert_eq!(
        attrib(&tree, tree.child_at(tree.root(), 0).unwrap()),
        BTreeMap::from([("type", "header"), ("rend", "h3")])
    );

    let tree = check("<TEI><teiheader/><text><body><head><p>text</p></head></body></text></TEI>");
    let ab = find(&tree, tree.root(), "ab").unwrap();
    assert_eq!(tree.node(ab).text.as_deref(), Some("text"));
    assert_eq!(attrib(&tree, ab), BTreeMap::from([("type", "header")]));

    let tree = check("<text><body><head>text1<p>text2</p></head></body></text>");
    let ab = find(&tree, tree.root(), "ab").unwrap();
    assert_eq!(
        tag_text_tail(&tree, tree.iter(ab, &[])),
        [("ab", Some("text1"), None), ("lb", None, Some("text2"))]
    );

    let tree = check(
        "<text>
            <body>
                <head>text1
                    <p>text2</p>
                </head>
            </body>
        </text>
    ",
    );
    let ab = find(&tree, tree.root(), "ab").unwrap();
    assert_eq!(
        tag_text_tail(&tree, tree.iter(ab, &[])),
        [("ab", Some("text1"), None), ("lb", None, Some("text2"))]
    );

    let tree = check(
        "<TEI><text><body><head><list><item>text1</item></list><p>text2</p></head>tail</body></text></TEI>",
    );
    let body = find(&tree, tree.root(), "body").unwrap();
    assert_eq!(
        tag_text_tail(&tree, tree.iter(body, &[])),
        [
            ("body", None, None),
            ("ab", None, None),
            ("list", None, Some("text2")),
            ("item", Some("text1"), None),
            ("p", Some("tail"), None),
        ]
    );
}

#[test]
fn test_ab_with_p_parent_resolved() {
    let call = Call::default();
    let check = |source: &str| {
        let mut tree = xml_tree(source);
        let root = tree.root();
        check_tei(&mut tree, root, Some("fake_url"), &call).unwrap();
        tree
    };
    // the parser of the last case strips the blank text of the pretty-printed input
    let check_blank_stripped = |source: &str| {
        let mut tree = xml_tree(source);
        let root = tree.root();
        remove_blank_text(&mut tree, root, &call).unwrap();
        check_tei(&mut tree, root, Some("fake_url"), &call).unwrap();
        tree
    };

    let tree = check("<text><p><head>text1</head></p></text>");
    assert!(find(&tree, tree.root(), "ab").is_some());
    assert!(find(&tree, tree.root(), "p").is_none());

    let tree = check("<body><p>text1<head>text2</head></p></body>");
    let ab = find(&tree, tree.root(), "ab").unwrap();
    assert_eq!(tree.tag(tree.node(ab).parent().unwrap()), Some("body"));
    assert_eq!(tree.node(ab).text.as_deref(), Some("text2"));

    let tree = check("<TEI><text><body><p><head>text1</head></p>text2</body></text></TEI>");
    assert_eq!(
        text_of(&tree, find(&tree, tree.root(), "ab")),
        Some("text1")
    );
    assert_eq!(text_of(&tree, find(&tree, tree.root(), "p")), Some("text2"));

    let tree = check("<text><p><head rend='h3'>text</head></p></text>");
    let ab = child_named(&tree, tree.root(), "ab").unwrap();
    assert_eq!(
        attrib(&tree, ab),
        BTreeMap::from([("rend", "h3"), ("type", "header")])
    );

    let tree = check("<text><p><head>text1</head><list/><head>text2</head></p></text>");
    assert_eq!(
        tag_text_tail(&tree, tree.iter(tree.root(), &[])),
        [
            ("text", None, None),
            ("ab", Some("text1"), None),
            ("p", None, None),
            ("list", None, None),
            ("ab", Some("text2"), None),
        ]
    );

    let tree = check("<TEI><text><body><p><head>text1</head>text2</p></body></text></TEI>");
    assert_eq!(text_of(&tree, find(&tree, tree.root(), "p")), Some("text2"));

    let tree = check("<TEI><text><body><p>text0<head>text1</head>text2</p></body></text></TEI>");
    let ab = find(&tree, tree.root(), "ab").unwrap();
    assert_eq!(text_of(&tree, tree.node(ab).next_sibling()), Some("text2"));

    let tree = check(
        "<TEI><text><body><p>text0<list/><head>text1</head>text2</p>text3</body></text></TEI>",
    );
    assert!(show(&tree, tree.root()).contains("text2"));
    assert!(find_child_of(&tree, "p", "list").is_some());

    let source = "
    <TEI>
      <text><body>
        <p>text1<head>text2</head>text3
          <head>text4</head>text5
        </p>
      </body></text>
    </TEI>";
    let tree = check(source);
    assert_eq!(
        stripped_tails(&tree, &["p", "ab"]),
        [
            ("p", Some("text1"), Some("")),
            ("ab", Some("text2"), None),
            ("p", Some("text3"), None),
            ("ab", Some("text4"), None),
            ("p", Some("text5"), None),
        ]
    );

    let source = "
    <TEI>
      <text>
        <body>
          <p>text0<head>text1</head>
            <list>
              <item>text2</item>
            </list>
            <head>text3</head>text4
          </p>
        </body>
      </text>
    </TEI>";
    let tree = check(source);
    assert_eq!(
        stripped_tails(&tree, &["p", "ab"]),
        [
            ("p", Some("text0"), Some("")),
            ("ab", Some("text1"), Some("")),
            ("p", None, None),
            ("ab", Some("text3"), None),
            ("p", Some("text4"), None),
        ]
    );

    let tree = check("<TEI><text><body><p>text0<head>text1</head></p>text2</body></text></TEI>");
    assert_eq!(
        tag_text_tail(&tree, tree.iter(tree.root(), &["p", "ab"])),
        [
            ("p", Some("text0"), None),
            ("ab", Some("text1"), None),
            ("p", Some("text2"), None),
        ]
    );

    let tree = check(
        "<TEI><text><body><p>text1<head>text2</head>text3<head>text4</head></p></body></text></TEI>",
    );
    assert_eq!(
        tag_text_tail(&tree, tree.iter(tree.root(), &["p", "ab"])),
        [
            ("p", Some("text1"), None),
            ("ab", Some("text2"), None),
            ("p", Some("text3"), None),
            ("ab", Some("text4"), None),
        ]
    );

    let tree = check("<text><head>text1</head><p>text2<head>text3</head></p></text>");
    assert_eq!(
        tag_text_tail(&tree, tree.children(tree.root())),
        [
            ("ab", Some("text1"), None),
            ("p", Some("text2"), None),
            ("ab", Some("text3"), None),
        ]
    );

    let tree = check("<text><p>text1<head>text2</head></p><head>text3</head></text>");
    assert_eq!(
        tag_text_tail(&tree, tree.children(tree.root())),
        [
            ("p", Some("text1"), None),
            ("ab", Some("text2"), None),
            ("ab", Some("text3"), None),
        ]
    );

    let tree = check_blank_stripped(
        "
        <text>
            <head>text1<p>text2</p></head>
            <p>text3<head>text4</head></p>
        </text>",
    );
    assert_eq!(
        tag_text_tail(&tree, tree.iter(tree.root(), &[])),
        [
            ("text", None, None),
            ("ab", Some("text1"), None),
            ("lb", None, Some("text2")),
            ("p", Some("text3"), None),
            ("ab", Some("text4"), None),
        ]
    );

    let tree = check_blank_stripped(
        "
        <TEI>
        <text>
          <body>
            <p>text1<head>
                    <list>
                        <item>text2</item>
                    </list>
                </head>
            text3
            </p>
          </body>
        </text>
        </TEI>
        ",
    );
    assert_eq!(
        tag_text_tail(&tree, tree.iter(tree.root(), &["p", "ab", "item"])),
        [
            ("p", Some("text1"), None),
            ("ab", None, None),
            ("item", Some("text2"), None),
            ("p", Some("text3"), None),
        ]
    );
}

/// The first `child` element under a `parent` element, anywhere in the tree.
fn find_child_of(tree: &Tree, parent: &str, child: &str) -> Option<NodeId> {
    tree.iter(tree.root(), &[parent])
        .find_map(|id| child_named(tree, id, child))
}

/// `(tag, text, tail.strip())` for the `tags` elements of the tree, with lxml's `None` for no tail.
fn stripped_tails<'t>(
    tree: &'t Tree,
    tags: &[&str],
) -> Vec<(&'t str, Option<&'t str>, Option<&'t str>)> {
    tree.iter(tree.root(), tags)
        .map(|id| {
            let node = tree.node(id);
            (
                tree.tag(id).unwrap(),
                node.text.as_deref(),
                node.tail.as_deref().map(str::trim),
            )
        })
        .collect()
}

#[test]
fn test_handling_of_text_content_in_div() {
    let call = Call::default();
    let check = |source: &str| {
        let mut tree = xml_tree(source);
        let root = tree.root();
        check_tei(&mut tree, root, Some("fake_url"), &call).unwrap();
        tree
    };

    let tree = check("<TEI><text><body><div>text<head/></div></body></text></TEI>");
    assert_eq!(text_of(&tree, find(&tree, tree.root(), "p")), Some("text"));

    let tree = check("<TEI><text><body><div>text1<p>text2</p></div></body></text></TEI>");
    assert_eq!(
        text_of(&tree, find(&tree, tree.root(), "p")),
        Some("text1 text2")
    );

    let tree = check("<TEI><text><body><div>text<p/></div></body></text></TEI>");
    assert_eq!(text_of(&tree, find(&tree, tree.root(), "p")), Some("text"));

    let tree = check("<TEI><text><body><div><p/></div>tail</body></text></TEI>");
    assert_eq!(text_of(&tree, find(&tree, tree.root(), "p")), Some("tail"));
}

#[test]
fn test_no_markdown_marker_in_merged_item_content() {
    // Merging an invalid element into its list item must not inject a markdown list marker into the TEI tree.
    let mut tree = xml_tree(
        "<TEI><text><body><div><list><item><foo>bar</foo></item></list></div></body></text></TEI>",
    );
    let root = tree.root();
    check_tei(&mut tree, root, None, &Call::default()).unwrap();
    assert!(show(&tree, root).ends_with("<list><item>bar</item></list></div></body></text></TEI>"));
}

#[test]
fn pkg_version_matches_manifest() {
    let manifest =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    assert_eq!(
        Some(PKG_VERSION.to_owned()),
        common::upstream_version(&manifest)
    );
}
