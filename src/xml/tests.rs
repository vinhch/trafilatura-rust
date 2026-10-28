use std::sync::Arc;

use super::*;
use crate::cancel::{CHECK_INTERVAL, Call, CancelToken};
use crate::dom::{DumpMode, LocalName, MAX_DEPTH, NodeId, Ns, Tree, dump};
use crate::error::{ExtractError, Rejection};
use crate::html_processing::build_html_output;

/// `name="value"` pairs without whitespace in the values.
fn attributes(source: &str) -> Vec<(LocalName, Arc<str>)> {
    source
        .split_whitespace()
        .map(|pair| {
            let (key, value) = pair.split_once('=').unwrap();
            (LocalName::from(key), Arc::from(value.trim_matches('"')))
        })
        .collect()
}

/// Builds a tree from XML: `<a x="1">text<b/>tail</a>`; the first tag is the root. Attribute values
/// hold no whitespace.
fn xml(source: &str) -> Tree {
    let mut tree: Option<Tree> = None;
    let mut stack: Vec<NodeId> = Vec::new();
    for piece in source.split('<').skip(1) {
        let (tag, text) = piece.split_once('>').unwrap();
        let (name, closes, self_closing) = match (tag.strip_prefix('/'), tag.strip_suffix('/')) {
            (Some(name), _) => (name, true, false),
            (None, Some(name)) => (name, false, true),
            (None, None) => (tag, false, false),
        };
        let (name, attrs) = name.split_once(' ').unwrap_or((name, ""));
        let tree = tree
            .get_or_insert_with(|| Tree::new(LocalName::from(name), Ns::Html, attributes(attrs)));
        if closes {
            stack.pop();
        } else {
            let id = match stack.last() {
                Some(&parent) => {
                    tree.add_child(parent, LocalName::from(name), Ns::Html, attributes(attrs))
                }
                None => tree.root(),
            };
            if !self_closing {
                stack.push(id);
            }
        }
        if let Some(&top) = stack.last() {
            tree.add_text(top, text);
        }
    }
    tree.unwrap()
}

/// The element at `path`, child indexes from the root.
fn at(tree: &Tree, path: &[usize]) -> NodeId {
    path.iter()
        .fold(tree.root(), |id, &index| tree.child_at(id, index).unwrap())
}

/// lxml's `tostring` for attribute-free trees, without the root's tail.
fn show(tree: &Tree, id: NodeId) -> String {
    let node = tree.node(id);
    let name = &*node.name;
    let text = node.text.as_deref().unwrap_or("");
    if text.is_empty() && tree.first_child(id).is_none() {
        return format!("<{name}/>");
    }
    let children: String = tree
        .children(id)
        .map(|child| {
            let tail = tree.node(child).tail.as_deref().unwrap_or("");
            format!("{}{tail}", show(tree, child))
        })
        .collect();
    format!("<{name}>{text}{children}</{name}>")
}

fn shown(tree: &Tree) -> String {
    show(tree, tree.root())
}

fn emptied(source: &str) -> String {
    let mut tree = xml(source);
    let root = tree.root();
    remove_empty_elements(&mut tree, root, &Call::default()).unwrap();
    shown(&tree)
}

/// `(keeps_empty, separates_inline, is_padding_cell)` of the element at `path`.
fn flags(source: &str, path: &[usize]) -> (bool, bool, bool) {
    let tree = xml(source);
    let id = at(&tree, path);
    (
        keeps_empty(&tree, id),
        separates_inline(&tree, id),
        is_padding_cell(&tree, id),
    )
}

#[test]
fn delete_element_keeps_tail_on_previous() {
    let mut tree = xml("<body><a/>ta<b/>tb<c/>tc</body>");
    let target = at(&tree, &[1]);
    delete_element(&mut tree, target, true);
    // oracle: r = etree.fromstring("<body><a/>ta<b/>tb<c/>tc</body>"); delete_element(r[1]); etree.tostring(r, encoding="unicode")
    assert_eq!(shown(&tree), "<body><a/>tatb<c/>tc</body>");

    let mut tree = xml("<body><a>in</a><b/>tb</body>");
    let target = at(&tree, &[1]);
    delete_element(&mut tree, target, true);
    // oracle: r = etree.fromstring("<body><a>in</a><b/>tb</body>"); delete_element(r[1]); etree.tostring(r, encoding="unicode")
    assert_eq!(shown(&tree), "<body><a>in</a>tb</body>");
}

