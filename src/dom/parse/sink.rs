//! The html5ever tree sink and the driver that feeds it in chunks; it has no upstream counterpart (lxml gives upstream the parser).

use std::borrow::Cow;
use std::cell::RefCell;
use std::mem;
use std::sync::Arc;

use html5ever::interface::tree_builder::{ElementFlags, NodeOrText, QuirksMode, TreeSink};
use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{
    BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use html5ever::tree_builder::{TreeBuilder, TreeBuilderOpts, create_element};
use html5ever::{Attribute, LocalName, ParseOpts, QualName, TokenizerResult, local_name, ns};

use crate::cancel::{CHECK_INTERVAL, Call};
use crate::dom::scan::{Pause, TagScanner};
use crate::dom::{Ns, Tree};
use crate::error::{ExtractError, Rejection, TreeLimit};

use super::{
    ATTRIBUTES_PER_NODE, BOM, CHUNK_BYTES, CLONE_UNITS_PER_ATTRIBUTE, Inner, Kind,
    MAX_TAG_ATTRIBUTES, NOT_IN_SCOPE, OwnedName, PAIR_UNITS, ParseLimits, is_formatting, ns_of,
    sort_units,
};

pub(super) struct Sink<'c> {
    pub(super) inner: RefCell<Inner>,
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

    pub(super) fn node_count(&self) -> usize {
        self.inner.borrow().nodes.len()
    }

    /// Charges the Noah's Ark compare of a formatting start tag html5ever has just processed: the
    /// compare belongs to the element the tag itself creates, the newest HTML element named `name`
    /// since arena index `since` (reconstruction clones come first and run no compare). Nothing is
    /// charged when that element was not placed.
    fn charge_compare_since(&self, since: usize, name: &LocalName) {
        let mut inner = self.inner.borrow_mut();
        let newest = (since..inner.nodes.len()).rev().find_map(|index| {
            let id = u32::try_from(index).ok()?;
            let node = inner.node(id);
            (node.kind == Kind::Element && node.qual.ns == ns!(html) && node.qual.local == *name)
                .then_some((id, node.parent))
        });
        if let Some((id, Some(parent))) = newest {
            inner.charge_formatting_compare(parent, id);
        }
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
            tree.reserve(inner.elements);
            let root = tree.root();
            inner.convert(html, &mut tree, root);
            return Ok(tree);
        }
        let node = inner.node_mut(html);
        let attrs = mem::take(&mut node.attrs);
        let mut tree = Tree::new(node.qual.local.clone(), ns_of(&node.qual), attrs);
        tree.dropped_script_text = inner.dropped_script_text;
        // `elements` counts every element created, clones included: an upper bound.
        tree.reserve(inner.elements);
        let root = tree.root();
        inner.convert(html, &mut tree, root);
        Ok(tree)
    }

    // The adoption agency leaves the formatting element in the list on this error, and cloned its
    // attributes first: the cost of one clone is at most the widest formatting element's.
    fn parse_error(&self, msg: Cow<'static, str>) {
        if msg == NOT_IN_SCOPE {
            let mut inner = self.inner.borrow_mut();
            let units = inner.max_formatting_attributes.saturating_add(PAIR_UNITS);
            inner.charge(units);
        }
    }

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
        inner.attributes = inner.attributes.saturating_add(attrs.len());
        let formatting = is_formatting(&name);
        if formatting {
            inner.max_formatting_attributes = inner.max_formatting_attributes.max(attrs.len());
            // The sink cannot tell a reconstruction clone from the tag it copies, so both are charged.
            inner.charge(attrs.len().saturating_mul(CLONE_UNITS_PER_ATTRIBUTE));
        }
        if inner.abort.is_none() {
            // deviation: D-5
            if inner.max_nodes.is_some_and(|max| {
                inner.elements > max || inner.attributes > max.saturating_mul(ATTRIBUTES_PER_NODE)
            }) {
                inner.abort = Some(ExtractError::Rejected(Rejection::TreeTooLarge(
                    TreeLimit::MaxNodes,
                )));
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
        let sort_units = if formatting { sort_units(&attrs) } else { 0 };
        let id = inner.new_node(Kind::Element, name);
        let node = inner.node_mut(id);
        node.attrs = attrs;
        node.sort_units = sort_units;
        node.mathml_integration_point = flags.mathml_annotation_xml_integration_point;
        id
    }

    fn create_comment(&self, _text: StrTendril) -> u32 {
        self.inner.borrow().dropped
    }

    fn create_pi(&self, _target: StrTendril, _data: StrTendril) -> u32 {
        self.inner.borrow().dropped
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
            inner.abort = Some(ExtractError::Rejected(Rejection::TreeTooLarge(
                TreeLimit::TagAttributes,
            )));
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

pub(super) fn parse_opts() -> ParseOpts {
    ParseOpts {
        // libxml2 and html5lib parse `<noscript>` content as elements.
        tree_builder: TreeBuilderOpts {
            scripting_enabled: false,
            ..TreeBuilderOpts::default()
        },
        ..ParseOpts::default()
    }
}

pub(super) fn new_sink<'c>(limits: &ParseLimits, call: &'c Call<'c>, fragment: bool) -> Sink<'c> {
    let mut inner = Inner::new(limits);
    inner.drop_script_text = limits.drop_script_text && !fragment;
    Sink {
        inner: RefCell::new(inner),
        call,
        fragment,
    }
}

/// html5ever's tree builder behind a gate: once the sink has stopped (limit, cancel, depth cut) every
/// later token is dropped, so html5ever's work after an abort is the token it was in, not the rest
/// of the piece.
struct Gate<'c> {
    builder: TreeBuilder<u32, Sink<'c>>,
}

impl TokenSink for Gate<'_> {
    type Handle = u32;

    fn process_token(&self, token: Token, line: u64) -> TokenSinkResult<u32> {
        let sink = &self.builder.sink;
        if sink.stopped() {
            return TokenSinkResult::Continue;
        }
        // html5ever runs its Noah's Ark compare once per formatting start tag, in
        // `create_formatting_element_for`, so the charge is made per such tag.
        let formatting = match &token {
            Token::TagToken(tag)
                if tag.kind == TagKind::StartTag
                    && is_formatting(&QualName::new(None, ns!(html), tag.name.clone())) =>
            {
                Some((sink.node_count(), tag.name.clone()))
            }
            _ => None,
        };
        let result = self.builder.process_token(token, line);
        if let Some((since, name)) = formatting {
            sink.charge_compare_since(since, &name);
        }
        result
    }

    fn end(&self) {
        if !self.builder.sink.stopped() {
            self.builder.end();
        }
    }

    fn adjusted_current_node_present_but_not_in_html_namespace(&self) -> bool {
        self.builder
            .adjusted_current_node_present_but_not_in_html_namespace()
    }
}

