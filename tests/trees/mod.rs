#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Test builders for the trees upstream's tests make with lxml: `etree.fromstring` and
//! `lxml.html.fromstring`. A module of its own, not `tests/common`, which `tests/parity.rs` also
//! compiles and would warn about the builders it does not use.

use std::borrow::Cow;
use std::sync::Arc;

use trafilatura_rust::__internals::{
    Ctx, Extractor, LocalName, NodeId, Ns, ParseLimits, Tree, parse_document,
};
use trafilatura_rust::Call;

type Attrs = Vec<(LocalName, Arc<str>)>;

/// The five predefined XML entities.
fn unescape(text: &str) -> String {
    text.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// `name="value"` and `name='value'` pairs in source order.
fn parse_attrs(mut rest: &str) -> Attrs {
    let mut attrs = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            return attrs;
        }
        let (name, after) = rest.split_once('=').expect("attribute without `=`");
        let after = after.trim_start();
        let quote = after
            .chars()
            .next()
            .filter(|quote| matches!(quote, '"' | '\''))
            .expect("unquoted attribute value");
        let (value, after) = after
            .strip_prefix(quote)
            .and_then(|inside| inside.split_once(quote))
            .expect("unterminated attribute value");
        attrs.push((
            LocalName::from(name.trim()),
            Arc::from(unescape(value).as_str()),
        ));
        rest = after;
    }
}

/// The byte index of the `>` that ends a tag, one inside quotes does not count.
fn tag_end(tag: &str) -> usize {
    let mut open_quote = None;
    for (index, c) in tag.char_indices() {
        match (open_quote, c) {
            (None, '"' | '\'') => open_quote = Some(c),
            (Some(quote), c) if c == quote => open_quote = None,
            (None, '>') => return index,
            _ => {}
        }
    }
    panic!("tag without `>`: {tag}");
}

/// `etree.fromstring(source)`: the first tag is the root. Tags, attributes in source order with
/// either quote, self-closing tags, text and tail, and the five XML entities in text and values.
/// Panics on malformed input.
pub fn xml_tree(source: &str) -> Tree {
    let mut tree: Option<Tree> = None;
    let mut stack: Vec<NodeId> = Vec::new();
    let mut rest = source;
    while let Some((text, after)) = rest.split_once('<') {
        if let (Some(tree), Some(&open)) = (tree.as_mut(), stack.last()) {
            tree.add_text(open, &unescape(text));
        }
        let (tag, after) = after.split_at(tag_end(after));
        rest = after.strip_prefix('>').unwrap();
        if tag.starts_with('/') {
            stack.pop().expect("closing tag without an open element");
            continue;
        }
        let (tag, self_closing) = match tag.strip_suffix('/') {
            Some(tag) => (tag, true),
            None => (tag, false),
        };
        let (name, attrs) = tag.split_once(char::is_whitespace).unwrap_or((tag, ""));
        let (name, attrs) = (LocalName::from(name), parse_attrs(attrs));
        let id = match (tree.as_mut(), stack.last()) {
            (Some(tree), Some(&parent)) => tree.add_child(parent, name, Ns::Html, attrs),
            (None, _) => {
                let new = Tree::new(name, Ns::Html, attrs);
                let root = new.root();
                tree = Some(new);
                root
            }
            (Some(_), None) => panic!("second root element"),
        };
        if !self_closing {
            stack.push(id);
        }
    }
    assert!(stack.is_empty(), "unclosed element");
    tree.expect("no element in the source")
}

/// `lxml.html.defs.block_tags`.
const BLOCK_TAGS: &[&str] = &[
    "h5",
    "ul",
    "h1",
    "h4",
    "div",
    "ins",
    "thead",
    "form",
    "legend",
    "del",
    "tr",
    "h6",
    "dir",
    "col",
    "dt",
    "optgroup",
    "pre",
    "dd",
    "noscript",
    "option",
    "tbody",
    "menu",
    "p",
    "li",
    "center",
    "colgroup",
    "td",
    "h2",
    "ol",
    "caption",
    "hr",
    "tfoot",
    "blockquote",
    "address",
    "th",
    "table",
    "isindex",
    "h3",
    "fieldset",
    "dl",
];