#[test]
fn delete_element_keeps_tail_on_parent_text() {
    let mut tree = xml("<body>t<b/>tb<c/>tc</body>");
    let target = at(&tree, &[0]);
    delete_element(&mut tree, target, true);
    // oracle: r = etree.fromstring("<body>t<b/>tb<c/>tc</body>"); delete_element(r[0]); etree.tostring(r, encoding="unicode")
    assert_eq!(shown(&tree), "<body>ttb<c/>tc</body>");

    let mut tree = xml("<body><a/><b/>tb</body>");
    let target = at(&tree, &[0]);
    delete_element(&mut tree, target, true);
    // oracle: r = etree.fromstring("<body><a/><b/>tb</body>"); delete_element(r[0]); etree.tostring(r, encoding="unicode")
    assert_eq!(shown(&tree), "<body><b/>tb</body>");
}

#[test]
fn delete_element_drops_tail_without_keep_tail() {
    let mut tree = xml("<body><a/>ta<b/>tb</body>");
    let target = at(&tree, &[1]);
    delete_element(&mut tree, target, false);
    // oracle: r = etree.fromstring("<body><a/>ta<b/>tb</body>"); delete_element(r[1], keep_tail=False); etree.tostring(r, encoding="unicode")
    assert_eq!(shown(&tree), "<body><a/>ta</body>");
}

#[test]
fn delete_element_without_tail_changes_no_text() {
    let mut tree = xml("<body><a/><b/></body>");
    let target = at(&tree, &[1]);
    delete_element(&mut tree, target, true);
    // oracle: r = etree.fromstring("<body><a/><b/></body>"); delete_element(r[1]); etree.tostring(r, encoding="unicode")
    assert_eq!(shown(&tree), "<body><a/></body>");
}

#[test]
fn delete_element_without_parent_is_noop() {
    let mut tree = xml("<a>x</a>");
    let root = tree.root();
    tree.node_mut(root).tail = Some("t".to_owned());
    delete_element(&mut tree, root, true);
    // oracle: r = etree.fromstring("<a>x</a>"); delete_element(r); etree.tostring(r, encoding="unicode")
    assert_eq!(shown(&tree), "<a>x</a>");
    assert_eq!(tree.node(root).tail.as_deref(), Some("t"));
}

#[test]
fn remove_empty_elements_bottom_up() {
    // oracle: remove_empty_elements(etree.fromstring("<body><p><hi> </hi></p><graphic/></body>"))
    assert_eq!(
        emptied("<body><p><hi> </hi></p><graphic/></body>"),
        "<body><graphic/></body>"
    );
}

#[test]
fn remove_empty_elements_keeps_elements_with_a_tail() {
    // oracle: remove_empty_elements(etree.fromstring("<body><p/><p>x</p><p/>tail<p/></body>"))
    assert_eq!(
        emptied("<body><p/><p>x</p><p/>tail<p/></body>"),
        "<body><p>x</p><p/>tail</body>"
    );
    // oracle: remove_empty_elements(etree.fromstring("<body><p> </p>  <p>x</p></body>"))
    assert_eq!(
        emptied("<body><p> </p>  <p>x</p></body>"),
        "<body><p>x</p></body>"
    );
}

#[test]
fn remove_empty_elements_keeps_what_keeps_empty_says() {
    // oracle: remove_empty_elements(etree.fromstring("<code><hi/>a<hi/></code>"))
    assert_eq!(
        emptied("<code><hi/>a<hi/></code>"),
        "<code><hi/>a<hi/></code>"
    );
    // oracle: remove_empty_elements(etree.fromstring("<p/>"))
    assert_eq!(emptied("<p/>"), "<p/>");
    // oracle: remove_empty_elements(etree.fromstring("<table><row><cell/><cell>v</cell></row><row><cell/><cell/></row></table>"))
    assert_eq!(
        emptied("<table><row><cell/><cell>v</cell></row><row><cell/><cell/></row></table>"),
        "<table><row><cell/><cell>v</cell></row></table>"
    );
    // oracle: remove_empty_elements(etree.fromstring("<body><p>a<lb/><hi>b</hi></p><p>c<lb/>d</p><p>e<lb/><p>f</p></p></body>"))
    assert_eq!(
        emptied("<body><p>a<lb/><hi>b</hi></p><p>c<lb/>d</p><p>e<lb/><p>f</p></p></body>"),
        "<body><p>a<lb/><hi>b</hi></p><p>c<lb/>d</p><p>e<p>f</p></p></body>"
    );
}