/// What `html5ever::Parser` is, with the gate between tokenizer and tree builder.
pub(super) struct Driver<'c> {
    tokenizer: Tokenizer<Gate<'c>>,
    input: BufferQueue,
}

impl<'c> Driver<'c> {
    pub(super) fn new(sink: Sink<'c>, fragment: bool) -> Self {
        let opts = parse_opts();
        let (builder, initial_state) = if fragment {
            // The context element is `div`.
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
        Self {
            tokenizer: Tokenizer::new(
                Gate { builder },
                TokenizerOpts {
                    initial_state,
                    ..opts.tokenizer
                },
            ),
            input: BufferQueue::default(),
        }
    }

    pub(super) fn sink(&self) -> &Sink<'c> {
        &self.tokenizer.sink.builder.sink
    }

    fn feed_all(&self) {
        while !matches!(self.tokenizer.feed(&self.input), TokenizerResult::Done) {}
    }

    pub(super) fn process(&self, piece: &str) {
        self.input.push_back(StrTendril::from_slice(piece));
        self.feed_all();
    }

    fn finish(self) -> Result<Tree, ExtractError> {
        self.feed_all();
        self.tokenizer.end();
        self.tokenizer.sink.builder.sink.finish()
    }
}

/// End of the piece that starts at `start`, always past it: `CHUNK_BYTES` on, at a char boundary and
/// not before a `BOM`. A run of BOMs that fills the piece moves the end forward to the end of the run.
fn chunk_end(html: &str, start: usize) -> usize {
    let is_cut = |end: usize| {
        html.is_char_boundary(end) && !html.get(end..).is_some_and(|rest| rest.starts_with(BOM))
    };
    let natural = start.saturating_add(CHUNK_BYTES).min(html.len());
    (start.saturating_add(1)..=natural)
        .rev()
        .find(|&end| is_cut(end))
        .or_else(|| (natural.saturating_add(1)..=html.len()).find(|&end| is_cut(end)))
        .unwrap_or(html.len())
}

pub(super) fn drive(
    parser: Driver<'_>,
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
        let count_before = parser.sink().node_count();
        if let Some(piece) = html.get(start..cut).filter(|piece| !piece.is_empty()) {
            parser.process(piece);
            let sink = parser.sink();
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
                let switched = parser.sink().html_element_since(count_before, &name);
                scanner.resume_after_tag(switched);
            }
            Pause::Cdata => scanner.resume_at_cdata(
                parser
                    .tokenizer
                    .sink
                    .adjusted_current_node_present_but_not_in_html_namespace(),
            ),
        }
        // Every pause is answered above, so `scan` consumes at least one byte of a non-empty piece.
        debug_assert!(
            cut > start,
            "the piece starting at {start} made no progress"
        );
        start = cut;
    }
    call.check()?;
    let tree = parser.finish()?;
    if limits.max_nodes.is_some_and(|max| tree.len() > max) {
        return Err(ExtractError::Rejected(Rejection::TreeTooLarge(
            TreeLimit::MaxNodes,
        )));
    }
    Ok(tree)
}
