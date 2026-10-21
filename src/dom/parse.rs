//! html5ever tree sink over the arena, and the two parse entry points.
//!
//! html5ever calls the sink through `&self`, so the working tree lives in a `RefCell` that is
//! borrowed for the length of one sink call only. The sink keeps text as separate nodes while
//! parsing (the adoption agency and foster parenting move elements, never the text around them)
//! and folds them into the text/tail model of `Tree` in `finish`.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::mem;
use std::sync::Arc;

use html5ever::interface::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tree_builder::TreeBuilderOpts;
use html5ever::{Attribute, LocalName, ParseOpts, Parser, QualName, local_name, ns};

use super::{Ns, Tree};
use crate::cancel::{CHECK_INTERVAL, Call};
use crate::error::{ExtractError, Rejection};

/// libxml2's nesting limit (AGENTS Runtime contract): parsing ends at the cut.
pub const MAX_DEPTH: usize = 256;

/// Input is fed to html5ever in chunks of this many bytes, with a cancel and node-limit check between
/// chunks. html5ever cannot be stopped inside a chunk, so the chunk size bounds the work done after an
/// abort.
const CHUNK_BYTES: usize = 8 * 1024;

/// Attribute values at least this long are shared between the elements html5ever clones.
const SHARED_VALUE_BYTES: usize = 32;

/// Limits applied while parsing.
#[derive(Clone, Copy, Debug)]
pub struct ParseLimits {
    /// Element nesting depth at which parsing stops and the partial tree is kept.
    pub max_depth: usize,
    /// Element count above which parsing fails with `Rejection::TreeTooLarge`; `None` = no limit.
    pub max_nodes: Option<usize>,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            max_depth: MAX_DEPTH,
            max_nodes: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Document,
    Element,
    Text,
    /// Comment, processing instruction: has a handle but is never attached.
    Dropped,
}

struct SinkNode {
    kind: Kind,
    qual: QualName,
    attrs: Vec<(LocalName, Arc<str>)>,
    text: String,
    mathml_integration_point: bool,
    parent: Option<u32>,
    first_child: Option<u32>,
    last_child: Option<u32>,
    prev: Option<u32>,
    next: Option<u32>,
}

struct Inner {
    nodes: Vec<SinkNode>,
    max_depth: usize,
    max_nodes: Option<usize>,
    /// Elements created so far, the unattached ones html5ever clones included.
    elements: usize,
    /// Set at the depth cut or on abort: every later sink call changes nothing.
    stopped: bool,
    abort: Option<ExtractError>,
    /// Long attribute values by (address, length) of the buffer html5ever hands over. html5ever
    /// clones the open formatting elements' attributes (tendrils share one buffer) for every block
    /// element, so one value would otherwise be copied once per clone. The address is only a
    /// hint: a hit is used after comparing content, so a reused address cannot return a wrong value.
    values: HashMap<(usize, usize), Arc<str>>,
}

fn empty_name() -> QualName {
    QualName::new(None, ns!(), local_name!(""))
}

fn ns_of(qual: &QualName) -> Ns {
    if qual.ns == ns!(svg) {
        Ns::Svg
    } else if qual.ns == ns!(mathml) {
        Ns::MathMl
    } else {
        Ns::Html
    }
}

impl Inner {
    fn new(limits: &ParseLimits) -> Self {
        let mut inner = Self {
            nodes: Vec::new(),
            max_depth: limits.max_depth,
            max_nodes: limits.max_nodes,
            elements: 0,
            stopped: false,
            abort: None,
            values: HashMap::new(),
        };
        inner.new_node(Kind::Document, empty_name());
        inner
    }

    /// Handle 0 is the document, which also stands in when the arena is full (then `stopped`).
    fn new_node(&mut self, kind: Kind, qual: QualName) -> u32 {
        let Ok(id) = u32::try_from(self.nodes.len()) else {
            self.stopped = true;
            return 0;
        };
        self.nodes.push(SinkNode {
            kind,
            qual,
            attrs: Vec::new(),
            text: String::new(),
            mathml_integration_point: false,
            parent: None,
            first_child: None,
            last_child: None,
            prev: None,
            next: None,
        });
        id
    }