#[test]
fn remove_empty_elements_stops_when_cancelled() {
    let items = "<p>x</p>".repeat(CHECK_INTERVAL + 1);
    let mut tree = xml(&format!("<body>{items}</body>"));
    assert!(tree.len() > CHECK_INTERVAL);
    let token = CancelToken::new();
    token.cancel();
    let root = tree.root();
    assert_eq!(
        remove_empty_elements(&mut tree, root, &Call::default().cancel(&token)),
        Err(ExtractError::Cancelled)
    );
}

#[test]
fn keeps_empty_root_graphic_and_code_child() {
    // oracle: keeps_empty(etree.fromstring("<p/>"))
    assert_eq!(flags("<p/>", &[]), (true, false, false));
    // oracle: keeps_empty(etree.fromstring("<body><graphic/></body>")[0])
    assert_eq!(flags("<body><graphic/></body>", &[0]), (true, false, false));
    // oracle: keeps_empty(etree.fromstring("<body><code><hi/></code></body>")[0][0])
    assert_eq!(
        flags("<body><code><hi/></code></body>", &[0, 0]),
        (true, false, false)
    );
    // oracle: keeps_empty(etree.fromstring("<body><p/></body>")[0])
    assert_eq!(flags("<body><p/></body>", &[0]), (false, false, false));
}

#[test]
fn keeps_empty_padding_cell() {
    // oracle: r = etree.fromstring("<row><cell/><cell>v</cell></row>"); (keeps_empty(r[0]), is_padding_cell(r[0]))
    assert_eq!(
        flags("<row><cell/><cell>v</cell></row>", &[0]),
        (true, false, true)
    );
    // oracle: r = etree.fromstring("<row><cell/>tail</row>"); (keeps_empty(r[0]), is_padding_cell(r[0]))
    assert_eq!(flags("<row><cell/>tail</row>", &[0]), (true, false, true));
    // oracle: r = etree.fromstring("<row><cell/><cell/></row>"); (keeps_empty(r[0]), is_padding_cell(r[0]))
    assert_eq!(
        flags("<row><cell/><cell/></row>", &[0]),
        (false, false, false)
    );
}

#[test]
fn is_padding_cell_needs_a_cell_with_a_parent() {
    // oracle: r = etree.fromstring("<row><hi/><cell>v</cell></row>"); is_padding_cell(r[0])
    assert_eq!(
        flags("<row><hi/><cell>v</cell></row>", &[0]),
        (false, false, false)
    );
    // oracle: is_padding_cell(etree.fromstring("<cell>v</cell>"))
    assert_eq!(flags("<cell>v</cell>", &[]), (true, false, false));
}

#[test]
fn separates_inline_lb_before_ref() {
    // oracle: r = etree.fromstring("<body><lb/><ref/></body>"); separates_inline(r[0])
    assert_eq!(flags("<body><lb/><ref/></body>", &[0]), (true, true, false));
    // oracle: r = etree.fromstring("<body><lb/><graphic/></body>"); separates_inline(r[0])
    assert_eq!(
        flags("<body><lb/><graphic/></body>", &[0]),
        (true, true, false)
    );
}

#[test]
fn separates_inline_needs_an_lb_before_inline_content() {
    // oracle: r = etree.fromstring("<body><lb/><p/></body>"); separates_inline(r[0])
    assert_eq!(flags("<body><lb/><p/></body>", &[0]), (false, false, false));
    // oracle: r = etree.fromstring("<body><lb/></body>"); separates_inline(r[0])
    assert_eq!(flags("<body><lb/></body>", &[0]), (false, false, false));
    // oracle: r = etree.fromstring("<body><ref/><lb/></body>"); separates_inline(r[1])
    assert_eq!(
        flags("<body><ref/><lb/></body>", &[1]),
        (false, false, false)
    );
    // oracle: r = etree.fromstring("<body><lb/><hi/></body>"); separates_inline(r[0])
    assert_eq!(flags("<body><lb/><hi/></body>", &[0]), (true, true, false));
}

