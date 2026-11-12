#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Test builders for the trees upstream's tests make with lxml: `etree.fromstring` and
//! `lxml.html.fromstring`. A module of its own, not `tests/common`, which `tests/parity/` also
//! compiles and would warn about the builders it does not use.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::Arc;

use trafilatura_rust::__internals::{
    Config, Ctx, Extractor, Focus, Format, LocalName, NodeId, Ns, ParseLimits, Tree, convert_tags,
    extract_comments, extract_content, parse_document, tree_cleaning,
};
use trafilatura_rust::Call;

type Attrs = Vec<(LocalName, Arc<str>)>;

/// The five predefined XML entities.
fn unescape(text: &str) -> String {
    text.replace("&#13;", "\r")
        .replace("&lt;", "<")
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

/// `lxml.html.fromstring(html)`: the crate's `parse_document`, which is that function over the libxml2
/// tree. Returns the tree and the element lxml returns.
// upstream: lxml/html/__init__.py::fromstring
pub fn lxml_fromstring(html: &str) -> (Tree, NodeId) {
    let tree = parse_document(html, &ParseLimits::default(), &Call::default()).unwrap();
    let root = tree.root();
    (tree, root)
}

/// A context over `options` and `call`, as `Ctx` is made where upstream passes `options`.
pub fn ctx_for<'a>(options: &'a Extractor, call: &'a Call<'a>) -> Ctx<'a> {
    Ctx {
        options: Cow::Borrowed(options),
        call,
    }
}

/// The `Extractor` fields a generated case sets; the others keep their defaults.
pub struct CaseOptions {
    /// `Extractor.tables`.
    pub tables: bool,
    /// `Extractor.images`.
    pub images: bool,
    /// `Extractor.links`.
    pub links: bool,
    /// `Extractor.formatting`.
    pub formatting: bool,
    /// `balanced`, `precision` or `recall`.
    pub focus: &'static str,
    /// `Extractor.comments`.
    pub comments: bool,
    /// `Extractor.url`.
    pub url: Option<&'static str>,
}

/// The `Extractor` of a case: upstream's defaults plus the case's options.
pub fn extractor(options: &CaseOptions) -> Extractor {
    let focus = match options.focus {
        "balanced" => Focus::Balanced,
        "precision" => Focus::Precision,
        "recall" => Focus::Recall,
        other => panic!("bad focus {other}"),
    };
    let (with_metadata, only_with_metadata) = (false, false);
    let mut extractor = Extractor::new(
        Config::default(),
        Format::Python,
        focus == Focus::Precision,
        focus == Focus::Recall,
        Some(options.formatting),
        with_metadata,
        only_with_metadata,
        HashSet::new(),
        HashSet::new(),
    );
    extractor.tables = options.tables;
    extractor.images = options.images;
    extractor.links = options.links;
    extractor.comments = options.comments;
    extractor.url = options.url.map(str::to_string);
    extractor
}

/// The options of a case as one line, for a failure message.
pub fn describe(options: &CaseOptions) -> String {
    format!(
        "tables={} images={} links={} formatting={} focus={} comments={} url={:?}",
        options.tables,
        options.images,
        options.links,
        options.formatting,
        options.focus,
        options.comments,
        options.url
    )
}

/// The html pipeline of a case on the libxml2 parse of `html`: `tree_cleaning`, `convert_tags`, then
/// `op` (`extract_content`, or `comments_then_content`: `extract_comments`, then `extract_content`
/// on the tree it leaves). Returns the tree, the comments body and its text (for the second op),
/// and the content body and its text; the bodies are detached elements of the tree.
pub fn run_pipeline(
    html: &str,
    op: &str,
    options: &CaseOptions,
) -> (Tree, Option<(NodeId, String)>, NodeId, String) {
    let options = extractor(options);
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let mut tree = parse_document(html, &ParseLimits::default(), &call).unwrap();
    let root = tree.root();
    let root = tree_cleaning(&mut tree, root, &ctx).unwrap();
    convert_tags(&mut tree, root, &ctx, None).unwrap();
    let comments = match op {
        "extract_content" => None,
        "comments_then_content" => Some(extract_comments(&mut tree, root, &ctx).unwrap()),
        other => panic!("unknown op {other}"),
    };
    let (body, text) = extract_content(&mut tree, root, &ctx).unwrap();
    (tree, comments, body, text)
}
