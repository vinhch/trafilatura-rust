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
use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{
    BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts,
};
use html5ever::tree_builder::{TreeBuilder, TreeBuilderOpts, create_element};
use html5ever::{
    Attribute, LocalName, Namespace, ParseOpts, QualName, TokenizerResult, local_name, ns,
};

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

/// Attributes the whole parse may create per allowed node (`max_nodes`), reconstruction clones
/// included. Real pages carry about 5 per element; an attack tag of 1,000 attributes cloned by every
/// `<p>` is what the budget stops, long before the node limit.
// deviation: D-5
const ATTRIBUTES_PER_NODE: usize = 10;

/// Work the whole parse may spend per allowed node (`max_nodes`) on html5ever's formatting
/// elements, in the units of `Inner::charge` (one unit is 13 ns): 20,000,000 at the default, about
/// 0.26 s of its attribute compares, sorted in the worst order, and clones. The corpus of
/// `docs/parity-harness.md` stays at least a hundred times below it (`docs/deviations.md`, D-5).
// deviation: D-5
const COMPARE_WORK_PER_NODE: usize = 40;

/// Units one pair of formatting entries costs html5ever beyond its attribute lists: the two list
/// clones and the call, about 160 ns for lists of a few attributes.
const PAIR_UNITS: usize = 12;

/// Units per attribute of a formatting element html5ever clones outside the compare: a
/// reconstruction clone copies the attributes of the `Tag` twice and the sink hashes each value, 50
/// to 120 ns per attribute measured (a clone of a page of 196 entries of 1,000 attributes with values
/// of 0, 20 and 200 bytes); rounded up to 130 ns.
const CLONE_UNITS_PER_ATTRIBUTE: usize = 10;

/// Name bytes that cost one more unit per comparison of a sort: comparing names of a long common
/// prefix takes 11 ns plus 0.067 ns per byte (300 and 1,000 byte names measured), against 13 ns a unit.
const NAME_BYTES_PER_UNIT: usize = 128;

/// html5ever's parse error for an end tag whose formatting element is open but out of scope. The
/// adoption agency clones the element's `Tag` first and leaves it in the active formatting list, so
/// the same end tag can repeat the clone for as long as the input lasts.
const NOT_IN_SCOPE: &str = "Formatting element not in scope";

/// Input is fed to html5ever in pieces of `CHUNK_BYTES`, with a cancel and limit check between
/// pieces. A piece ends early at a scanner pause and runs longer only to take a run of `BOM` whole.
/// html5ever cannot be stopped inside a piece, but `Gate` drops every token once the sink has
/// stopped, so the work after an abort is one token's, and a piece costs html5ever the same
/// per byte whatever the budget.
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
    /// open formatting elements included; so does a total of more than `ATTRIBUTES_PER_NODE`
    /// attributes per allowed element, clones included, or of more than `COMPARE_WORK_PER_NODE` units
    /// of html5ever's work on formatting elements per allowed element. `None` = no limit. Defaults to
    /// `DEFAULT_MAX_NODES`.
    pub max_nodes: Option<usize>,
    /// Raw input length above which loading fails with `Rejection::InputTooLarge`, checked before
    /// decompression; `None` = no limit. Defaults to `DEFAULT_MAX_INPUT_BYTES`.
    pub max_input_bytes: Option<usize>,
    /// Drops the text of HTML `style` elements and of HTML `script` elements whose `type` is not one
    /// upstream reads (`READ_SCRIPT_TYPES`); `parse_document` only, `parse_fragment` ignores it.
    /// `Tree::dropped_script_text` tells whether a parse dropped any. Defaults to `false`.
    pub drop_script_text: bool,
}

impl Default for ParseLimits {
    fn default() -> Self {
        Self {
            max_depth: MAX_DEPTH,
            // deviation: D-5
            max_nodes: Some(DEFAULT_MAX_NODES),
            // deviation: D-11
            max_input_bytes: Some(DEFAULT_MAX_INPUT_BYTES),
            drop_script_text: false,
        }
    }
}

/// The `script` types whose text upstream reads, all as exact `@type=` matches:
/// `baseline.py::_collect_json_content`, `core.py::_forum_thread_page` and `metadata.py`.
pub(crate) const READ_SCRIPT_TYPES: [&str; 2] =
    ["application/ld+json", "application/settings+json"];

/// Whether the text of this element is never read: an HTML `style`, or an HTML `script` whose first
/// `type` attribute is absent or not in `READ_SCRIPT_TYPES` (exact, case-sensitive).
pub(crate) fn is_unread_text_element(
    name: &LocalName,
    ns: Ns,
    attrs: &[(LocalName, Arc<str>)],
) -> bool {
    if ns != Ns::Html {
        return false;
    }
    if *name == local_name!("style") {
        return true;
    }
    if *name != local_name!("script") {
        return false;
    }
    let script_type = attrs
        .iter()
        .find(|(attr, _)| *attr == local_name!("type"))
        .map(|(_, value)| &**value);
    !script_type.is_some_and(|value| READ_SCRIPT_TYPES.contains(&value))
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Document,
    Element,
    Text,
    /// Comment, processing instruction: one shared handle, never attached.
    Dropped,
}

