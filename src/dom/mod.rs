//! The document tree: an arena of elements in lxml's text/tail model, filled by an html5ever tree
//! sink (`parse`) and printed in the form the parity harness compares (`dump`).
//!
//! Items are `pub` inside this private module; they leave the crate only through the hidden
//! `__internals` module.

mod dump;
#[cfg(test)]
mod html_cases;
mod mutate;
mod parse;
mod scan;
mod serialize;
mod walk;

use std::sync::{Arc, LazyLock};

pub use html5ever::LocalName;
pub use parse::{ParseLimits, parse_document, parse_fragment};
pub use walk::LiveIter;

#[cfg(any(feature = "__internals", test))]
pub use dump::{DumpMode, dump, dump_from};
// The depth test of the output walks builds trees past it.
#[cfg(test)]
pub use parse::MAX_DEPTH;

/// The value of a valueless attribute whose name `htmlBooleanAttrs` does not list. It is one allocation,
/// told apart from `""` by identity: the HTML method writes such an attribute as its bare name.
static VALUELESS: LazyLock<Arc<str>> = LazyLock::new(|| Arc::from(""));

/// The shared valueless value (`VALUELESS`).
// Unread until the tree builder is the parse entry point.
#[allow(dead_code)]
pub(crate) fn valueless() -> Arc<str> {
    Arc::clone(&VALUELESS)
}

/// Whether `value` is the shared valueless value, by identity.
pub(crate) fn is_valueless_value(value: &Arc<str>) -> bool {
    Arc::ptr_eq(value, &VALUELESS)
}

/// Index of a node in its `Tree`; only valid for the tree that produced it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

/// Namespace of an element.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ns {
    /// HTML elements.
    Html,
    /// SVG elements.
    Svg,
    /// MathML elements.
    MathMl,
}

/// An element with its lxml-style `text` (before the first child) and `tail` (after its end tag).
#[derive(Clone, Debug)]
pub struct Node {
    /// Local name, without prefix.
    pub name: LocalName,
    /// Namespace.
    pub ns: Ns,
    /// Attributes in source order, by local name.
    pub attrs: Vec<(LocalName, Arc<str>)>,
    /// Text before the first child element.
    pub text: Option<String>,
    /// Text after this element's end tag, up to the next sibling element or the parent's end.
    pub tail: Option<String>,
    parent: Option<NodeId>,
    first_child: Option<NodeId>,
    last_child: Option<NodeId>,
    prev: Option<NodeId>,
    next: Option<NodeId>,
}

impl Node {
    fn new(name: LocalName, ns: Ns, attrs: Vec<(LocalName, Arc<str>)>) -> Self {
        Self {
            name,
            ns,
            attrs,
            text: None,
            tail: None,
            parent: None,
            first_child: None,
            last_child: None,
            prev: None,
            next: None,
        }
    }

    /// The parent element. Only a `document_roots` entry or a detached element has none.
    pub fn parent(&self) -> Option<NodeId> {
        self.parent
    }

    /// The next sibling element.
    pub fn next_sibling(&self) -> Option<NodeId> {
        self.next
    }

    /// The previous sibling element.
    pub fn prev_sibling(&self) -> Option<NodeId> {
        self.prev
    }
}

/// An element tree: an arena of elements with a root, and the document-level elements it was parsed into.
/// Nodes are never freed before the tree is dropped; a node outside the root's subtree stays in the arena.
#[derive(Clone, Debug)]
pub struct Tree {
    nodes: Vec<Node>,
    /// The element `root` returns. It is `NodeId(0)` unless `set_root` moved it.
    root: NodeId,
    /// Cells `main_extractor` table construction has minted here; copies of the tree share it
    /// because they live in the same arena (D-9).
    pub(crate) table_cells: usize,
    /// The parse dropped the text of at least one unread `style` or `script`
    /// (`ParseLimits::drop_script_text`).
    pub(crate) dropped_script_text: bool,
    /// A dropped-text element survived `tree_cleaning`, so the readers after it would see its
    /// missing text and the extraction must run again on a full parse.
    pub(crate) needs_script_text: bool,
    /// libxml2's document-level elements in order: the first `html`, the root, then each `html` opened
    /// after the first one closed (implied or written). None of them has a parent.
    pub(crate) document_roots: Vec<NodeId>,
}