    fn share(&mut self, value: &str) -> Arc<str> {
        if value.len() < SHARED_VALUE_BYTES {
            return Arc::from(value);
        }
        let key = (value.as_ptr() as usize, value.len());
        match self.values.get(&key) {
            Some(shared) if **shared == *value => Arc::clone(shared),
            _ => {
                let shared: Arc<str> = Arc::from(value);
                self.values.insert(key, Arc::clone(&shared));
                shared
            }
        }
    }

    // Handles are minted by `new_node` and nodes are never removed.
    #[allow(clippy::indexing_slicing)]
    fn node(&self, id: u32) -> &SinkNode {
        &self.nodes[id as usize]
    }

    #[allow(clippy::indexing_slicing)]
    fn node_mut(&mut self, id: u32) -> &mut SinkNode {
        &mut self.nodes[id as usize]
    }

    fn detach(&mut self, id: u32) {
        let (parent, prev, next) = {
            let node = self.node_mut(id);
            (node.parent.take(), node.prev.take(), node.next.take())
        };
        match next {
            Some(next) => self.node_mut(next).prev = prev,
            None => {
                if let Some(parent) = parent {
                    self.node_mut(parent).last_child = prev;
                }
            }
        }
        match prev {
            Some(prev) => self.node_mut(prev).next = next,
            None => {
                if let Some(parent) = parent {
                    self.node_mut(parent).first_child = next;
                }
            }
        }
    }

    /// Number of elements from `id` up to the root, `id` included when it is an element.
    fn depth(&self, id: u32) -> usize {
        let mut depth = 0;
        let mut current = Some(id);
        while let Some(node_id) = current {
            let node = self.node(node_id);
            depth += usize::from(node.kind == Kind::Element);
            current = node.parent;
        }
        depth
    }

    /// Longest chain of elements in the subtree of `id`, `id` included.
    fn height(&self, id: u32) -> usize {
        let mut tallest = 0;
        let mut stack = vec![(id, 1usize)];
        while let Some((node_id, level)) = stack.pop() {
            let node = self.node(node_id);
            if node.kind != Kind::Element {
                continue;
            }
            tallest = tallest.max(level);
            let mut child = node.first_child;
            while let Some(child_id) = child {
                stack.push((child_id, level + 1));
                child = self.node(child_id).next;
            }
        }
        tallest
    }

    /// Whether `child`, with its subtree, stays within the depth limit under `parent`.
    fn fits(&self, parent: u32, child: u32) -> bool {
        if self.node(child).kind != Kind::Element {
            return true;
        }
        let height = if self.node(child).first_child.is_none() {
            1
        } else {
            self.height(child)
        };
        self.depth(parent).saturating_add(height) <= self.max_depth
    }

    /// Attaches `child` under `parent`, before `before` or last; past the depth limit the child
    /// stays detached and the parse stops.
    fn place(&mut self, parent: u32, child: u32, before: Option<u32>) {
        if self.stopped || self.node(child).kind == Kind::Dropped {
            return;
        }
        if !self.fits(parent, child) {
            self.stopped = true;
            return;
        }
        self.detach(child);
        let prev = match before {
            Some(sibling) => self.node(sibling).prev,
            None => self.node(parent).last_child,
        };
        {
            let node = self.node_mut(child);
            node.parent = Some(parent);
            node.prev = prev;
            node.next = before;
        }
        match prev {
            Some(prev) => self.node_mut(prev).next = Some(child),
            None => self.node_mut(parent).first_child = Some(child),
        }
        match before {
            Some(sibling) => self.node_mut(sibling).prev = Some(child),
            None => self.node_mut(parent).last_child = Some(child),
        }
    }