struct SinkNode {
    kind: Kind,
    qual: QualName,
    attrs: Vec<(LocalName, Arc<str>)>,
    /// `sort_units` of `attrs` for a formatting element, else 0.
    sort_units: usize,
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
    /// Attributes handed to `create_element` so far, the clones included.
    attributes: usize,
    /// html5ever's work on formatting elements so far (compares, sorts and clones), in `charge` units.
    compare_work: usize,
    /// Most attributes any formatting element created so far carries.
    max_formatting_attributes: usize,
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
    /// The one handle every comment and processing instruction shares: never attached, never
    /// counted, never changed, so memory does not grow with their number. Handles for which
    /// html5ever calls `same_node` are elements, so two comments comparing equal never matters.
    dropped: u32,
    /// Text under an unread `style` or `script` is not stored (`ParseLimits::drop_script_text`, never
    /// set for a fragment).
    drop_script_text: bool,
    /// A piece of text was dropped.
    dropped_script_text: bool,
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

/// The elements html5ever keeps in its list of active formatting elements and compares on insertion
/// (Noah's Ark clause).
fn is_formatting(qual: &QualName) -> bool {
    [
        local_name!("a"),
        local_name!("b"),
        local_name!("big"),
        local_name!("code"),
        local_name!("em"),
        local_name!("font"),
        local_name!("i"),
        local_name!("nobr"),
        local_name!("s"),
        local_name!("small"),
        local_name!("strike"),
        local_name!("strong"),
        local_name!("tt"),
        local_name!("u"),
    ]
    .contains(&qual.local)
        && qual.ns == ns!(html)
}

/// Units of the clone and sort `Tag::equiv_modulo_attr_order` makes of one attribute list: a pass
/// per attribute and one comparison per attribute and level of a merge sort, each comparison costing
/// a unit plus a unit per `NAME_BYTES_PER_UNIT` of name. The worst order (shuffled names) is charged
/// whatever the order, since an in-order list is the attacker's to give up.
fn sort_units(attrs: &[(LocalName, Arc<str>)]) -> usize {
    let count = attrs.len();
    let name_bytes: usize = attrs.iter().map(|(name, _)| name.len()).sum();
    let levels = usize::try_from(usize::BITS - count.leading_zeros()).unwrap_or(usize::MAX);
    count.saturating_add(
        levels.saturating_mul(count.saturating_add(name_bytes / NAME_BYTES_PER_UNIT)),
    )
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
            attributes: 0,
            compare_work: 0,
            max_formatting_attributes: 0,
            stopped: false,
            abort: None,
            short_values: HashSet::new(),
            values: HashMap::new(),
            merged: HashMap::new(),
            dropped: 0,
            drop_script_text: false,
            dropped_script_text: false,
        };
        inner.new_node(Kind::Document, empty_name());
        // Allocated here, not on first use: a full arena would answer `new_node` with the
        // document handle, which `place` would then attach.
        inner.dropped = inner.new_node(Kind::Dropped, empty_name());
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
            sort_units: 0,
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

    /// Adds `units` of html5ever's work on formatting elements (one unit is 13 ns) and
    /// aborts past `max_nodes * COMPARE_WORK_PER_NODE`.
    fn charge(&mut self, units: usize) {
        self.compare_work = self.compare_work.saturating_add(units);
        // deviation: D-5
        if self.abort.is_none()
            && self
                .max_nodes
                .is_some_and(|max| self.compare_work > max.saturating_mul(COMPARE_WORK_PER_NODE))
        {
            self.abort = Some(ExtractError::Rejected(Rejection::TreeTooLarge));
            self.stopped = true;
        }
    }