fn doubled_tags_stripped(source: &str) -> String {
    let mut tree = xml(source);
    let root = tree.root();
    assert_eq!(
        strip_double_tags(&mut tree, root, &Call::default()),
        Ok(root)
    );
    shown(&tree)
}

#[test]
fn strip_double_tags_matches_python() {
    // the walk goes on inside the subtree a merge just detached, and a merge keeps only the
    // element's text and tail
    // oracle: r = etree.fromstring("<doc><p>x<p>a<p>b</p>c</p>d</p><head>h<head>i</head></head></doc>"); strip_double_tags(r); etree.tostring(r, encoding="unicode")
    assert_eq!(
        doubled_tags_stripped("<doc><p>x<p>a<p>b</p>c</p>d</p><head>h<head>i</head></head></doc>"),
        "<doc><p>x a bcd</p><head>h i</head></doc>"
    );
    // oracle: r = etree.fromstring("<p>x<code>c<code>d<code>e</code></code></code></p>"); strip_double_tags(r); etree.tostring(r, encoding="unicode")
    assert_eq!(
        doubled_tags_stripped("<p>x<code>c<code>d<code>e</code></code></code></p>"),
        "<p>x<code>c d e</code></p>"
    );
    // oracle: r = etree.fromstring("<doc><p>x<p>a</p><p>b</p></p></doc>"); strip_double_tags(r); etree.tostring(r, encoding="unicode")
    assert_eq!(
        doubled_tags_stripped("<doc><p>x<p>a</p><p>b</p></p></doc>"),
        "<doc><p>x a b</p></doc>"
    );
    // the root is not one of its own descendants
    // oracle: r = etree.fromstring("<p>x<p>a</p><p>b</p></p>"); strip_double_tags(r); etree.tostring(r, encoding="unicode")
    assert_eq!(
        doubled_tags_stripped("<p>x<p>a</p><p>b</p></p>"),
        "<p>x<p>a</p><p>b</p></p>"
    );
}

#[test]
fn merge_with_parent_empty_parent_text() {
    let mut tree = xml("<p><x>z</x>t</p>");
    let root = tree.root();
    tree.node_mut(root).text = Some(String::new());
    let child = at(&tree, &[0]);
    merge_with_parent(&mut tree, child, false, &Call::default()).unwrap();
    // oracle: e = etree.fromstring("<p><x>z</x>t</p>"); e.text = ""; merge_with_parent(e[0]); (e.text, etree.tostring(e, encoding="unicode"))
    assert_eq!(tree.node(root).text.as_deref(), Some(" zt"));
    assert_eq!(shown(&tree), "<p> zt</p>");
}

#[test]
fn merge_with_parent_drops_children() {
    let mut tree = xml("<doc><p>x<p>a<hi>b</hi></p>t</p></doc>");
    let inner = at(&tree, &[0, 0]);
    merge_with_parent(&mut tree, inner, false, &Call::default()).unwrap();
    // oracle: r = etree.fromstring("<doc><p>x<p>a<hi>b</hi></p>t</p></doc>"); merge_with_parent(r[0][0]); etree.tostring(r, encoding="unicode")
    assert_eq!(shown(&tree), "<doc><p>x at</p></doc>");
}

#[test]
fn replace_element_text_code_removes_lb() {
    let mut tree = xml("<code>a<lb/>b<lb/>c</code>");
    let root = tree.root();
    let text = replace_element_text(&mut tree, root, true, false, &Call::default()).unwrap();
    // oracle: e = etree.fromstring("<code>a<lb/>b<lb/>c</code>"); (replace_element_text(e, True), etree.tostring(e, encoding="unicode"))
    assert_eq!(text, "```\na\nb\nc\n```\n");
    assert_eq!(shown(&tree), "<code>a</code>");
    assert!(tree.find(root, "lb").is_none());
}

