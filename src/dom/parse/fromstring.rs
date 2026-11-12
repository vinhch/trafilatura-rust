//! lxml's `fromstring` and `fragment_fromstring(create_parent="div")` over the libxml2 tree builder: the
//! root each one returns, picked from the document as lxml picks it.

use std::borrow::Cow;

use super::ParseLimits;
use super::builder::build_document;
use crate::cancel::Call;
use crate::dom::{NodeId, Ns, Tree};
use crate::error::{ExtractError, Rejection};

/// `lxml.html.defs.block_tags`: the names `_contains_block_level_tag` looks for.
// upstream: lxml/html/defs.py::block_tags
const BLOCK_TAGS: &[&str] = &[
    "address",
    "blockquote",
    "caption",
    "center",
    "col",
    "colgroup",
    "dd",
    "del",
    "dir",
    "div",
    "dl",
    "dt",
    "fieldset",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "hr",
    "ins",
    "isindex",
    "legend",
    "li",
    "menu",
    "noscript",
    "ol",
    "optgroup",
    "option",
    "p",
    "pre",
    "table",
    "tbody",
    "td",
    "tfoot",
    "th",
    "thead",
    "tr",
    "ul",
];

/// Python's `str.isspace`, which `\s` and `strip` use: `char::is_whitespace` and also U+001C to U+001F.
fn is_py_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1C}'..='\u{1F}').contains(&c)
}

/// `not text or not text.strip()`, for an optional text.
fn is_blank(text: Option<&str>) -> bool {
    text.is_none_or(|text| text.trim_matches(is_py_space).is_empty())
}

/// `_looks_like_full_html_unicode`: `^\s*<(?:html|!doctype)`, case-insensitive.
// upstream: lxml/html/__init__.py::_looks_like_full_html_unicode
fn looks_like_full_html(html: &str) -> bool {
    let Some(rest) = html.trim_start_matches(is_py_space).strip_prefix('<') else {
        return false;
    };
    starts_with_ignore_ascii_case(rest, "html") || starts_with_ignore_ascii_case(rest, "!doctype")
}

fn starts_with_ignore_ascii_case(text: &str, prefix: &str) -> bool {
    text.get(..prefix.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
}

/// `text += more` for an optional text, when `more` is not empty (lxml's `(x or '') + more`).
fn append_text(slot: &mut Option<String>, more: &str) {
    if !more.is_empty() {
        slot.get_or_insert_with(String::new).push_str(more);
    }
}

/// `parent.findall(name)` over HTML elements: the children named `name`, in order.
fn children_named(tree: &Tree, parent: NodeId, name: &str) -> Vec<NodeId> {
    tree.children(parent)
        .filter(|&child| {
            let node = tree.node(child);
            node.ns == Ns::Html && &*node.name == name
        })
        .collect()
}

/// `drop_tree`: removes `id` with its children and text. Its tail joins the tail of the previous
/// element, or the text of the parent when it is the first child.
// upstream: lxml/etree.pyx::_Element.drop_tree
fn drop_tree(tree: &mut Tree, id: NodeId) {
    let Some(parent) = tree.node(id).parent() else {
        return;
    };
    if let Some(tail) = tree.node_mut(id).tail.take() {
        match tree.node(id).prev_sibling() {
            Some(previous) => append_text(&mut tree.node_mut(previous).tail, &tail),
            None => append_text(&mut tree.node_mut(parent).text, &tail),
        }
    }
    tree.remove(id);
}

/// Moves every child of `from` to the end of `to`, tails included (`to.extend(from)`).
fn move_children(tree: &mut Tree, from: NodeId, to: NodeId) {
    while let Some(child) = tree.first_child(from) {
        tree.append(to, child);
    }
}

/// `_contains_block_level_tag`: whether `root` or a descendant has a block tag name, in any namespace.
// upstream: lxml/html/__init__.py::_contains_block_level_tag
fn contains_block_level_tag(tree: &Tree, root: NodeId) -> bool {
    tree.iter(root, &[])
        .any(|id| BLOCK_TAGS.contains(&&*tree.node(id).name))
}

// upstream: lxml/html/__init__.py::fromstring
pub(super) fn fromstring(
    html: &str,
    limits: &ParseLimits,
    call: &Call<'_>,
) -> Result<Tree, ExtractError> {
    let is_full_html = looks_like_full_html(html);
    let mut tree = build_document(html, limits, call)?;
    if is_full_html {
        return Ok(tree);
    }
    let document = tree.root();
    let bodies = children_named(&tree, document, "body");
    let body = bodies.split_first().map(|(&first, others)| {
        // Several bodies are smashed into the first one, with the text of each before its children.
        for &other in others {
            let text = tree.node_mut(other).text.take();
            if let Some(text) = text.filter(|text| !text.is_empty()) {
                match tree.last_child(first) {
                    Some(last) => append_text(&mut tree.node_mut(last).tail, &text),
                    None => append_text(&mut tree.node_mut(first).text, &text),
                }
            }
            move_children(&mut tree, other, first);
            drop_tree(&mut tree, other);
        }
        first
    });
    let heads = children_named(&tree, document, "head");
    if let Some((&head, others)) = heads.split_first() {
        // A document with a head is kept whole.
        for &other in others {
            move_children(&mut tree, other, head);
            drop_tree(&mut tree, other);
        }
        return Ok(tree);
    }
    let Some(body) = body else {
        return Ok(tree);
    };
    let only_child = if tree.child_count(body) == 1 {
        tree.first_child(body)
    } else {
        None
    };
    if let Some(only) = only_child
        && is_blank(tree.node(body).text.as_deref())
        && is_blank(tree.node(only).tail.as_deref())
    {
        // One element was passed in, wrapped by the body that the parser made.
        tree.set_root(only);
        return Ok(tree);
    }
    // A run of content with no single element: the body stands for it, renamed to a container.
    let container = if contains_block_level_tag(&tree, body) {
        "div"
    } else {
        "span"
    };
    tree.set_name(body, container);
    tree.set_root(body);
    Ok(tree)
}

// upstream: lxml/html/__init__.py::fragment_fromstring
pub(super) fn fragment_fromstring_div(
    html: &str,
    limits: &ParseLimits,
    call: &Call<'_>,
) -> Result<Tree, ExtractError> {
    // upstream: lxml/html/__init__.py::fragments_fromstring
    let source: Cow<'_, str> = if looks_like_full_html(html) {
        Cow::Borrowed(html)
    } else {
        Cow::Owned(format!("<html><body>{html}</body></html>"))
    };
    let mut tree = build_document(&source, limits, call)?;
    let document = tree.root();
    // lxml asserts one body; a missing or repeated body has no fragment to return
    let bodies = children_named(&tree, document, "body");
    let &[body] = bodies.as_slice() else {
        return Err(ExtractError::Rejected(Rejection::EmptyTree));
    };
    // blank leading text is dropped; the rest of the body's content moves under the new `div`
    let leading = tree
        .node_mut(body)
        .text
        .take()
        .filter(|text| !text.trim_matches(is_py_space).is_empty());
    let div = tree.new_element("div");
    tree.node_mut(div).text = leading;
    move_children(&mut tree, body, div);
    tree.set_root(div);
    Ok(tree)
}