/// Whether the source starts like a whole document: `^\s*<(?:html|!doctype)`, ignoring case.
fn looks_like_full_html(html: &str) -> bool {
    let Some(rest) = html.trim_start().strip_prefix('<') else {
        return false;
    };
    let starts = |prefix: &str| {
        rest.get(..prefix.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(prefix))
    };
    starts("html") || starts("!doctype")
}

/// `lxml.html.fromstring(html)` over the html5 tree: the element lxml returns, and the tree.
///
/// A full document gives `html`. Otherwise, the only child of `body` when it has one, with no
/// text around it; else `body` itself, renamed `div` when it holds a block-level tag and `span`
/// when not. lxml also returns the whole document whenever the parse has a `head`; the html5 tree
/// always has one, so that check is left out.
// upstream: lxml/html/__init__.py::fromstring
pub fn lxml_fromstring(html: &str) -> (Tree, NodeId) {
    let mut tree = parse_document(html, &ParseLimits::default(), &Call::default()).unwrap();
    let root = tree.root();
    if looks_like_full_html(html) {
        return (tree, root);
    }
    let body = tree
        .children(root)
        .find(|&child| tree.tag(child) == Some("body"))
        .expect("the html5 tree has a body");
    let blank = |text: &Option<String>| text.as_deref().is_none_or(|text| text.trim().is_empty());
    if let Some(only) = tree.first_child(body)
        && tree.child_count(body) == 1
        && blank(&tree.node(body).text)
        && blank(&tree.node(only).tail)
    {
        return (tree, only);
    }
    let has_block = tree
        .iter(body, &[])
        .any(|element| BLOCK_TAGS.contains(&&*tree.node(element).name));
    tree.set_name(body, if has_block { "div" } else { "span" });
    (tree, body)
}

/// A context over `options` and `call`, as `Ctx` is made where upstream passes `options`.
pub fn ctx_for<'a>(options: &'a Extractor, call: &'a Call<'a>) -> Ctx<'a> {
    Ctx {
        options: Cow::Borrowed(options),
        call,
    }
}

/// `text` as libxml2 writes it: `&`, `<`, `>` and `\r` become references, and in an attribute value
/// `"`, `\n` and `\t` too.
fn escape(text: &str, attribute: bool) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#13;"),
            '"' if attribute => out.push_str("&quot;"),
            '\n' if attribute => out.push_str("&#10;"),
            '\t' if attribute => out.push_str("&#9;"),
            c => out.push(c),
        }
    }
    out
}

fn write_element(tree: &Tree, node: NodeId, out: &mut String) {
    let data = tree.node(node);
    out.push('<');
    out.push_str(&data.name);
    for (name, value) in &data.attrs {
        out.push_str(&format!(" {name}=\"{}\"", escape(value, true)));
    }
    // lxml writes `<x/>` only when there is no text node at all: `Some("")` gives `<x></x>`.
    if data.text.is_none() && tree.first_child(node).is_none() {
        out.push_str("/>");
        return;
    }
    out.push('>');
    out.push_str(&escape(data.text.as_deref().unwrap_or(""), false));
    for child in tree.children(node) {
        write_element(tree, child, out);
        out.push_str(&escape(
            tree.node(child).tail.as_deref().unwrap_or(""),
            false,
        ));
    }
    out.push_str(&format!("</{}>", data.name));
}

/// `etree.tostring(node, encoding="unicode", with_tail=with_tail)`. Elements are written under
/// their local name, so a foreign element is not faithful: assert those through tree dumps.
pub fn tostring(tree: &Tree, node: NodeId, with_tail: bool) -> String {
    let mut out = String::new();
    write_element(tree, node, &mut out);
    if with_tail {
        out.push_str(&escape(
            tree.node(node).tail.as_deref().unwrap_or(""),
            false,
        ));
    }
    out
}