#[test]
fn replace_element_text_cancels_inside_inline_walk() {
    let children = "<hi/>".repeat(1000);
    let mut tree = xml(&format!("<hi>{children}</hi>"));
    let root = tree.root();
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    // the first tick already checks at count 0, so the 1000 children do not tell a check per child
    // from one at entry; the zero-child `Ok` below shows that the check sits in the child loop
    assert_eq!(
        replace_element_text(&mut tree, root, false, false, &call),
        Err(ExtractError::Cancelled)
    );
    // no children, no walk, no check
    let mut tree = xml("<hi>x</hi>");
    let root = tree.root();
    assert_eq!(
        replace_element_text(&mut tree, root, false, false, &call),
        Ok("x".to_owned())
    );
}

#[test]
fn merge_with_parent_previous_sibling_tail() {
    let merged = |source: &str, previous_tail: Option<&str>| {
        let mut tree = xml(source);
        let previous = at(&tree, &[0]);
        if let Some(tail) = previous_tail {
            tree.node_mut(previous).tail = Some(tail.to_owned());
        }
        let element = at(&tree, &[1]);
        merge_with_parent(&mut tree, element, false, &Call::default()).unwrap();
        (tree.node(previous).tail.clone(), shown(&tree))
    };
    // a non-empty tail on the previous sibling joins the merged text with a space
    // oracle: r = etree.fromstring("<doc><a/>pt<p>c</p>t</doc>"); merge_with_parent(r[1]); (r[0].tail, etree.tostring(r, encoding="unicode"))
    assert_eq!(
        merged("<doc><a/>pt<p>c</p>t</doc>", None),
        (Some("pt ct".to_owned()), "<doc><a/>pt ct</doc>".to_owned())
    );
    // no tail: the merged text alone
    // oracle: r = etree.fromstring("<doc><a/><p>c</p>t</doc>"); merge_with_parent(r[1]); (r[0].tail, etree.tostring(r, encoding="unicode"))
    assert_eq!(
        merged("<doc><a/><p>c</p>t</doc>", None),
        (Some("ct".to_owned()), "<doc><a/>ct</doc>".to_owned())
    );
    // an empty tail is as good as none, without the joining space
    // oracle: r = etree.fromstring("<doc><a/><p>c</p>t</doc>"); r[0].tail = ""; merge_with_parent(r[1]); (r[0].tail, etree.tostring(r, encoding="unicode"))
    assert_eq!(
        merged("<doc><a/><p>c</p>t</doc>", Some("")),
        (Some("ct".to_owned()), "<doc><a/>ct</doc>".to_owned())
    );
}

#[test]
fn strip_double_tags_cancels() {
    let mut tree = xml("<doc><p>x<p>a</p></p></doc>");
    let root = tree.root();
    let token = CancelToken::new();
    token.cancel();
    assert_eq!(
        strip_double_tags(&mut tree, root, &Call::default().cancel(&token)),
        Err(ExtractError::Cancelled)
    );
}

/// `xml_to_text` over the whole tree, in the given mode.
fn text_of(tree: &mut Tree, include_formatting: bool) -> String {
    let root = tree.root();
    xml_to_text(tree, Some(root), include_formatting, &Call::default()).unwrap()
}

#[test]
fn empty_piece_reads_as_separator() {
    // `replace_element_text` of `<hi><foo/></hi>` is the empty string, which `_last_char` reads as
    // a separator: no newline is put before the next block
    let source = "<doc>text<hi><foo/></hi><p>b</p></doc>";
    // oracle: xml.xmltotxt(etree.fromstring("<doc>text<hi><foo/></hi><p>b</p></doc>"), False)
    assert_eq!(text_of(&mut xml(source), false), "textb");
    // oracle: xml.xmltotxt(etree.fromstring("<doc>text<hi><foo/></hi><p>b</p></doc>"), True)
    assert_eq!(text_of(&mut xml(source), true), "textb\n");
    // without the `hi`, the block starts on its own line
    let source = "<doc>text<p>b</p></doc>";
    // oracle: xml.xmltotxt(etree.fromstring("<doc>text<p>b</p></doc>"), False)
    assert_eq!(text_of(&mut xml(source), false), "text\nb");
    // oracle: xml.xmltotxt(etree.fromstring("<doc>text<p>b</p></doc>"), True)
    assert_eq!(text_of(&mut xml(source), true), "text\nb\n");
}

