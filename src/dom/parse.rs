//! html5ever tree sink over the arena, and the two parse entry points.
//!
//! html5ever calls the sink through `&self`, so the working tree lives in a `RefCell` that is
//! borrowed for the length of one sink call only. The sink keeps text as separate nodes while
//! parsing (the adoption agency and foster parenting move elements, never the text around them)
//! and folds them into the text/tail model of `Tree` in `finish`.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::mem;
use std::sync::Arc;

use html5ever::interface::tree_builder::{
    ElemName, ElementFlags, NodeOrText, QuirksMode, TreeSink,
};
use html5ever::tendril::{StrTendril, TendrilSink};
use html5ever::tokenizer::TokenSink;
use html5ever::tree_builder::TreeBuilderOpts;
use html5ever::{Attribute, LocalName, Namespace, ParseOpts, Parser, QualName, local_name, ns};

use super::scan::{Pause, TagScanner};
use super::{Ns, Tree};
use crate::cancel::{CHECK_INTERVAL, Call};
use crate::error::{ExtractError, Rejection};
use crate::settings::{DEFAULT_MAX_INPUT_BYTES, DEFAULT_MAX_NODES};

/// libxml2's nesting limit (`docs/architecture.md`, Runtime contract): parsing ends at the cut.
pub const MAX_DEPTH: usize = 256;

/// Attributes one tag, or one `html` or `body` element merged from repeated tags, may carry; above
/// it parsing fails with `Rejection::TreeTooLarge`. It bounds html5ever's duplicate check, which is
/// quadratic in the attributes of one tag: a page at the input cap made of tags of this many
/// attributes stays in the order of an ordinary parse. Real pages stay far below 100.
// deviation: D-5
pub const MAX_TAG_ATTRIBUTES: usize = 1_000;

/// Input is fed to html5ever in pieces of at most this many bytes, with a cancel and node-limit check
/// between pieces; a piece ends early at a scanner pause. html5ever cannot be stopped inside a piece, so
/// the piece size bounds the work done after an abort.
const CHUNK_BYTES: usize = 8 * 1024;

/// U+FEFF: html5ever drops one at the start of every `feed` call, so no piece may start with it.
const BOM: char = '\u{feff}';

/// Attribute values at least this long are shared between the elements html5ever clones.
const SHARED_VALUE_BYTES: usize = 32;

/// Limits applied while parsing.
#[derive(Clone, Copy, Debug)]
pub struct ParseLimits {
    /// Element nesting depth at which parsing stops and the partial tree is kept.
    pub max_depth: usize,
    /// Element count above which parsing fails with `Rejection::TreeTooLarge`, html5ever's clones of
    /// open formatting elements included; `None` = no limit. Defaults to `DEFAULT_MAX_NODES`.
    pub max_nodes: Option<usize>,
    /// Raw input length above which loading fails with `Rejection::InputTooLarge`, checked before
    /// decompression; `None` = no limit. Defaults to `DEFAULT_MAX_INPUT_BYTES`.
    pub max_input_bytes: Option<usize>,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            max_depth: MAX_DEPTH,
            // deviation: D-5
            max_nodes: Some(DEFAULT_MAX_NODES),
            // deviation: D-11
            max_input_bytes: Some(DEFAULT_MAX_INPUT_BYTES),
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
    /// Short attribute values by content: tendrils of up to 8 bytes are inline, so a clone of
    /// `id=7` has no shared buffer to recognize.
    short_values: HashSet<Arc<str>>,
    /// Long attribute values by (address, length) of the buffer html5ever hands over. html5ever
    /// clones the open formatting elements' attributes (tendrils share one buffer) for every block
    /// element, so one value would otherwise be copied once per clone. The address is only a
    /// hint: a hit is used after comparing content, so a reused address cannot return a wrong value.
    values: HashMap<(usize, usize), Arc<str>>,
    /// Attribute names of each `add_attrs_if_missing` target (only `html` and `body` reach it), so
    /// a merge looks a name up instead of scanning the element's attributes.
    merged: HashMap<u32, HashSet<LocalName>>,
}

/// An element name cloned out of the arena (atom clones), so html5ever may hold it across sink
/// calls that borrow the `RefCell` mutably.
#[derive(Debug)]
struct OwnedName {
    ns: Namespace,
    local: LocalName,
}

impl ElemName for OwnedName {
    fn ns(&self) -> &Namespace {
        &self.ns
    }