    fn insert(&mut self, parent: u32, before: Option<u32>, child: NodeOrText<u32>) {
        match child {
            NodeOrText::AppendNode(node) => self.place(parent, node, before),
            NodeOrText::AppendText(text) => {
                if self.stopped {
                    return;
                }
                let prev = match before {
                    Some(sibling) => self.node(sibling).prev,
                    None => self.node(parent).last_child,
                };
                if let Some(prev) = prev.filter(|&prev| self.node(prev).kind == Kind::Text) {
                    self.node_mut(prev).text.push_str(&text);
                    return;
                }
                let id = self.new_node(Kind::Text, empty_name());
                self.node_mut(id).text.push_str(&text);
                self.place(parent, id, before);
            }
        }
    }

    /// Folds the sink tree under `src_root` into `tree` below `out_root`.
    fn convert(&mut self, src_root: u32, tree: &mut Tree, out_root: super::NodeId) {
        // (source element, output element, next source child)
        let mut stack = vec![(src_root, out_root, self.node(src_root).first_child)];
        while let Some(frame) = stack.last_mut() {
            let (_, out, next) = *frame;
            let Some(child) = next else {
                stack.pop();
                continue;
            };
            frame.2 = self.node(child).next;
            let node = self.node_mut(child);
            match node.kind {
                Kind::Text => tree.add_text(out, &node.text),
                Kind::Element => {
                    let attrs = mem::take(&mut node.attrs);
                    let id = tree.add_child(out, node.qual.local.clone(), ns_of(&node.qual), attrs);
                    stack.push((child, id, node.first_child));
                }
                Kind::Document | Kind::Dropped => {}
            }
        }
    }

    /// The first element under the document: `html`.
    fn document_element(&self) -> Option<u32> {
        let mut child = self.node(0).first_child;
        while let Some(id) = child {
            if self.node(id).kind == Kind::Element {
                return Some(id);
            }
            child = self.node(id).next;
        }
        None
    }
}

struct Sink<'c> {
    inner: RefCell<Inner>,
    call: &'c Call<'c>,
    fragment: bool,
}

impl Sink<'_> {
    fn stopped(&self) -> bool {
        self.inner.borrow().stopped
    }

    fn take_abort(&self) -> Option<ExtractError> {
        self.inner.borrow_mut().abort.take()
    }
}