#[test]
fn unescape_runs_after_escaping_and_sanitize() {
    // (the text of the `p` after the XML parse, TXT output, Markdown output)
    let cases = [
        // entity text in the source comes out unescaped, and `sanitize` drops every U+2424
        // oracle: xml.xmltotxt(etree.fromstring("<p>a &amp;lt;b&amp;gt; &amp;amp; x␤y</p>"), fmt) for fmt in (False, True)
        ("a &lt;b&gt; &amp; x\u{2424}y", "a <b> & xy", "a <b> & xy\n"),
        // a reference that decodes to a line separator or to U+2424 survives: `unescape` runs last
        // oracle: xml.xmltotxt(etree.fromstring("<p>a&amp;#13;b&amp;#10;c&amp;#9252;d</p>"), fmt) for fmt in (False, True)
        (
            "a&#13;b&#10;c&#9252;d",
            "a\rb\nc\u{2424}d",
            "a\rb\nc\u{2424}d\n",
        ),
        // oracle: xml.xmltotxt(etree.fromstring("<p>a&amp;#8232;b</p>"), fmt) for fmt in (False, True)
        ("a&#8232;b", "a\u{2028}b", "a\u{2028}b\n"),
    ];
    for (text, plain, markdown) in cases {
        for (include_formatting, expected) in [(false, plain), (true, markdown)] {
            let mut tree = xml("<p/>");
            let root = tree.root();
            tree.node_mut(root).text = Some(text.to_owned());
            assert_eq!(
                text_of(&mut tree, include_formatting),
                expected,
                "{text:?}, formatting {include_formatting}"
            );
        }
    }
}

#[test]
fn list_marker_parent_and_no_list() {
    // the tail of a list in a `p` is indented by the width of the parent's marker (two spaces, not
    // the three of `2. `), and the numbering reads the `rend` of the parent whatever it is
    let source = "<doc><list rend=ol><item>x</item><item>a<p>b<list><item>c</item></list> tail</p></item></list></doc>";
    // oracle: xml.xmltotxt(etree.fromstring('<doc><list rend="ol"><item>x</item><item>a<p>b<list><item>c</item></list> tail</p></item></list></doc>'), False)
    assert_eq!(text_of(&mut xml(source), false), "- x\n- a b\n  - c\ntail");
    // oracle: xml.xmltotxt(etree.fromstring('<doc><list rend="ol"><item>x</item><item>a<p>b<list><item>c</item></list> tail</p></item></list></doc>'), True)
    assert_eq!(
        text_of(&mut xml(source), true),
        "1. x\n2. a b\n  - c\n\n  tail\n"
    );
    // a list directly in an ordered item: the indent is the width of `1. `, which is Markdown-only
    let source = "<doc><list rend=ol><item>a<list><item>c</item></list> tail</item></list></doc>";
    // oracle: xml.xmltotxt(etree.fromstring('<doc><list rend="ol"><item>a<list><item>c</item></list> tail</item></list></doc>'), False)
    assert_eq!(text_of(&mut xml(source), false), "- a\n  - c\ntail");
    // oracle: xml.xmltotxt(etree.fromstring('<doc><list rend="ol"><item>a<list><item>c</item></list> tail</item></list></doc>'), True)
    assert_eq!(text_of(&mut xml(source), true), "1. a\n  - c\n\n   tail\n");
    // an item with no `list` ancestor has no indent: `"  " * -1` is empty
    let source = "<doc><item>a</item><item>b</item></doc>";
    // oracle: xml.xmltotxt(etree.fromstring("<doc><item>a</item><item>b</item></doc>"), False)
    assert_eq!(text_of(&mut xml(source), false), "- a\n- b");
    // oracle: xml.xmltotxt(etree.fromstring("<doc><item>a</item><item>b</item></doc>"), True)
    assert_eq!(text_of(&mut xml(source), true), "- a\n- b");
}