    /// Charges the compare html5ever ran before inserting formatting element `child` under
    /// `parent`: against each same-name open formatting element, which are exactly its ancestors
    /// (`docs/architecture.md`, Bounded). Entries before a marker, or already dropped from the list,
    /// are charged too, so this never undercounts.
    fn charge_formatting_compare(&mut self, parent: u32, child: u32) {
        let (name, fresh) = {
            let node = self.node(child);
            (node.qual.local.clone(), node.sort_units)
        };
        let mut units = 0usize;
        let mut ancestor = Some(parent);
        while let Some(id) = ancestor {
            let node = self.node(id);
            if node.kind == Kind::Element && node.qual.local == name && node.qual.ns == ns!(html) {
                units = units
                    .saturating_add(PAIR_UNITS)
                    .saturating_add(fresh)
                    .saturating_add(node.sort_units);
            }
            ancestor = node.parent;
        }
        self.charge(units);
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
                if self.drop_script_text {
                    let owner = self.node(parent);
                    if owner.kind == Kind::Element
                        && is_unread_text_element(
                            &owner.qual.local,
                            ns_of(&owner.qual),
                            &owner.attrs,
                        )
                    {
                        self.dropped_script_text = true;
                        return;
                    }
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
                Kind::Text => tree.add_text_owned(out, mem::take(&mut node.text)),
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
struct Driver<'c> {
    tokenizer: Tokenizer<Gate<'c>>,
    input: BufferQueue,
}

impl<'c> Driver<'c> {
    fn new(sink: Sink<'c>, fragment: bool) -> Self {
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

    fn sink(&self) -> &Sink<'c> {
        &self.tokenizer.sink.builder.sink
    }

    fn feed_all(&self) {
        while !matches!(self.tokenizer.feed(&self.input), TokenizerResult::Done) {}
    }

    fn process(&self, piece: &str) {
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

fn drive(
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
    drive(
        Driver::new(new_sink(limits, call, false), false),
        html,
        limits,
        call,
    )
}

/// Parses a fragment in the context of a `div`; the root is that `div`, holding the result.
pub fn parse_fragment(
    html: &str,
    limits: &ParseLimits,
    call: &Call<'_>,
) -> Result<Tree, ExtractError> {
    call.check()?;
    drive(
        Driver::new(new_sink(limits, call, true), true),
        html,
        limits,
        call,
    )
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::HashSet;

    use html5ever::Parser;
    use html5ever::tendril::TendrilSink;

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
    fn comments_and_processing_instructions_do_not_grow_the_arena() {
        let page = |comment: &str| {
            format!(
                "<html><body><p>a{}b</p></body></html>",
                comment.repeat(100_000)
            )
        };
        let call = Call::default();
        let count = |html: &str| {
            feed_whole(html, &ParseLimits::default(), &call)
                .tokenizer
                .sink
                .sink
                .node_count()
        };
        let bare = count(&page(""));
        for comment in ["<!---->", "<!>", "<?pi>"] {
            let with_comments = count(&page(comment));
            assert!(
                with_comments <= bare + 1,
                "{comment}: {with_comments} nodes against {bare} without comments"
            );
        }
    }

    #[test]
    fn comments_in_every_insertion_mode_parse_as_html5ever_does() {
        let pages = [
            "@<!DOCTYPE html>@<html>@<head>@<title>t@</title>@</head>@<body>@<p>x@y</p>@</body>@</html>@",
            "@<p>a@b",
            "<table>@<tr>@<td>a@b</td>@</tr>@</table>@",
            "<table>x@y</table>",
            "<table>@x@<tr>@",
            "<body>a</body>@b",
            "</body>@</html>@",
            "<template>@<tr>@<td>x@</td></tr>@</template>",
            "<svg>@<g>@<title>@t</title></g>@<![CDATA[x]]>@</svg>@",
            "<math>@<mi>@x</mi>@<annotation-xml encoding=text/html>@<p>@</annotation-xml></math>",
            "<select>@<option>a@b</option>@</select>@",
            "<frameset>@<frame>@</frameset>@",
            "<noscript>@</noscript>@",
            "<textarea>@</textarea>@",
            "<b><p>@</b>@x",
            "<a>1@<div>2@</a>3@",
            "<p><b>x@</p>@<p>y",
            "<caption>@<colgroup>@<col>@",
            "<ul><li>@<ul><li>@</ul></ul>@",
            "<script>@</script>@<style>@</style>@",
        ];
        let comments = ["<!---->", "<!>", "<?pi x>", "<!--c-->", "<!--c--!>"];
        for page in pages {
            for comment in comments {
                let html = page.replace('@', comment);
                for fragment in [false, true] {
                    let tree = parse(&html, fragment).unwrap();
                    let whole = whole_tree(&html, fragment);
                    assert_eq!(
                        dump(&tree, DumpMode::Exact),
                        dump(&whole, DumpMode::Exact),
                        "{html:?} fragment {fragment}"
                    );
                }
            }
        }
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

    // `entries` distinct `<b>` of 1,000 attributes (999 shared, one unique so Noah's Ark keeps them
    // all), then `<p>` makes html5ever clone every one: `entries` x 1,000 attributes per paragraph.
    fn reconstruction_page(entries: usize, paragraphs: usize) -> String {
        let mut html = String::from("<html><body><p>");
        for i in 0..entries {
            html.push_str(&format!("<b{} u{i}>", attrs(MAX_TAG_ATTRIBUTES - 1)));
        }
        html.push_str(&"<p>x".repeat(paragraphs));
        html
    }

    // Fed whole, html5ever keeps working after the abort, so `paragraphs` is chosen to leave the
    // element count below the node limit while the attributes pass the budget.
    fn assert_clones_trip_the_attribute_budget(entries: usize, paragraphs: usize, max: usize) {
        let call = Call::default();
        let html = reconstruction_page(entries, paragraphs);
        let parser = feed_whole(&html, &limited(max), &call);
        let inner = parser.tokenizer.sink.sink.inner.borrow();
        assert_eq!(
            inner.abort,
            Some(ExtractError::Rejected(Rejection::TreeTooLarge))
        );
        let kept: usize = inner.nodes.iter().map(|node| node.attrs.len()).sum();
        assert!(kept <= max * ATTRIBUTES_PER_NODE, "{kept} attributes kept");
        assert!(
            inner.elements < max,
            "the attributes, not the node limit, must trip: {} elements",
            inner.elements
        );
    }

    #[test]
    fn attribute_budget_counts_clones_at_a_small_limit() {
        assert_clones_trip_the_attribute_budget(5, 20, 2_000);
    }

    // 250 entries x 2,000 paragraphs is the reconstruction attack at the default limit.
    #[test]
    #[ignore = "slow"]
    fn attribute_budget_counts_clones() {
        assert_clones_trip_the_attribute_budget(250, 40, DEFAULT_MAX_NODES);
        assert_eq!(
            parse(&reconstruction_page(250, 2_000), false).unwrap_err(),
            ExtractError::Rejected(Rejection::TreeTooLarge)
        );
    }

    #[test]
    fn a_page_within_the_attribute_budget_parses() {
        let page = |count: usize, attributes: usize| {
            format!(
                "<html><body>{}",
                format!("<p{}>", attrs(attributes)).repeat(count)
            )
        };
        let html = page(DEFAULT_MAX_NODES / 2, 10);
        assert!(parse(&html, false).is_ok());
    }

    fn html_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for path in entries.flatten().map(|entry| entry.path()) {
            if path.is_dir() {
                html_files(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "html") {
                out.push(path);
            }
        }
    }

    // The corpus of `docs/parity-harness.md` (the upstream checkout, as `tests/common` finds it): the
    // attributes created and the work charged per page, through the gate, stay a hundredth of their
    // budgets. Skips without it.
    #[test]
    #[ignore = "slow"]
    fn corpus_work_is_far_below_the_budgets() {
        let root = std::env::var_os("TRAFILATURA_UPSTREAM")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../trafilatura")
            });
        let mut files = Vec::new();
        for dir in ["eval", "cache", "resources"] {
            html_files(&root.join("tests").join(dir), &mut files);
        }
        files.sort();
        let mut most = (0, std::path::PathBuf::new());
        let mut most_work = (0, std::path::PathBuf::new());
        for file in &files {
            let html = String::from_utf8_lossy(&std::fs::read(file).unwrap()).into_owned();
            let call = Call::default();
            let driver = Driver::new(
                new_sink(&limited(usize::MAX / ATTRIBUTES_PER_NODE), &call, false),
                false,
            );
            driver.process(&html);
            let inner = driver.sink().inner.borrow();
            if inner.attributes > most.0 {
                most = (inner.attributes, file.clone());
            }
            if inner.compare_work > most_work.0 {
                most_work = (inner.compare_work, file.clone());
            }
        }
        println!(
            "{} pages, most attributes {} in {:?}, most compare work {} in {:?}",
            files.len(),
            most.0,
            most.1,
            most_work.0,
            most_work.1
        );
        let budget = DEFAULT_MAX_NODES * ATTRIBUTES_PER_NODE;
        assert!(most.0 * 100 <= budget, "{} of {budget}", most.0);
        let work_budget = DEFAULT_MAX_NODES * COMPARE_WORK_PER_NODE;
        assert!(
            most_work.0 * 100 <= work_budget,
            "{} of {work_budget}",
            most_work.0
        );
    }

    // Budget of 2,000 nodes is 20,000 attributes: the total passes exactly at the budget.
    #[test]
    fn attribute_budget_is_exact() {
        let limits = limited(2_000);
        let page = |extra: usize| {
            let mut html = String::from("<html><body>");
            html.push_str(&format!("<p{}>", attrs(10)).repeat(1_000));
            html.push_str(&format!("<p{}>", attrs(MAX_TAG_ATTRIBUTES)).repeat(10));
            html.push_str(&format!("<p{}>", attrs(extra)));
            html
        };
        let parse_with = |html: &str| parse_document(html, &limits, &Call::default());
        assert!(parse_with(&page(0)).is_ok());
        assert_eq!(
            parse_with(&page(1)).unwrap_err(),
            ExtractError::Rejected(Rejection::TreeTooLarge)
        );
        let unlimited = ParseLimits {
            max_nodes: None,
            ..limits
        };
        assert!(parse_document(&page(1), &unlimited, &Call::default()).is_ok());
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
        // A character reference right before `<` is still pending when a piece ends there. Its text
        // reconstructs the formatting element `b` as an HTML element, so the section is a bogus
        // comment that ends at the first `>`, not CDATA.
        for char_ref in ["&amp", "&amp;", "&#65", "&#x41", "&"] {
            for foreign in ["<svg><foreignObject><p><b></p>", "<math><mi><p><b></p>"] {
                cases.push((format!("{foreign}{char_ref}<![CDATA[ > {t} ]]>"), true));
                cases.push((format!("{foreign}{char_ref}<!-- {t} -->"), false));
                cases.push((format!("{foreign}{char_ref}<!--> {t}"), true));
            }
        }
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
            "</p>",
            "<b>",
            "</b>",
            "<i>",
            "<meta charset=x>",
            "<html>",
            "<template>",
            "<noscript>",
            "<svg><foreignObject><p><b></p>",
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
            "&amp",
            "&#65",
            "&#x41",
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

    // Without the scan html5ever compares about N^2 / 2 names, which takes minutes for these sizes.
    #[test]
    fn huge_tag_is_rejected_within_a_second() {
        for count in [100_000, 400_000] {
            let html = tag(count);
            let start = std::time::Instant::now();
            let result = parse(&html, false);
            let elapsed = start.elapsed();
            assert_eq!(
                result.unwrap_err(),
                ExtractError::Rejected(Rejection::TreeTooLarge)
            );
            assert!(
                elapsed < std::time::Duration::from_secs(1),
                "{count} attributes: {elapsed:?}"
            );
        }
    }

    // A run of BOMs fills a whole piece, which cannot start or end at one (html5ever drops a BOM at
    // the start of each `feed`): the piece grows to the end of the run instead.
    #[test]
    fn bom_runs_do_not_stall_the_drive() {
        let boms = |count: usize| BOM.to_string().repeat(count);
        let mut pages = vec![
            format!("x{}", boms(3_000)),
            format!("<style>{}", boms(3_000)),
            format!("<script>{}</script>", boms(3_000)),
            format!("<p>x{}y</p>", boms(2 * CHUNK_BYTES)),
            format!("<style>{}</style>y", boms(2 * CHUNK_BYTES)),
            boms(CHUNK_BYTES),
            format!("{}y", boms(CHUNK_BYTES)),
        ];
        for k in 0..=8 {
            pages.push(format!("{}{}tail", "x".repeat(CHUNK_BYTES - k), boms(4)));
            pages.push(format!(
                "{}{}tail",
                "x".repeat(CHUNK_BYTES - k),
                boms(4_000)
            ));
            pages.push(format!(
                "<p>{}{}</p>",
                "x".repeat(CHUNK_BYTES - 3 - k),
                boms(3)
            ));
        }
        for html in &pages {
            for fragment in [false, true] {
                let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                let call = Call::default().deadline(deadline);
                let limits = ParseLimits::default();
                let result = if fragment {
                    parse_fragment(html, &limits, &call)
                } else {
                    parse_document(html, &limits, &call)
                };
                let label: String = html.chars().take(12).collect();
                let tree = result.unwrap_or_else(|error| {
                    panic!(
                        "{label:?} ({} bytes) fragment {fragment}: {error:?}",
                        html.len()
                    )
                });
                assert_eq!(
                    dump(&tree, DumpMode::Exact),
                    dump(&whole_tree(html, fragment), DumpMode::Exact),
                    "{label:?} ({} bytes) fragment {fragment}",
                    html.len()
                );
            }
        }
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

    /// `<{name} a0 .. a998 u{unique}>`: 1,000 attributes, so Noah's Ark keeps entries of one name apart
    /// when `unique` differs.
    fn heavy_tag(name: &str, unique: usize) -> String {
        format!("<{name}{} u{unique}>", attrs(MAX_TAG_ATTRIBUTES - 1))
    }

    fn heavy_tags(name: &str, count: usize) -> String {
        (0..count).map(|i| heavy_tag(name, i)).collect()
    }

    /// `prefix` followed by `unit` repeated up to `bytes`.
    fn filled(prefix: &str, unit: &str, bytes: usize) -> String {
        let count = bytes.saturating_sub(prefix.len()) / unit.len();
        format!("{prefix}{}", unit.repeat(count))
    }

    /// The abort and the compare work of `html` fed in one piece through the gate with `max_nodes`.
    fn compare_outcome(html: &str, max_nodes: usize) -> (Option<ExtractError>, usize) {
        let call = Call::default();
        let driver = Driver::new(new_sink(&limited(max_nodes), &call, false), false);
        driver.process(html);
        let inner = driver.sink().inner.borrow();
        (inner.abort.clone(), inner.compare_work)
    }

    const TOO_LARGE: ExtractError = ExtractError::Rejected(Rejection::TreeTooLarge);

    // The budget of 2,000 nodes is 80,000 units; the compare of one `<b></b>` against 10 entries of
    // 1,000 attributes is about 20,000.
    #[test]
    fn compare_budget_rejects_the_heavy_entries_attack_at_a_small_limit() {
        let attack = |prefix: &str, unit: &str| format!("<html><body>{prefix}{}", unit.repeat(200));
        let heavy = heavy_tags("b", 10);
        let pages = [
            attack(&heavy, "<b></b>"),
            attack(&format!("{heavy}<table>"), "<b></b>"),
            attack(&format!("<p>{heavy}"), "</p><b></b><p>"),
            attack(&format!("{}<table>", heavy_tag("b", 0)), "</b>"),
        ];
        for (index, html) in pages.iter().enumerate() {
            let (abort, work) = compare_outcome(html, 2_000);
            assert_eq!(abort, Some(TOO_LARGE), "page {index}");
            assert!(work > 80_000, "{work}");
            assert_eq!(
                parse_document(html, &limited(2_000), &Call::default()).unwrap_err(),
                TOO_LARGE
            );
        }
    }

    // `max_nodes: None` lifts the compare budget as it lifts the node limit.
    #[test]
    fn compare_budget_is_lifted_with_the_node_limit() {
        let html = format!(
            "<html><body>{}{}",
            heavy_tags("b", 10),
            "<b></b>".repeat(200)
        );
        let unlimited = ParseLimits {
            max_nodes: None,
            ..ParseLimits::default()
        };
        assert!(parse_document(&html, &unlimited, &Call::default()).is_ok());
    }

    // Attack pages at the size they were measured (release: 11 s each without the budget) and at the
    // input cap: each ends rejected in under a second.
    #[test]
    #[ignore = "slow"]
    fn attack_pages_are_rejected_in_under_a_second() {
        let cap = DEFAULT_MAX_INPUT_BYTES;
        let nested = |entries: usize| format!("<html><body>{}", heavy_tags("b", entries));
        let light: String = (0..250).map(|i| format!("<b id=e{i}>")).collect();
        // (label, prefix, repeated unit, count of units; 0 = up to the input cap)
        let mut pages: Vec<(String, String, &str, usize)> = vec![
            (
                "50 heavy b, <b></b> x 2,000".into(),
                nested(50),
                "<b></b>",
                2_000,
            ),
            (
                "50 heavy b, <b></b> x 8,000".into(),
                nested(50),
                "<b></b>",
                8_000,
            ),
            (
                "200 heavy b, <b></b> x 2,000".into(),
                nested(200),
                "<b></b>",
                2_000,
            ),
            (
                "50 heavy b, <b></b> to the cap".into(),
                nested(50),
                "<b></b>",
                0,
            ),
            (
                "200 heavy b, <b></b> to the cap".into(),
                nested(200),
                "<b></b>",
                0,
            ),
            (
                "foster, 50 heavy b, to the cap".into(),
                format!("{}<table>", nested(50)),
                "<b></b>",
                0,
            ),
            (
                "misnested </p>, 50 heavy b, to the cap".into(),
                format!("<html><body><p>{}", heavy_tags("b", 50)),
                "</p><b></b><p>",
                0,
            ),
            (
                "250 light entries, <b id></b> to the cap".into(),
                format!("<html><body>{light}"),
                "<b id=x></b>",
                0,
            ),
        ];
        // End tags of an out-of-scope formatting element: a clone of its attributes each.
        for (name, end) in [("b", "</b>"), ("a", "</a>"), ("nobr", "</nobr>")] {
            for scope in ["<table>", "<svg><desc>"] {
                pages.push((
                    format!("{end} under {scope}, to the cap"),
                    format!("<html><body>{}{scope}", heavy_tag(name, 0)),
                    end,
                    0,
                ));
            }
        }
        let limits = ParseLimits::default();
        for (label, prefix, unit, count) in &pages {
            let html = if *count == 0 {
                filled(prefix, unit, cap)
            } else {
                format!("{prefix}{}", unit.repeat(*count))
            };
            let start = std::time::Instant::now();
            let result = parse_document(&html, &limits, &Call::default());
            let elapsed = start.elapsed();
            println!("{label}: {} bytes, {elapsed:?}", html.len());
            assert_eq!(result.unwrap_err(), TOO_LARGE, "{label}");
            assert!(
                elapsed < std::time::Duration::from_secs(1),
                "{label} ({} bytes): {elapsed:?}",
                html.len()
            );
        }
    }

    /// `<name n0 .. n{count-2} unique>` with the names `{prefix}{i}` in a pseudo-random order and
    /// `value` on each (none when empty): the order that makes `Tag::equiv_modulo_attr_order`'s sorts
    /// do their full work, where an in-order tag costs one pass.
    fn shuffled_tag(
        name: &str,
        count: usize,
        prefix: &str,
        value: &str,
        unique: &str,
        seed: &mut u64,
    ) -> String {
        let mut names: Vec<String> = (0..count - 1).map(|i| format!("{prefix}{i}")).collect();
        names.push(unique.to_string());
        for i in (1..names.len()).rev() {
            *seed ^= *seed << 13;
            *seed ^= *seed >> 7;
            *seed ^= *seed << 17;
            let j = usize::try_from(*seed % (i as u64 + 1)).unwrap();
            names.swap(i, j);
        }
        let mut tag = format!("<{name}");
        for attribute in names {
            if value.is_empty() {
                tag.push_str(&format!(" {attribute}"));
            } else {
                tag.push_str(&format!(" {attribute}=\"{value}\""));
            }
        }
        tag.push('>');
        tag
    }

    fn assert_rejected_in_under_a_second(label: &str, html: &str) {
        let start = std::time::Instant::now();
        let result = parse_document(html, &ParseLimits::default(), &Call::default());
        let elapsed = start.elapsed();
        println!("{label}: {} bytes, {elapsed:?}", html.len());
        assert_eq!(result.unwrap_err(), TOO_LARGE, "{label}");
        assert!(
            elapsed < std::time::Duration::from_secs(1),
            "{label} ({} bytes): {elapsed:?}",
            html.len()
        );
    }

    // The compare sorts both attribute lists, so shuffled names cost about ten times an in-order
    // list (116 ns against 13 ns per attribute at 1,000) and names with a long common prefix more:
    // 50 heavy `b` then probes of 1,000 attributes each followed by `</b>`.
    #[test]
    #[ignore = "slow"]
    fn shuffled_name_attack_pages_are_rejected_in_under_a_second() {
        let long_prefix = "averyveryverylongcommonprefixofnamesxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
        let kilobyte_prefix = "q".repeat(1_000);
        // (prefix, heavy entries, probes), each page up to about 63 MB
        for (prefix, entries, probes) in [
            ("a", 50, 2_000),
            (long_prefix, 50, 700),
            (kilobyte_prefix.as_str(), 30, 20),
        ] {
            let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
            let mut html = String::from("<html><body>");
            for entry in 0..entries {
                html.push_str(&shuffled_tag(
                    "b",
                    1_000,
                    prefix,
                    "",
                    &format!("u{entry}"),
                    &mut seed,
                ));
            }
            for probe in 0..probes {
                html.push_str(&shuffled_tag(
                    "b",
                    1_000,
                    prefix,
                    "",
                    &format!("p{probe}"),
                    &mut seed,
                ));
                html.push_str("</b>");
            }
            let label = format!("shuffled names, prefix of {} bytes", prefix.len());
            assert_rejected_in_under_a_second(&label, &html);
        }
    }

    // Reconstruction clones cost html5ever a clone of the attributes each (50 to 120 ns per attribute,
    // more with long values): 14 formatting names of 14 entries of 1,000 attributes, then `<p>x`.
    #[test]
    #[ignore = "slow"]
    fn reconstruction_attack_pages_are_rejected_in_under_a_second() {
        let names = [
            "a", "b", "big", "code", "em", "font", "i", "nobr", "s", "small", "strike", "strong",
            "tt", "u",
        ];
        for value in [String::new(), "v".repeat(20), "v".repeat(200)] {
            let mut seed = 0x2545_F491_4F6C_DD1D_u64;
            let mut html = String::from("<html><body><p>");
            for name in names {
                for entry in 0..14 {
                    html.push_str(&shuffled_tag(
                        name,
                        1_000,
                        "a",
                        &value,
                        &format!("u{entry}"),
                        &mut seed,
                    ));
                }
            }
            html.push_str(&"<p>x".repeat(2_000));
            let label = format!("14 x 14 entries, values of {} bytes", value.len());
            assert_rejected_in_under_a_second(&label, &html);
        }
    }

    // Tags that cost html5ever nothing after a deep stack of heavy formatting elements: the budget
    // may be nearly spent (N = 40 to 42 heavy entries come close) yet every later tag is free, so
    // nothing may make each of them expensive. Each page takes at most 1.5 times as long as the same
    // tail behind N entries without attributes, whether it ends rejected or parsed. A `</x>` walks
    // the open elements to find its match, so its cost grows with the depth (html5ever's own cost,
    // attributes or not; the figures are in D-5); a comment does not, and takes what it takes alone.
    #[test]
    #[ignore = "slow"]
    fn free_tags_behind_heavy_entries_cost_what_they_cost_alone() {
        let cap = DEFAULT_MAX_INPUT_BYTES;
        let time = |html: &str, limits: &ParseLimits| {
            let start = std::time::Instant::now();
            let result = parse_document(html, limits, &Call::default());
            (start.elapsed(), result.is_ok())
        };
        let default = ParseLimits::default();
        let mut failures = Vec::new();
        for tail in ["</x>", "<!---->"] {
            let tail_only = tail.repeat(cap / tail.len());
            let alone = time(&format!("<html><body>{tail_only}"), &default).0;
            for entries in [1, 20, 40, 42, 43, 50] {
                let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
                let heavy: String = (0..entries)
                    .map(|entry| {
                        let unique = format!("u{entry}");
                        shuffled_tag("b", MAX_TAG_ATTRIBUTES, "a", "", &unique, &mut seed)
                    })
                    .collect();
                let light: String = (0..entries).map(|entry| format!("<b u{entry}>")).collect();
                let (light_time, _) = time(&format!("<html><body>{light}{tail_only}"), &default);
                let (elapsed, parsed) = time(&format!("<html><body>{heavy}{tail_only}"), &default);
                println!(
                    "{entries} heavy b, {tail} to the cap: {elapsed:?} (light entries {light_time:?}, alone {alone:?}), parsed {parsed}"
                );
                if elapsed >= light_time * 3 / 2 {
                    failures.push(format!(
                        "{entries} heavy b, {tail}: {elapsed:?} against {light_time:?} with light entries"
                    ));
                }
                if tail == "<!---->" && elapsed >= alone * 3 / 2 {
                    failures.push(format!(
                        "{entries} heavy b, {tail}: {elapsed:?} against {alone:?} alone"
                    ));
                }
            }
        }
        assert!(failures.is_empty(), "{failures:#?}");
    }

    // The same deep stacks with the budgets lifted: what a heavy entry adds is nothing beyond what
    // the depth costs html5ever.
    #[test]
    #[ignore = "slow"]
    fn deep_stacks_cost_the_same_with_attributes_and_the_budgets_lifted() {
        let tail = "</x>".repeat(DEFAULT_MAX_INPUT_BYTES / 4);
        let unlimited = ParseLimits {
            max_nodes: None,
            max_input_bytes: None,
            ..ParseLimits::default()
        };
        let time = |html: &str| {
            let start = std::time::Instant::now();
            parse_document(html, &unlimited, &Call::default()).unwrap();
            start.elapsed()
        };
        for entries in [43, 50, 250] {
            let mut seed = 0x2545_F491_4F6C_DD1D_u64;
            let heavy: String = (0..entries)
                .map(|entry| {
                    let unique = format!("u{entry}");
                    shuffled_tag("b", MAX_TAG_ATTRIBUTES, "a", "", &unique, &mut seed)
                })
                .collect();
            let light: String = (0..entries).map(|entry| format!("<b u{entry}>")).collect();
            let light_time = time(&format!("<html><body>{light}{tail}"));
            let heavy_time = time(&format!("<html><body>{heavy}{tail}"));
            println!(
                "{entries} entries, </x> to the cap: heavy {heavy_time:?}, light {light_time:?}"
            );
            assert!(heavy_time < light_time * 3 / 2, "{entries} entries");
        }
    }

    // The other exits of the adoption agency either remove the entry or are the plain cost of a tag:
    // with a heavy open formatting element a page of end tags takes as long as without attributes.
    #[test]
    #[ignore = "slow"]
    fn end_tags_that_drop_the_entry_cost_the_same_with_a_heavy_element() {
        let bytes = 16 * 1024 * 1024;
        let time = |html: &str| {
            let start = std::time::Instant::now();
            let limits = ParseLimits {
                max_input_bytes: None,
                ..ParseLimits::default()
            };
            parse_document(html, &limits, &Call::default()).unwrap();
            start.elapsed()
        };
        for open in ["<div>", "<div><div>", "<p>x", "<button>", "<table><tr><td>"] {
            let heavy = time(&filled(
                &format!("<html><body>{}{open}", heavy_tag("b", 0)),
                "</b>",
                bytes,
            ));
            let light = time(&filled(&format!("<html><body><b>{open}"), "</b>", bytes));
            assert!(
                heavy < light * 2,
                "{open}: heavy {heavy:?}, light {light:?}"
            );
        }
    }

    // The not-in-scope end tag is charged through html5ever's parse error: this fails if the message
    // changes, which would silently uncharge the clone each such end tag makes.
    #[test]
    fn not_in_scope_parse_error_message_is_charged() {
        let open = format!("<html><body>{}<table>", heavy_tag("b", 0));
        let (_, without) = compare_outcome(&open, 500_000);
        let (_, with) = compare_outcome(&format!("{open}</b>"), 500_000);
        assert_eq!(with - without, MAX_TAG_ATTRIBUTES + PAIR_UNITS);
        // A scoped end tag is an ordinary close.
        let (_, closed) =
            compare_outcome(&format!("<html><body>{}</b>", heavy_tag("b", 0)), 500_000);
        let (_, open_only) =
            compare_outcome(&format!("<html><body>{}", heavy_tag("b", 0)), 500_000);
        assert_eq!(closed, open_only);
    }

    // The 14 names html5ever 0.40.1 keeps in its list of active formatting elements (`rules.rs`).
    const FORMATTING_NAMES: [&str; 14] = [
        "a", "b", "big", "code", "em", "font", "i", "nobr", "s", "small", "strike", "strong", "tt",
        "u",
    ];

    // The element names of the HTML Standard, obsolete ones included, plus the legacy `image` and
    // `isindex` html5ever's tree builder special-cases. The test covers the names listed here only:
    // a name missing from this table is not checked.
    #[rustfmt::skip]
    const HTML_ELEMENT_NAMES: &[&str] = &[
        "a", "abbr", "acronym", "address", "applet", "area", "article", "aside", "audio", "b",
        "base", "basefont", "bdi", "bdo", "bgsound", "big", "blink", "blockquote", "body", "br",
        "button", "canvas", "caption", "center", "cite", "code", "col", "colgroup", "command",
        "data", "datalist", "dd", "del", "details", "dfn", "dialog", "dir", "div", "dl", "dt", "em",
        "embed", "fencedframe", "fieldset", "figcaption", "figure", "font", "footer", "form",
        "frame", "frameset", "h1", "h2", "h3", "h4", "h5", "h6", "head", "header", "hgroup", "hr",
        "html", "i", "iframe", "image", "img", "input", "ins", "isindex", "kbd", "keygen", "label",
        "legend", "li", "link", "listing", "main", "map", "mark", "marquee", "math", "menu",
        "menuitem", "meta", "meter", "multicol", "nav", "nextid", "nobr", "noembed", "noframes",
        "noscript", "object", "ol", "optgroup", "option", "output", "p", "param", "picture",
        "plaintext", "pre", "progress", "q", "rb", "rp", "rt", "rtc", "ruby", "s", "samp", "script",
        "search", "section", "select", "selectedcontent", "slot", "small", "source", "spacer",
        "span", "strike", "strong", "style", "sub", "summary", "sup", "svg", "table", "tbody", "td",
        "template", "textarea", "tfoot", "th", "thead", "time", "title", "tr", "track", "tt", "u",
        "ul", "var", "video", "wbr", "xmp",
    ];

    #[test]
    fn is_formatting_is_exactly_html5evers_list() {
        let html = |name: &str| QualName::new(None, ns!(html), LocalName::from(name));
        for name in FORMATTING_NAMES {
            assert!(is_formatting(&html(name)), "{name}");
        }
        for name in HTML_ELEMENT_NAMES {
            assert_eq!(
                is_formatting(&html(name)),
                FORMATTING_NAMES.contains(name),
                "{name}"
            );
        }
        for name in ["strike2", "bb", ""] {
            assert!(!is_formatting(&html(name)), "{name}");
        }
        let svg_a = QualName::new(None, ns!(svg), local_name!("a"));
        assert!(!is_formatting(&svg_a));
    }

    // The compare is charged per formatting start tag, by name: a name missing from `is_formatting`
    // would go uncharged. `a` and `nobr` start tags make html5ever close an open element of the same
    // name first, so an `applet` marker keeps the heavy one open for them.
    #[test]
    fn every_formatting_start_tag_is_charged_a_compare() {
        for name in FORMATTING_NAMES {
            let open = format!("<html><body>{}", heavy_tag(name, 0));
            let (_, base) = compare_outcome(&open, 500_000);
            let (_, plain) = compare_outcome(&format!("{open}<{name}>"), 500_000);
            let (_, marked) = compare_outcome(&format!("{open}<applet><{name}>"), 500_000);
            let (_, applet) = compare_outcome(&format!("{open}<applet>"), 500_000);
            assert!(
                marked > applet,
                "{name}: {marked} with the tag, {applet} without"
            );
            if name == "a" || name == "nobr" {
                assert_eq!(plain, base, "{name} closes the open one first");
            } else {
                assert!(plain > base, "{name}: {plain} with the tag, {base} without");
            }
        }
    }

    // A formatting start tag whose own element the depth cut leaves unplaced is charged nothing. The
    // page: a heavy `b`, a `b` that `</p>` leaves only in the active formatting list, `MAX_DEPTH - 4`
    // nested `div`s, then `<b>`. Reconstruction clones the listed `b` under the last `div`, at
    // `MAX_DEPTH`, and the tag's own `b` has no room. The clone sits under the heavy `b`, so charging
    // it in the tag's place would cost as much again and could reject a page that is only too deep.
    #[test]
    fn unplaced_formatting_element_is_charged_nothing() {
        let prefix = format!(
            "<html><body>{}<p><b></p>{}",
            heavy_tag("b", 0),
            "<div>".repeat(MAX_DEPTH - 4)
        );
        let html = format!("{prefix}<b>");
        let (_, before) = compare_outcome(&prefix, 500_000);
        // The budget covers the prefix and is below the prefix plus a second charge of the same size.
        let max_nodes = before.div_ceil(COMPARE_WORK_PER_NODE);
        assert!(before > 2 * PAIR_UNITS && max_nodes * COMPARE_WORK_PER_NODE < 2 * before);

        let call = Call::default();
        let driver = Driver::new(new_sink(&limited(max_nodes), &call, false), false);
        driver.process(&html);
        {
            let inner = driver.sink().inner.borrow();
            let b = local_name!("b");
            let unplaced = |node: &&SinkNode| {
                node.kind == Kind::Element && node.qual.local == b && node.parent.is_none()
            };
            assert!(inner.stopped, "the depth cut stops the parse");
            assert_eq!(inner.nodes.iter().filter(unplaced).count(), 1);
            assert_eq!(inner.abort, None);
            assert_eq!(inner.compare_work, before);
            let deepest = (0..inner.nodes.len())
                .filter_map(|index| u32::try_from(index).ok())
                .filter(|&id| {
                    inner.node(id).kind == Kind::Element && inner.node(id).parent.is_some()
                })
                .map(|id| inner.depth(id))
                .max();
            assert_eq!(deepest, Some(MAX_DEPTH));
        }

        assert!(parse_document(&html, &limited(max_nodes), &call).is_ok());
    }

    // A real page: 20,000 paragraphs, each with an unclosed `font`. Noah's Ark keeps three entries,
    // so a paragraph costs 210 units, 21 percent of the budget in all.
    #[test]
    fn unclosed_font_page_of_20000_paragraphs_parses() {
        let paragraph = "<p><font color=\"#000000\" size=2 face=Arial>text of the paragraph\n</p>";
        let html = format!("<html><body>{}", paragraph.repeat(20_000));
        let tree = parse_document(&html, &ParseLimits::default(), &Call::default()).unwrap();
        assert!(tree.len() > 80_000, "{} nodes", tree.len());
        let (abort, work) = compare_outcome(&html, DEFAULT_MAX_NODES);
        assert_eq!(abort, None);
        assert!(
            work * 2 < DEFAULT_MAX_NODES * COMPARE_WORK_PER_NODE,
            "{work} units"
        );
    }

    /// `count` paragraphs, each with an unclosed `<font color=cN size=2>`, the colors cycling.
    fn font_paragraphs(count: usize, colors: usize) -> String {
        let body: String = (0..count)
            .map(|i| format!("<p><font color=c{} size=2>t</p>", i % colors))
            .collect();
        format!("<html><body>{body}")
    }

    // Reconstruction and adoption-agency clones run no Noah's Ark compare, so they are not charged
    // as one: pages of unclosed formatting elements that differ in an attribute stay parseable.
    #[test]
    fn unclosed_font_pages_with_varying_attributes_parse() {
        for (count, colors) in [(200, 200), (2_000, 10), (500, 20)] {
            let html = font_paragraphs(count, colors);
            let tree = parse(&html, false)
                .unwrap_or_else(|error| panic!("{count} paragraphs, {colors} colors: {error:?}"));
            assert_eq!(
                dump(&tree, DumpMode::Exact),
                dump(&whole_tree(&html, false), DumpMode::Exact),
                "{count} paragraphs, {colors} colors"
            );
        }
    }

    // html5ever cannot stop inside a piece, so the gate drops the tokens after the abort: fed whole,
    // 1,000 paragraphs create the 20 elements the limit allows, one more, and no more.
    #[test]
    fn tokens_after_the_abort_do_not_reach_the_tree_builder() {
        let call = Call::default();
        let html = format!("<html><body>{}", "<p>x".repeat(1_000));
        for fragment in [false, true] {
            let driver = Driver::new(new_sink(&limited(20), &call, fragment), fragment);
            driver.process(&html);
            let inner = driver.sink().inner.borrow();
            assert_eq!(inner.abort, Some(TOO_LARGE), "fragment {fragment}");
            assert!(inner.stopped);
            assert_eq!(inner.elements, 21, "fragment {fragment}");
        }
    }

    // The charge counts same-name open formatting elements as the ancestors of the new element.
    // html5ever's list holds them as open elements (reconstruction runs first), and they are
    // ancestors even across foster parenting and the adoption agency. The page is fed one tag at a
    // time and, after every formatting start tag, each same-name formatting element html5ever still
    // references (open stack and list, `trace_handles`) must be an ancestor of the new element. The
    // pieces hold no marker (`td`, `th`, `caption`, `template`, `applet`, `object`, `marquee`), whose
    // entries html5ever stops comparing at.
    #[test]
    fn same_name_formatting_elements_are_ancestors_of_the_new_one() {
        struct Handles(RefCell<Vec<u32>>);
        impl html5ever::interface::Tracer for Handles {
            type Handle = u32;
            fn trace_handle(&self, node: &u32) {
                self.0.borrow_mut().push(*node);
            }
        }
        let formatting = [
            ("<b>", "b"),
            ("<b x=1>", "b"),
            ("<b x=2>", "b"),
            ("<i>", "i"),
            ("<a href=x>", "a"),
            ("<a>", "a"),
            ("<font color=r>", "font"),
            ("<u>", "u"),
            ("<nobr>", "nobr"),
            ("<em>", "em"),
        ];
        let others = [
            "</b>",
            "</i>",
            "</a>",
            "</font>",
            "</u>",
            "</nobr>",
            "</em>",
            "<p>",
            "</p>",
            "<div>",
            "</div>",
            "<table>",
            "</table>",
            "<tbody>",
            "<tr>",
            "<span>",
            "</span>",
            "x",
            "<br>",
            "<li>",
            "<ul>",
            "</ul>",
            "<h1>",
            "</h1>",
            "<form>",
            "<button>",
            "</button>",
            "<svg>",
            "</svg>",
            "<select>",
            "<option>",
            "</select>",
            "<pre>",
            "<center>",
        ];
        let mut seed = 0x2545_F491_4F6C_DD1D_u64;
        let mut next = |bound: usize| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            usize::try_from(seed % bound as u64).unwrap()
        };
        let mut checked = 0usize;
        for round in 0..3_000 {
            let call = Call::default();
            let limits = ParseLimits {
                max_nodes: None,
                ..ParseLimits::default()
            };
            let mut parser =
                html5ever::parse_document(new_sink(&limits, &call, false), parse_opts());
            let mut fed = String::new();
            for _ in 0..40 {
                let (piece, name) = if next(2) == 0 {
                    let (piece, name) = formatting[next(formatting.len())];
                    (piece, Some(name))
                } else {
                    (others[next(others.len())], None)
                };
                fed.push_str(piece);
                parser.process(StrTendril::from_slice(piece));
                let Some(name) = name else { continue };
                let handles = Handles(RefCell::new(Vec::new()));
                parser.tokenizer.sink.trace_handles(&handles);
                let inner = parser.tokenizer.sink.sink.inner.borrow();
                let Some(new) = inner.nodes.len().checked_sub(1) else {
                    continue;
                };
                let new = u32::try_from(new).unwrap();
                let node = inner.node(new);
                if node.kind != Kind::Element || &*node.qual.local != name {
                    continue;
                }
                let mut ancestors = HashSet::new();
                let mut up = node.parent;
                while let Some(id) = up {
                    ancestors.insert(id);
                    up = inner.node(id).parent;
                }
                if ancestors.is_empty() {
                    continue;
                }
                checked += 1;
                for &handle in handles.0.borrow().iter() {
                    let other = inner.node(handle);
                    let same_name = other.kind == Kind::Element
                        && other.qual.ns == ns!(html)
                        && &*other.qual.local == name;
                    assert!(
                        handle == new || !same_name || ancestors.contains(&handle),
                        "round {round}: {name} {handle} is not an ancestor of {new} after {fed:?}"
                    );
                }
            }
        }
        assert!(checked > 20_000, "{checked} formatting tags checked");
    }

    #[test]
    fn long_script_text_dropped_across_chunks() {
        let html = format!("<script>{}</script><p>x</p>", "a".repeat(CHUNK_BYTES * 3));
        let limits = ParseLimits {
            drop_script_text: true,
            ..ParseLimits::default()
        };
        let tree = parse_document(&html, &limits, &Call::default()).unwrap();
        let text_of = |name: &str| {
            let mut stack = vec![tree.root()];
            while let Some(id) = stack.pop() {
                if &*tree.node(id).name == name {
                    return tree.node(id).text.clone();
                }
                stack.extend(tree.children(id));
            }
            panic!("no element named {name}");
        };
        assert_eq!(text_of("script"), None);
        assert_eq!(text_of("p"), Some("x".to_string()));
        assert!(tree.dropped_script_text);
    }
}