impl TreeSink for Sink<'_> {
    type Handle = u32;
    type Output = Tree;
    type ElemName<'a>
        = std::cell::Ref<'a, QualName>
    where
        Self: 'a;

    fn finish(self) -> Tree {
        let mut inner = self.inner.into_inner();
        let empty = || Tree::new(local_name!("html"), Ns::Html, Vec::new());
        let Some(html) = inner.document_element() else {
            return empty();
        };
        if self.fragment {
            // The context element is `div`; the tree builder's `html` root holds the result.
            let mut tree = Tree::new(local_name!("div"), Ns::Html, Vec::new());
            let root = tree.root();
            inner.convert(html, &mut tree, root);
            return tree;
        }
        let node = inner.node_mut(html);
        let attrs = mem::take(&mut node.attrs);
        let mut tree = Tree::new(node.qual.local.clone(), ns_of(&node.qual), attrs);
        let root = tree.root();
        inner.convert(html, &mut tree, root);
        tree
    }

    fn parse_error(&self, _msg: Cow<'static, str>) {}

    fn get_document(&self) -> u32 {
        0
    }

    fn elem_name<'a>(&'a self, target: &'a u32) -> Self::ElemName<'a> {
        std::cell::Ref::map(self.inner.borrow(), |inner| &inner.node(*target).qual)
    }

    fn create_element(&self, name: QualName, attrs: Vec<Attribute>, flags: ElementFlags) -> u32 {
        let mut inner = self.inner.borrow_mut();
        inner.elements = inner.elements.saturating_add(1);
        if inner.abort.is_none() {
            if inner.max_nodes.is_some_and(|max| inner.elements > max) {
                inner.abort = Some(ExtractError::Rejected(Rejection::TreeTooLarge));
                inner.stopped = true;
            } else if inner.elements.is_multiple_of(CHECK_INTERVAL)
                && !inner.stopped
                && let Err(error) = self.call.check()
            {
                inner.abort = Some(error);
                inner.stopped = true;
            }
        }
        // Once stopped the element only needs a handle: nothing it carries reaches the tree.
        let attrs: Vec<_> = if inner.stopped {
            Vec::new()
        } else {
            attrs
                .into_iter()
                .map(|a| (a.name.local, inner.share(&a.value)))
                .collect()
        };
        let id = inner.new_node(Kind::Element, name);
        let node = inner.node_mut(id);
        node.attrs = attrs;
        node.mathml_integration_point = flags.mathml_annotation_xml_integration_point;
        id
    }

    fn create_comment(&self, _text: StrTendril) -> u32 {
        self.inner
            .borrow_mut()
            .new_node(Kind::Dropped, empty_name())
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> u32 {
        self.inner
            .borrow_mut()
            .new_node(Kind::Dropped, empty_name())
    }

    fn append(&self, parent: &u32, child: NodeOrText<u32>) {
        self.inner.borrow_mut().insert(*parent, None, child);
    }

    fn append_based_on_parent_node(
        &self,
        element: &u32,
        prev_element: &u32,
        child: NodeOrText<u32>,
    ) {
        let mut inner = self.inner.borrow_mut();
        match inner.node(*element).parent {
            Some(parent) => inner.insert(parent, Some(*element), child),
            None => inner.insert(*prev_element, None, child),
        }
    }

    fn append_doctype_to_document(
        &self,
        _name: StrTendril,
        _public_id: StrTendril,
        _system_id: StrTendril,
    ) {
    }

    // Without a separate fragment node, the template element holds its content as html5lib does.
    fn get_template_contents(&self, target: &u32) -> u32 {
        *target
    }

    fn same_node(&self, x: &u32, y: &u32) -> bool {
        x == y
    }

    fn set_quirks_mode(&self, _mode: QuirksMode) {}

    fn append_before_sibling(&self, sibling: &u32, new_node: NodeOrText<u32>) {
        let mut inner = self.inner.borrow_mut();
        if let Some(parent) = inner.node(*sibling).parent {
            inner.insert(parent, Some(*sibling), new_node);
        }
    }

    fn add_attrs_if_missing(&self, target: &u32, attrs: Vec<Attribute>) {
        let mut inner = self.inner.borrow_mut();
        if inner.stopped {
            return;
        }
        let node = inner.node_mut(*target);
        for attr in attrs {
            if !node.attrs.iter().any(|(name, _)| *name == attr.name.local) {
                node.attrs.push((attr.name.local, Arc::from(&*attr.value)));
            }
        }
    }

    fn remove_from_parent(&self, target: &u32) {
        let mut inner = self.inner.borrow_mut();
        if !inner.stopped {
            inner.detach(*target);
        }
    }

    fn reparent_children(&self, node: &u32, new_parent: &u32) {
        let mut inner = self.inner.borrow_mut();
        let mut next = inner.node(*node).first_child;
        while let Some(child) = next {
            if inner.stopped {
                return;
            }
            next = inner.node(child).next;
            inner.place(*new_parent, child, None);
        }
    }

    fn is_mathml_annotation_xml_integration_point(&self, handle: &u32) -> bool {
        self.inner.borrow().node(*handle).mathml_integration_point
    }
}

fn parse_opts() -> ParseOpts {
    ParseOpts {
        // libxml2 and html5lib parse `<noscript>` content as elements.
        tree_builder: TreeBuilderOpts {
            scripting_enabled: false,
            ..TreeBuilderOpts::default()
        },
        ..ParseOpts::default()
    }
}

fn new_sink<'c>(limits: &ParseLimits, call: &'c Call<'c>, fragment: bool) -> Sink<'c> {
    Sink {
        inner: RefCell::new(Inner::new(limits)),
        call,
        fragment,
    }
}

fn drive(
    mut parser: Parser<Sink<'_>>,
    html: &str,
    limits: &ParseLimits,
    call: &Call<'_>,
) -> Result<Tree, ExtractError> {
    let mut start = 0;
    while start < html.len() {
        call.check()?;
        let mut end = start.saturating_add(CHUNK_BYTES).min(html.len());
        while !html.is_char_boundary(end) {
            end -= 1;
        }
        let Some(chunk) = html.get(start..end) else {
            break;
        };
        parser.process(StrTendril::from_slice(chunk));
        let sink = &parser.tokenizer.sink.sink;
        if let Some(error) = sink.take_abort() {
            return Err(error);
        }
        if sink.stopped() {
            break;
        }
        start = end;
    }
    call.check()?;
    let tree = parser.finish();
    if limits.max_nodes.is_some_and(|max| tree.len() > max) {
        return Err(ExtractError::Rejected(Rejection::TreeTooLarge));
    }
    Ok(tree)
}

/// Parses a full document with the HTML5 algorithm; the root is the `html` element.
///
/// Comments, processing instructions and the doctype are dropped. Nesting deeper than
/// `limits.max_depth` ends the parse and keeps the partial tree.
pub fn parse_document(
    html: &str,
    limits: &ParseLimits,
    call: &Call<'_>,
) -> Result<Tree, ExtractError> {
    call.check()?;
    let parser = html5ever::parse_document(new_sink(limits, call, false), parse_opts());
    drive(parser, html, limits, call)
}

/// Parses a fragment in the context of a `div`; the root is that `div`, holding the result.
pub fn parse_fragment(
    html: &str,
    limits: &ParseLimits,
    call: &Call<'_>,
) -> Result<Tree, ExtractError> {
    call.check()?;
    let context = QualName::new(None, ns!(html), local_name!("div"));
    let parser = html5ever::parse_fragment(
        new_sink(limits, call, true),
        parse_opts(),
        context,
        Vec::new(),
        false,
    );
    drive(parser, html, limits, call)
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    // `<p>` then `n` formatting elements with a long attribute, then `m` paragraphs: each `<p>`
    // makes html5ever clone every open formatting element.
    fn amplifying(n: usize, m: usize, value: &str) -> String {
        let mut html = String::from("<html><body><p>");
        for i in 0..n {
            html.push_str(&format!("<b id={i} t={value}>"));
        }
        html.push_str(&"<p>x".repeat(m));
        html
    }

    fn feed_whole<'a>(html: &str, limits: &ParseLimits, call: &'a Call<'a>) -> Parser<Sink<'a>> {
        let mut parser = html5ever::parse_document(new_sink(limits, call, false), parse_opts());
        parser.process(StrTendril::from_slice(html));
        parser
    }

    #[test]
    fn cloned_formatting_elements_share_attribute_values() {
        let value = "v".repeat(500);
        let html = amplifying(20, 50, &value);
        let call = Call::default();
        let parser = feed_whole(&html, &ParseLimits::default(), &call);
        let inner = parser.tokenizer.sink.sink.inner.borrow();
        let values: Vec<&Arc<str>> = inner
            .nodes
            .iter()
            .flat_map(|node| node.attrs.iter())
            .filter(|(name, _)| &**name == "t")
            .map(|(_, value)| value)
            .collect();
        let distinct: HashSet<*const u8> = values.iter().map(|v| v.as_ptr()).collect();
        assert!(values.len() > 20 * 50, "{} elements carry t", values.len());
        assert_eq!(distinct.len(), 20);
    }

    #[test]
    fn no_attributes_are_kept_once_the_node_limit_is_hit() {
        let html = amplifying(50, 2000, &"v".repeat(200));
        let call = Call::default();
        let limits = ParseLimits {
            max_nodes: Some(5_000),
            ..ParseLimits::default()
        };
        let parser = feed_whole(&html, &limits, &call);
        let inner = parser.tokenizer.sink.sink.inner.borrow();
        assert!(inner.stopped);
        assert_eq!(
            inner.abort,
            Some(ExtractError::Rejected(Rejection::TreeTooLarge))
        );
        let with_attrs = inner.nodes.iter().filter(|n| !n.attrs.is_empty()).count();
        assert!(with_attrs <= 5_000, "{with_attrs} nodes kept attributes");
        assert!(inner.elements > 50_000, "the input must outgrow the limit");
    }
}