// The numbering of an ordered list costs a constant per item: render time stays linear in the items.
#[test]
#[ignore = "slow"]
fn ordered_list_numbering_is_linear() {
    let time = |count: usize| {
        let source = format!(
            "<doc><list rend=ol>{}</list></doc>",
            "<item>w</item>".repeat(count)
        );
        let mut tree = xml(&source);
        let start = std::time::Instant::now();
        text_of(&mut tree, true);
        start.elapsed()
    };
    let base = time(50_000);
    let scaled = time(200_000);
    assert!(scaled < base * 8, "{base:?} for N, {scaled:?} for 4N");
}

#[test]
fn xml_to_text_raises_on_long_reference() {
    // a decimal reference of more than 4300 digits makes `html.unescape` raise, and nothing catches it
    for include_formatting in [false, true] {
        let mut tree = xml("<p/>");
        let root = tree.root();
        tree.node_mut(root).text = Some(format!("&#{};", "1".repeat(5000)));
        assert_eq!(
            xml_to_text(&mut tree, Some(root), include_formatting, &Call::default()),
            Err(ExtractError::Rejected(Rejection::InvalidValue)),
            "formatting {include_formatting}"
        );
    }
}

#[test]
fn xml_to_text_markdown_leaves_input_untouched() {
    let source = "<body><p>formula \\(x\\) <hi rend=#b><hi rend=#i>y</hi></hi><code>a<lb/>b</code> *z*</p></body>";
    let mut tree = xml(source);
    let root = tree.root();
    let before = dump(&tree, DumpMode::Exact);
    let text = xml_to_text(&mut tree, Some(root), true, &Call::default()).unwrap();
    assert!(text.contains("$x$") && text.contains("***y***") && text.contains("\\*z\\*"));
    assert_eq!(dump(&tree, DumpMode::Exact), before);
    xml_to_text(&mut tree, Some(root), false, &Call::default()).unwrap();
    assert_eq!(dump(&tree, DumpMode::Exact), before);
}

type Level = (
    &'static str,
    &'static [(&'static str, &'static str)],
    Option<&'static str>,
);

/// A chain of `levels` nested elements, each with the name, attributes and text `level` gives.
fn chain(levels: usize, level: impl Fn(usize) -> Level) -> Tree {
    let make_attrs = |attrs: &[(&str, &str)]| -> Vec<(LocalName, Arc<str>)> {
        attrs
            .iter()
            .map(|(key, value)| (LocalName::from(*key), Arc::from(*value)))
            .collect()
    };
    let (name, attrs, text) = level(0);
    let mut tree = Tree::new(LocalName::from(name), Ns::Html, make_attrs(attrs));
    let mut parent = tree.root();
    tree.node_mut(parent).text = text.map(str::to_owned);
    for index in 1..levels {
        let (name, attrs, text) = level(index);
        parent = tree.add_child(parent, LocalName::from(name), Ns::Html, make_attrs(attrs));
        tree.node_mut(parent).text = text.map(str::to_owned);
    }
    tree
}

/// `(length, "- " count, "x" count, "[" count, "]" count)` of the output.
type Counts = (usize, usize, usize, usize, usize);

/// `(length, "<ul>", "<li>", "<strong>", "<a " and "x" counts)` of the HTML output.
type HtmlCounts = (usize, usize, usize, usize, usize, usize);

