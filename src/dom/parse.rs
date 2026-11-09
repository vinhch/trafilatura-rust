//! html5ever tree sink over the arena, and the two parse entry points.
//!
//! html5ever calls the sink through `&self`, so the working tree lives in a `RefCell` that is
//! borrowed for the length of one sink call only. The sink keeps text as separate nodes while
//! parsing (the adoption agency and foster parenting move elements, never the text around them)
//! and folds them into the text/tail model of `Tree` in `finish`.

use std::collections::{HashMap, HashSet};
use std::mem;
use std::sync::Arc;

use html5ever::interface::tree_builder::{ElemName, NodeOrText};
use html5ever::{LocalName, Namespace, QualName, local_name, ns};

use super::{Ns, Tree};
use crate::cancel::Call;
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

mod sink;

use sink::{Driver, drive, new_sink};

#[cfg(test)]
mod tests;