    fn local_name(&self) -> &LocalName {
        &self.local
    }
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
            short_values: HashSet::new(),
            values: HashMap::new(),
            merged: HashMap::new(),
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
            if let Some(shared) = self.short_values.get(value) {
                return Arc::clone(shared);
            }
            let shared: Arc<str> = Arc::from(value);
            self.short_values.insert(Arc::clone(&shared));
            return shared;
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
        let mut depth = 0usize;
        let mut current = Some(id);
        while let Some(node_id) = current {
            let node = self.node(node_id);
            depth = depth.saturating_add(usize::from(node.kind == Kind::Element));
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
                stack.push((child_id, level.saturating_add(1)));
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

    fn node_count(&self) -> usize {
        self.inner.borrow().nodes.len()
    }

    /// Whether an HTML element named `name` was created at or after arena index `since`. After a piece
    /// that ends with a raw-text start tag this says whether html5ever switched its tokenizer: it
    /// inserts those names only from their own start tag, and switches exactly when it inserts one as
    /// an HTML element.
    fn html_element_since(&self, since: usize, name: &LocalName) -> bool {
        let inner = self.inner.borrow();
        inner.nodes.get(since..).is_some_and(|created| {
            created.iter().any(|node| {
                node.kind == Kind::Element && node.qual.ns == ns!(html) && node.qual.local == *name
            })
        })
    }
}

impl TreeSink for Sink<'_> {
    type Handle = u32;
    /// The tree, or the abort (limit, cancel, deadline) an element creation hit while `finish` ran.
    type Output = Result<Tree, ExtractError>;
    type ElemName<'a>
        = OwnedName
    where
        Self: 'a;

    fn finish(self) -> Self::Output {
        let mut inner = self.inner.into_inner();
        if let Some(error) = inner.abort.take() {
            return Err(error);
        }
        let empty = || Tree::new(local_name!("html"), Ns::Html, Vec::new());
        let Some(html) = inner.document_element() else {
            return Ok(empty());
        };
        if self.fragment {
            // The context element is `div`; the tree builder's `html` root holds the result.
            let mut tree = Tree::new(local_name!("div"), Ns::Html, Vec::new());
            let root = tree.root();
            inner.convert(html, &mut tree, root);
            return Ok(tree);
        }
        let node = inner.node_mut(html);
        let attrs = mem::take(&mut node.attrs);
        let mut tree = Tree::new(node.qual.local.clone(), ns_of(&node.qual), attrs);
        let root = tree.root();
        inner.convert(html, &mut tree, root);
        Ok(tree)
    }

