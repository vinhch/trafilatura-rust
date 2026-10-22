use super::*;
use crate::cancel::{CHECK_INTERVAL, Call, CancelToken};
use crate::dom::{LocalName, NodeId, Ns, Tree};
use crate::error::ExtractError;

/// Builds a tree from attribute-free XML: `<a>text<b/>tail</a>`; the first tag is the root.
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
        let tree = tree.get_or_insert_with(|| Tree::new(LocalName::from(name), Ns::Html, vec![]));
        if closes {
            stack.pop();
        } else {
            let id = match stack.last() {
                Some(&parent) => tree.add_child(parent, LocalName::from(name), Ns::Html, vec![]),
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