/// A shape of the depth test: its name, its level at an index, the counts for both text modes and
/// the counts of the HTML output.
type Shape = (&'static str, fn(usize) -> Level, [Counts; 2], HtmlCounts);

fn html_counts(html: &str) -> HtmlCounts {
    (
        html.chars().count(),
        html.matches("<ul>").count(),
        html.matches("<li>").count(),
        html.matches("<strong>").count(),
        html.matches("<a ").count(),
        html.matches('x').count(),
    )
}

fn counts(text: &str) -> Counts {
    (
        text.chars().count(),
        text.matches("- ").count(),
        text.matches('x').count(),
        text.matches('[').count(),
        text.matches(']').count(),
    )
}

#[test]
fn output_deep_nesting_fits_512k_stack() {
    // one tree per recursive shape, past the depth the parser lets through, on the smallest stack a
    // host may give its threads, in a debug build. Measured by shrinking the stack: `hi chain` and
    // `ref in hi` overflow first, below about 390 KiB, `list > item` below about 350 KiB.
    let levels = MAX_DEPTH + 16;
    std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(move || {
            let shapes: [Shape; 3] = [
                (
                    "list > item",
                    |index| {
                        if index % 2 == 0 {
                            ("list", &[], None)
                        } else {
                            ("item", &[], Some("x"))
                        }
                    },
                    // oracle: for fmt in (False, True): t = xml.xmltotxt(nest(kinds), fmt); (len(t), t.count("- "), t.count("x"), t.count("["), t.count("]")), with nest building 272 nested SubElement levels: a list at the even ones, an item with text x at the odd ones
                    [(18903, 136, 136, 0, 0), (18904, 136, 136, 0, 0)],
                    // oracle: for the same nest(kinds): out = build_html_output(doc, False) with doc.body = nest(kinds); (len(out), out.count("<ul>"), out.count("<li>"), out.count("<strong>"), out.count("<a "), out.count("x"))
                    (2613, 135, 136, 0, 0, 136),
                ),
                (
                    "hi chain",
                    |_| ("hi", &[("rend", "#b")], Some("x")),
                    // oracle: the same counts for 272 nested hi rend="#b", each with text x
                    [(272, 0, 272, 0, 0), (1360, 0, 272, 0, 0)],
                    // oracle: the same html counts for the hi chain
                    (4909, 0, 0, 271, 0, 272),
                ),
                (
                    "ref in hi",
                    |index| {
                        if index % 2 == 0 {
                            ("hi", &[("rend", "#b")], Some("x"))
                        } else {
                            ("ref", &[("target", "u")], Some("x"))
                        }
                    },
                    // oracle: the same counts for 272 levels, hi rend="#b" at the even ones and ref target="u" at the odd ones, each with text x
                    [(1222, 0, 272, 136, 136), (1766, 0, 272, 136, 136)],
                    // oracle: the same html counts for ref in hi
                    (4773, 0, 0, 135, 136, 272),
                ),
            ];
            for (name, level, expected, expected_html) in shapes {
                for (include_formatting, expected) in [false, true].into_iter().zip(expected) {
                    let mut tree = chain(levels, level);
                    let text = text_of(&mut tree, include_formatting);
                    assert_eq!(
                        counts(&text),
                        expected,
                        "{name}, formatting {include_formatting}"
                    );
                }
                let mut tree = chain(levels, level);
                let root = tree.root();
                let html = build_html_output(&mut tree, root, None, &Call::default()).unwrap();
                assert_eq!(html_counts(&html), expected_html, "{name}, html");
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

// Merging a run of adjacent `hi` costs a constant per element: time stays linear in the run length.
#[test]
#[ignore = "slow"]
fn adjacent_hi_merge_is_linear() {
    let time = |count: usize| {
        let source = format!("<doc><p>{}</p></doc>", "<hi rend=#b>w</hi>".repeat(count));
        let mut tree = xml(&source);
        let root = tree.root();
        let start = std::time::Instant::now();
        merge_adjacent_hi(&mut tree, root, &Call::default(), &mut 0).unwrap();
        let elapsed = start.elapsed();
        let paragraph = tree.first_child(root).unwrap();
        let merged = tree.first_child(paragraph).unwrap();
        assert_eq!(tree.children(paragraph).count(), 1);
        assert_eq!(tree.node(merged).text, Some("w".repeat(count)));
        elapsed
    };
    let base = time(100_000);
    let scaled = time(400_000);
    assert!(scaled < base * 8, "{base:?} for N, {scaled:?} for 4N");
}

#[test]
fn merge_adjacent_hi_ticks_in_the_pair_loop() {
    // `CHECK_INTERVAL` is 256: the visits of the `p` and its two `hi` children tick at 253, 254 and
    // 255 without a check, so only the tick of the pair loop, at 256, sees the cancelled token
    assert_eq!(CHECK_INTERVAL, 256);
    let mut tree = xml("<p><hi rend=#b>a</hi><hi rend=#b>b</hi></p>");
    let root = tree.root();
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    let mut visited = 253;
    assert_eq!(
        merge_adjacent_hi(&mut tree, root, &call, &mut visited),
        Err(ExtractError::Cancelled)
    );
}