    fn parse_error(&self, _msg: Cow<'static, str>) {}

    fn get_document(&self) -> u32 {
        0
    }

    fn elem_name<'a>(&'a self, target: &'a u32) -> Self::ElemName<'a> {
        let inner = self.inner.borrow();
        let qual = &inner.node(*target).qual;
        OwnedName {
            ns: qual.ns.clone(),
            local: qual.local.clone(),
        }
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
        let Inner { nodes, merged, .. } = &mut *inner;
        let Some(node) = nodes.get_mut(*target as usize) else {
            return;
        };
        let names = merged
            .entry(*target)
            .or_insert_with(|| node.attrs.iter().map(|(name, _)| name.clone()).collect());
        let mut exceeded = false;
        for attr in attrs {
            if !names.insert(attr.name.local.clone()) {
                continue;
            }
            // deviation: D-5
            if node.attrs.len() >= MAX_TAG_ATTRIBUTES {
                exceeded = true;
                break;
            }
            node.attrs.push((attr.name.local, Arc::from(&*attr.value)));
        }
        if exceeded {
            inner.abort = Some(ExtractError::Rejected(Rejection::TreeTooLarge));
            inner.stopped = true;
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

/// End of the piece that starts at `start`: `CHUNK_BYTES` on, at a char boundary and not before a
/// `BOM`.
fn chunk_end(html: &str, start: usize) -> usize {
    let mut end = start.saturating_add(CHUNK_BYTES).min(html.len());
    while end > start
        && (!html.is_char_boundary(end)
            || html.get(end..).is_some_and(|rest| rest.starts_with(BOM)))
    {
        end = end.saturating_sub(1);
    }
    if end > start {
        return end;
    }
    // Only a run of BOMs at the very start of the input can fill the piece: take all of it.
    let rest = html.get(start..).unwrap_or_default();
    html.len()
        .saturating_sub(rest.trim_start_matches(BOM).len())
}

fn drive(
    mut parser: Parser<Sink<'_>>,
    html: &str,
    limits: &ParseLimits,
    call: &Call<'_>,
) -> Result<Tree, ExtractError> {
    let mut scanner = TagScanner::new();
    let mut start = 0;
    while start < html.len() {
        call.check()?;
        let end = chunk_end(html, start);
        let (cut, pause) = scanner.scan(html, start, end)?;
        let count_before = parser.tokenizer.sink.sink.node_count();
        if let Some(piece) = html.get(start..cut).filter(|piece| !piece.is_empty()) {
            parser.process(StrTendril::from_slice(piece));
            let sink = &parser.tokenizer.sink.sink;
            if let Some(error) = sink.take_abort() {
                return Err(error);
            }
            if sink.stopped() {
                break;
            }
        }
        match pause {
            Pause::End => {}
            Pause::RawTextTag(name) => {
                let switched = parser
                    .tokenizer
                    .sink
                    .sink
                    .html_element_since(count_before, &name);
                scanner.resume_after_tag(switched);
            }
            Pause::Cdata => scanner.resume_at_cdata(
                parser
                    .tokenizer
                    .sink
                    .adjusted_current_node_present_but_not_in_html_namespace(),
            ),
        }
        start = cut;
    }
    call.check()?;
    let tree = parser.finish()?;
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
    use std::cell::Cell;
    use std::collections::HashSet;

    use html5ever::TokenizerResult;
    use html5ever::tokenizer::{BufferQueue, Token, TokenSinkResult, Tokenizer, TokenizerOpts};
    use html5ever::tree_builder::{TreeBuilder, create_element};

    use super::super::{DumpMode, dump};
    use super::*;
    use crate::cancel::CancelToken;

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

        let ids: Vec<&Arc<str>> = inner
            .nodes
            .iter()
            .flat_map(|node| node.attrs.iter())
            .filter(|(name, _)| &**name == "id")
            .map(|(_, value)| value)
            .collect();
        let distinct_ids: HashSet<*const u8> = ids.iter().map(|v| v.as_ptr()).collect();
        assert!(ids.len() > 20 * 50);
        assert_eq!(
            distinct_ids.len(),
            20,
            "short values are interned by content"
        );
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

    fn attr_pairs(tree: &Tree, id: super::super::NodeId) -> Vec<(String, String)> {
        tree.node(id)
            .attrs
            .iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect()
    }

    fn pair(name: &str, value: &str) -> (String, String) {
        (name.to_string(), value.to_string())
    }

    #[test]
    fn repeated_html_and_body_tags_add_missing_attributes() {
        let html = "<html x=1><body a=1><html x=2 y=3><body a=2 b=3>";
        let tree = parse_document(html, &ParseLimits::default(), &Call::default()).unwrap();
        let root = tree.root();
        let body = tree
            .children(root)
            .find(|&id| &*tree.node(id).name == "body")
            .unwrap();
        assert_eq!(attr_pairs(&tree, root), [pair("x", "1"), pair("y", "3")]);
        assert_eq!(attr_pairs(&tree, body), [pair("a", "1"), pair("b", "3")]);
    }

    #[test]
    fn merged_attributes_are_capped() {
        let tags = |n: usize| (0..n).map(|i| format!("<body a{i}=1>")).collect::<String>();
        let parse = |n: usize| parse_document(&tags(n), &ParseLimits::default(), &Call::default());
        let tree = parse(MAX_TAG_ATTRIBUTES).unwrap();
        let body = tree
            .children(tree.root())
            .find(|&id| &*tree.node(id).name == "body")
            .unwrap();
        assert_eq!(tree.node(body).attrs.len(), MAX_TAG_ATTRIBUTES);
        assert_eq!(
            parse(MAX_TAG_ATTRIBUTES + 1).unwrap_err(),
            ExtractError::Rejected(Rejection::TreeTooLarge)
        );
    }

    // Repeated names are skipped, so the body stays small: parse time stays linear in the input size.
    #[test]
    #[ignore = "slow"]
    fn body_attribute_merge_is_linear() {
        let time = |count: usize| {
            let html: String = (0..count)
                .map(|i| format!("<body a{}=1>", i % 1_000))
                .collect();
            let start = std::time::Instant::now();
            let limits = ParseLimits {
                max_input_bytes: None,
                ..ParseLimits::default()
            };
            parse_document(&html, &limits, &Call::default()).unwrap();
            start.elapsed()
        };
        let base = time(1_000_000);
        let scaled = time(4_000_000);
        assert!(scaled < base * 8, "{base:?} for N, {scaled:?} for 4N");
    }

    // html5ever holds a name from `elem_name` across later sink calls that mutate the arena.
    #[test]
    fn elem_name_held_across_create_element() {
        let call = Call::default();
        let sink = new_sink(&ParseLimits::default(), &call, false);
        let first = sink.create_element(
            QualName::new(None, ns!(html), local_name!("div")),
            Vec::new(),
            ElementFlags::default(),
        );
        let name = sink.elem_name(&first);
        let second = sink.create_element(
            QualName::new(None, ns!(html), local_name!("p")),
            Vec::new(),
            ElementFlags::default(),
        );
        sink.append(&0, NodeOrText::AppendNode(first));
        sink.append(&first, NodeOrText::AppendNode(second));
        assert_eq!(*name.local_name(), local_name!("div"));
        assert_eq!(*name.ns(), ns!(html));
    }

    fn limited(max: usize) -> ParseLimits {
        ParseLimits {
            max_nodes: Some(max),
            ..ParseLimits::default()
        }
    }

    // Elements created while input ends (`<html>` at end of input makes `body`; the empty string
    // makes all three) must count against the limit: html5ever builds them inside `finish`.
    #[test]
    fn node_limit_is_exact_for_elements_created_at_end_of_input() {
        for html in ["", "<html>", "<html><body><di"] {
            let call = Call::default();
            let count = parse_document(html, &ParseLimits::default(), &call)
                .unwrap()
                .len();
            assert!(
                parse_document(html, &limited(count), &call).is_ok(),
                "{html:?}: limit == count"
            );
            assert_eq!(
                parse_document(html, &limited(count - 1), &call).unwrap_err(),
                ExtractError::Rejected(Rejection::TreeTooLarge),
                "{html:?}: limit == count - 1"
            );
        }
    }

    // `drive` checks the token before `finish`, so an abort raised inside `finish` is set directly.
    #[test]
    fn abort_raised_while_finishing_is_reported() {
        for error in [ExtractError::Cancelled, ExtractError::DeadlineExceeded] {
            let call = Call::default();
            let limits = ParseLimits::default();
            let parser = feed_whole("<html>", &limits, &call);
            parser.tokenizer.sink.sink.inner.borrow_mut().abort = Some(error.clone());
            assert_eq!(parser.finish().unwrap_err(), error);
        }
        let token = CancelToken::new();
        token.cancel();
        let call = Call::default().cancel(&token);
        assert_eq!(
            parse_document("", &ParseLimits::default(), &call).unwrap_err(),
            ExtractError::Cancelled
        );
    }

    fn attrs(count: usize) -> String {
        (0..count).map(|i| format!(" a{i}")).collect()
    }

    /// `<x a0 a1 … a{count-1}>`, every name distinct.
    fn tag(count: usize) -> String {
        format!("<x{}>", attrs(count))
    }

    /// 1,500 attributes in 250 groups of every form of attribute.
    fn mixed_forms_tag() -> String {
        let groups: String = (0..250)
            .map(|group| {
                let k = group * 6;
                format!(
                    " a{k}=\"v\" a{}='v' a{}=v a{} a{}=\"\"a{}=\"\"",
                    k + 1,
                    k + 2,
                    k + 3,
                    k + 4,
                    k + 5
                )
            })
            .collect();
        format!("<x{groups}>")
    }

    /// (page, html5ever's tokenizer reads a tag of more than MAX_TAG_ATTRIBUTES attributes)
    fn cases() -> Vec<(String, bool)> {
        let t = tag(1_500);
        let mut cases = vec![
            (t.clone(), true),
            (format!("<p>{t}</p>"), true),
            (mixed_forms_tag(), true),
            (format!("<svg><![cdata[ > {t} ]]>"), true),
            (format!("<frameset><style>{t}"), true),
            (format!("<svg><title>{t}</title></svg>"), true),
            (format!("<svg><style>{t}"), true),
            (format!("<math><style>{t}"), true),
            (format!("<svg><script>{t}"), true),
            (format!("<![CDATA[ > {t}"), true),
            (format!("<!--> {t}"), true),
            (format!("<!---> {t}"), true),
            (format!("<!-- a --!> {t}"), true),
            (
                format!("<script><!--<script></script> <a b=\" </script> {t} \""),
                true,
            ),
            (format!("<title></titlex></title> {t}"), true),
            (format!("<svg><foreignObject><style></style>{t}"), true),
            // html5ever drops a U+FEFF that starts a feed call; the pieces of `drive` never start with one.
            (format!("<{BOM}x{}>", attrs(1_500)), false),
        ];
        for name in [
            "script", "style", "textarea", "title", "xmp", "iframe", "noembed", "noframes",
        ] {
            cases.push((format!("<{name}>\"{t}\"</{name}>"), false));
        }
        for (page, counted) in [
            (format!("<plaintext>{t}"), false),
            (format!("<!-- {t} -->"), false),
            (format!("<!-- --!- {t} -->"), false),
            (format!("<p title=\"{t}\">"), false),
            (format!("<p title='{t}'>"), false),
            (format!("<svg><![CDATA[ {t} ]]></svg>"), false),
            (format!("<?{t}?>"), false),
            (format!("<!DOCTYPE {t}>"), false),
            (format!("<title></titlex> {t} </title>"), false),
            (
                format!("<script><!--<script></script> {t} </script>"),
                false,
            ),
            (format!("<svg><foreignObject><![CDATA[ {t} ]]>"), false),
            (format!("<math><mi><![CDATA[ {t} ]]>"), false),
            (format!("<math><mi><style>\"{t}\"</style>"), false),
            (format!("<svg><desc><title>\"{t}\"</title>"), false),
            (format!("<select><title>\"{t}\"</title>"), false),
            (format!("<select><style>\"{t}\"</style>"), false),
            (tag(1_000), false),
        ] {
            cases.push((page, counted));
        }
        cases
    }

    /// Passes every token to `inner` and records the most attributes on one tag token: the tokenizer's
    /// own count, whatever the tree builder then does with the tag.
    struct TagSpy<'a, S> {
        inner: S,
        most: &'a Cell<usize>,
    }

    impl<S: TokenSink> TokenSink for TagSpy<'_, S> {
        type Handle = S::Handle;

        fn process_token(&self, token: Token, line: u64) -> TokenSinkResult<Self::Handle> {
            if let Token::TagToken(tag) = &token {
                self.most.set(self.most.get().max(tag.attrs.len()));
            }
            self.inner.process_token(token, line)
        }

        fn end(&self) {
            self.inner.end();
        }

        fn adjusted_current_node_present_but_not_in_html_namespace(&self) -> bool {
            self.inner
                .adjusted_current_node_present_but_not_in_html_namespace()
        }
    }

    /// The most attributes html5ever's tokenizer builds on one tag when fed `html` whole.
    fn most_tag_attributes(html: &str, fragment: bool) -> usize {
        let most = Cell::new(0);
        let call = Call::default();
        let limits = ParseLimits {
            max_nodes: None,
            ..ParseLimits::default()
        };
        let sink = new_sink(&limits, &call, fragment);
        let opts = parse_opts();
        let (builder, initial_state) = if fragment {
            let context = create_element(
                &sink,
                QualName::new(None, ns!(html), local_name!("div")),
                Vec::new(),
            );
            let builder = TreeBuilder::new_for_fragment(sink, context, None, opts.tree_builder);
            let state = builder.tokenizer_state_for_context_elem(false);
            (builder, Some(state))
        } else {
            (TreeBuilder::new(sink, opts.tree_builder), None)
        };
        let tokenizer = Tokenizer::new(
            TagSpy {
                inner: builder,
                most: &most,
            },
            TokenizerOpts {
                initial_state,
                ..opts.tokenizer
            },
        );
        let queue = BufferQueue::default();
        queue.push_back(StrTendril::from_slice(html));
        while !matches!(tokenizer.feed(&queue), TokenizerResult::Done) {}
        tokenizer.end();
        most.get()
    }

    /// `html` parsed by html5ever in one piece, without the scanner.
    fn whole_tree(html: &str, fragment: bool) -> Tree {
        let call = Call::default();
        let limits = ParseLimits {
            max_nodes: None,
            ..ParseLimits::default()
        };
        let mut parser = if fragment {
            html5ever::parse_fragment(
                new_sink(&limits, &call, true),
                parse_opts(),
                QualName::new(None, ns!(html), local_name!("div")),
                Vec::new(),
                false,
            )
        } else {
            html5ever::parse_document(new_sink(&limits, &call, false), parse_opts())
        };
        parser.process(StrTendril::from_slice(html));
        parser.finish().unwrap()
    }

    fn parse(html: &str, fragment: bool) -> Result<Tree, ExtractError> {
        let limits = ParseLimits::default();
        let call = Call::default();
        if fragment {
            parse_fragment(html, &limits, &call)
        } else {
            parse_document(html, &limits, &call)
        }
    }

    #[test]
    fn scan_agrees_with_html5ever() {
        // the case starts k bytes before the first chunk boundary, so its markup straddles it
        let padded = |k: usize| format!("<p>{}</p>", "x".repeat(CHUNK_BYTES - 7 - k));
        for k in 0..=16 {
            assert_eq!(padded(k).len() + k, CHUNK_BYTES);
        }
        let mut failures = Vec::new();
        for (page, counted) in cases() {
            let htmls = std::iter::once(page.clone())
                .chain((0..=16).map(|k| format!("{}{page}", padded(k))));
            for (variant, html) in htmls.enumerate() {
                for fragment in [false, true] {
                    let label: String = page.chars().take(60).collect();
                    let ctx = format!("{label:?} variant {variant} fragment {fragment}");
                    // The document tree builder accepts a `frameset` only before any content, so the
                    // `style` that follows is ignored there alone.
                    let counted = if page.starts_with("<frameset>") {
                        variant == 0 && !fragment
                    } else {
                        counted
                    };
                    if (most_tag_attributes(&html, fragment) > MAX_TAG_ATTRIBUTES) != counted {
                        failures.push(format!("html5ever does not agree with the case: {ctx}"));
                        continue;
                    }
                    match (parse(&html, fragment), counted) {
                        (Err(ExtractError::Rejected(Rejection::TreeTooLarge)), true) => {}
                        (Ok(tree), false) => {
                            let whole = whole_tree(&html, fragment);
                            if dump(&tree, DumpMode::Exact) != dump(&whole, DumpMode::Exact) {
                                failures
                                    .push(format!("tree differs from the unscanned parse: {ctx}"));
                            }
                        }
                        (result, _) => failures.push(format!(
                            "counted {counted}, got {:?}: {ctx}",
                            result.map(|_| ())
                        )),
                    }
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{} failures, first: {:#?}",
            failures.len(),
            &failures[..failures.len().min(8)]
        );
    }

    // Random markup made of the pieces that change a tokenizer state, with one tag of
    // MAX_TAG_ATTRIBUTES + 1 attributes at a random place: the scan rejects it exactly when
    // html5ever's tokenizer reads that many attributes on one tag.
    #[test]
    fn scan_agrees_with_html5ever_on_random_markup() {
        let bom = BOM.to_string();
        let pieces = [
            "<",
            ">",
            "</",
            "<!",
            "<!-",
            "<!--",
            "-->",
            "--!>",
            "--",
            "-",
            "<![CDATA[",
            "]]>",
            "]]",
            "]",
            "<?",
            "<!DOCTYPE",
            "<!doctype html",
            " PUBLIC \"x\"",
            "<script>",
            "</script>",
            "<style>",
            "</style>",
            "<title>",
            "</title>",
            "<textarea>",
            "</textarea>",
            "<xmp>",
            "<iframe>",
            "<noembed>",
            "<noframes>",
            "<plaintext>",
            "<svg>",
            "</svg>",
            "<math>",
            "<mi>",
            "<foreignObject>",
            "<desc>",
            "<select>",
            "<table>",
            "<frameset>",
            "<head>",
            "<body>",
            "<p>",
            "<b>",
            "<meta charset=x>",
            "<html>",
            "<template>",
            "<noscript>",
            "<scr",
            "</scr",
            "ipt>",
            "<!--<script>",
            "'",
            "\"",
            "=",
            "/",
            " ",
            "\n",
            "\r\n",
            "x",
            "a",
            "&amp;",
            "&",
            "&#",
            "<a ",
            "<a b=",
            "</a ",
            "<script ",
            "</SCRIPT ",
            "</title ",
            "é",
            bom.as_str(),
        ];
        let big = tag(MAX_TAG_ATTRIBUTES + 1);
        let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
        let mut next = |bound: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            usize::try_from(seed % bound as u64).unwrap()
        };
        let mut failures = Vec::new();
        for round in 0..2_000 {
            let mut parts: Vec<&str> = (0..1 + next(30))
                .map(|_| pieces[next(pieces.len())])
                .collect();
            parts.insert(next(parts.len() + 1), &big);
            let padding = match next(3) {
                0 => String::new(),
                _ => format!("<p>{}</p>", "x".repeat(CHUNK_BYTES - 7 - next(40))),
            };
            let html = format!("{padding}{}", parts.concat());
            for fragment in [false, true] {
                let counted = most_tag_attributes(&html, fragment) > MAX_TAG_ATTRIBUTES;
                let rejected = parse(&html, fragment)
                    .is_err_and(|error| error == ExtractError::Rejected(Rejection::TreeTooLarge));
                if counted != rejected {
                    failures.push(format!(
                        "round {round} fragment {fragment}: html5ever {counted}, scan {rejected}: {:?}",
                        parts.concat()
                    ));
                }
            }
        }
        assert!(
            failures.is_empty(),
            "{} failures, first: {:#?}",
            failures.len(),
            &failures[..failures.len().min(5)]
        );
    }

    // The pauses a scan of `html` in steps of `step` bytes reports, as (position, pause), and whether it
    // rejected. `resume_*` answers alternate, the same way for every step size.
    fn scan_trace(html: &str, step: usize) -> (Vec<(usize, String)>, bool) {
        let mut scanner = TagScanner::new();
        let mut pauses = Vec::new();
        let mut start = 0;
        while start < html.len() {
            let mut end = start.saturating_add(step).min(html.len());
            while !html.is_char_boundary(end) {
                end += 1;
            }
            let Ok((cut, pause)) = scanner.scan(html, start, end) else {
                return (pauses, true);
            };
            let yes = pauses.len() % 2 == 0;
            match &pause {
                Pause::End => {}
                Pause::RawTextTag(_) => scanner.resume_after_tag(yes),
                Pause::Cdata => scanner.resume_at_cdata(yes),
            }
            if pause != Pause::End {
                pauses.push((cut, format!("{pause:?}")));
            }
            start = cut;
        }
        (pauses, false)
    }

    #[test]
    fn state_carries_across_scan_calls() {
        for (page, _) in cases() {
            let label: String = page.chars().take(60).collect();
            assert_eq!(
                scan_trace(&page, 1),
                scan_trace(&page, page.len()),
                "{label:?}"
            );
        }
    }

    #[test]
    fn end_tag_attributes_are_counted() {
        let end = |count: usize| format!("</p{}>", attrs(count));
        assert_eq!(
            parse(&end(MAX_TAG_ATTRIBUTES + 1), false).unwrap_err(),
            ExtractError::Rejected(Rejection::TreeTooLarge)
        );
        assert!(parse(&end(MAX_TAG_ATTRIBUTES), false).is_ok());
    }

    #[test]
    fn duplicate_names_are_counted() {
        let html = format!("<x{}>", " a".repeat(MAX_TAG_ATTRIBUTES + 1));
        assert_eq!(
            parse(&html, false).unwrap_err(),
            ExtractError::Rejected(Rejection::TreeTooLarge)
        );
    }

    #[test]
    fn huge_tag_is_rejected_in_linear_time() {
        let time = |count: usize| {
            let html = tag(count);
            let start = std::time::Instant::now();
            let result = parse(&html, false);
            let elapsed = start.elapsed();
            assert_eq!(
                result.unwrap_err(),
                ExtractError::Rejected(Rejection::TreeTooLarge)
            );
            elapsed
        };
        let base = time(100_000);
        let scaled = time(400_000);
        let second = std::time::Duration::from_secs(1);
        assert!(base < second && scaled < second, "{base:?}, {scaled:?}");
        assert!(scaled < base * 8, "{base:?} for N, {scaled:?} for 4N");
    }

    #[test]
    fn a_bom_after_a_chunk_boundary_is_kept() {
        let padded = format!("<p>{}</p>", "x".repeat(CHUNK_BYTES - 7));
        let html = format!("{padded}{BOM}y");
        let tree = parse(&html, false).unwrap();
        assert_eq!(
            dump(&tree, DumpMode::Exact),
            dump(&whole_tree(&html, false), DumpMode::Exact)
        );
        assert!(dump(&tree, DumpMode::Exact).0.contains(BOM));
    }
}