impl Tree {
    /// A tree holding only a root element.
    pub fn new(name: LocalName, ns: Ns, attrs: Vec<(LocalName, Arc<str>)>) -> Self {
        Self {
            nodes: vec![Node::new(name, ns, attrs)],
            root: NodeId(0),
            table_cells: 0,
            dropped_script_text: false,
            needs_script_text: false,
            document_roots: vec![NodeId(0)],
        }
    }

    /// The root element. It may sit below a document-level element (`document_roots`).
    pub fn root(&self) -> NodeId {
        self.root
    }

    /// Makes `id` the root that `root` returns. The nodes outside it keep their links.
    // Unread until the fromstring port is the parse entry point.
    #[allow(dead_code)]
    pub(crate) fn set_root(&mut self, id: NodeId) {
        self.root = id;
    }

    /// Whether attribute `name` of `id` holds the valueless sentinel (`valueless`) rather than a value
    /// that equals it.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn is_valueless(&self, id: NodeId, name: &str) -> bool {
        self.node(id)
            .attrs
            .iter()
            .find(|(key, _)| &**key == name)
            .is_some_and(|(_, value)| is_valueless_value(value))
    }

    /// The element behind `id`.
    #[allow(clippy::indexing_slicing)] // ids are minted by this tree and nodes are never removed
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0 as usize]
    }

    /// The element behind `id`, for editing `text`, `tail`, `attrs` and `name`. The links between
    /// nodes change only through `append`, `insert`, `add_next`, `add_previous` and `remove`.
    #[allow(clippy::indexing_slicing)] // same invariant as `node`
    pub fn node_mut(&mut self, id: NodeId) -> &mut Node {
        &mut self.nodes[id.0 as usize]
    }

    /// Number of elements in the tree, root included; a tree is never empty.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Child elements of `id` in document order.
    pub fn children(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(self.node(id).first_child, |&child| self.node(child).next)
    }

    /// Appends a new element as the last child of `parent`.
    pub fn add_child(
        &mut self,
        parent: NodeId,
        name: LocalName,
        ns: Ns,
        attrs: Vec<(LocalName, Arc<str>)>,
    ) -> NodeId {
        let id = self.push(Node::new(name, ns, attrs));
        self.link_last(parent, id);
        id
    }

    /// Adds a detached element as the next document-level root, after the ones there are.
    pub(crate) fn add_document_root(
        &mut self,
        name: LocalName,
        ns: Ns,
        attrs: Vec<(LocalName, Arc<str>)>,
    ) -> NodeId {
        let id = self.push(Node::new(name, ns, attrs));
        self.document_roots.push(id);
        id
    }

    /// Stores a detached node; its id saturates because the parser rejects or stops long before
    /// u32::MAX elements.
    fn push(&mut self, node: Node) -> NodeId {
        let id = NodeId(u32::try_from(self.nodes.len()).unwrap_or(u32::MAX));
        self.nodes.push(node);
        id
    }

    /// Appends character data at the end of `parent`'s content: its `text` while it has no child
    /// element, else the `tail` of its last child.
    #[cfg_attr(not(feature = "__internals"), allow(dead_code))]
    pub fn add_text(&mut self, parent: NodeId, text: &str) {
        if text.is_empty() {
            return;
        }
        self.text_target(parent)
            .get_or_insert_with(String::new)
            .push_str(text);
    }

    /// `add_text` for a `String` the caller no longer needs: an empty target takes the allocation
    /// itself instead of a copy of it.
    pub fn add_text_owned(&mut self, parent: NodeId, text: String) {
        if text.is_empty() {
            return;
        }
        match self.text_target(parent) {
            Some(existing) => existing.push_str(&text),
            target @ None => {
                let mut text = text;
                // The sink grows text piece by piece, up to twice its length, and the raw tree
                // lives through the whole extraction.
                if text.capacity() > text.len() {
                    text.shrink_to_fit();
                }
                *target = Some(text);
            }
        }
    }

    /// Makes room for `additional` more elements in the arena.
    pub fn reserve(&mut self, additional: usize) {
        self.nodes.reserve(additional);
    }

    /// Where character data at the end of `parent`'s content goes.
    fn text_target(&mut self, parent: NodeId) -> &mut Option<String> {
        match self.node(parent).last_child {
            Some(last) => &mut self.node_mut(last).tail,
            None => &mut self.node_mut(parent).text,
        }
    }
}

#[cfg(test)]
mod tests;
