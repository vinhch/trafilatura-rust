//! The document tree: an arena of elements in lxml's text/tail model, filled by an html5ever tree
//! sink (`parse`) and printed in the form the parity harness compares (`dump`).
//!
//! Items are `pub` inside this private module; they leave the crate only through the hidden
//! `__internals` module.

mod dump;
mod parse;

use std::sync::Arc;

pub use html5ever::LocalName;
pub use parse::{ParseLimits, parse_document};

// Only the `__internals` module and the tests use these until the pipeline lands.
#[cfg(any(feature = "__internals", test))]
pub use dump::{DumpMode, dump};
#[cfg(any(feature = "__internals", test))]
pub use parse::parse_fragment;

/// Index of a node in its `Tree`; only valid for the tree that produced it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NodeId(u32);

/// Namespace of an element.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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

    /// The parent element, `None` for the root.
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

/// An element tree with one root; nodes are never freed before the tree is dropped.
#[derive(Clone, Debug)]
pub struct Tree {
    nodes: Vec<Node>,
}

impl Tree {
    /// A tree holding only a root element.
    pub fn new(name: LocalName, ns: Ns, attrs: Vec<(LocalName, Arc<str>)>) -> Self {
        Self {
            nodes: vec![Node::new(name, ns, attrs)],
        }
    }

    /// The root element.
    pub fn root(&self) -> NodeId {
        NodeId(0)
    }

    /// The element behind `id`.
    #[allow(clippy::indexing_slicing)] // ids are minted by this tree and nodes are never removed
    pub fn node(&self, id: NodeId) -> &Node {
        &self.nodes[id.0 as usize]
    }

    #[allow(clippy::indexing_slicing)] // same invariant as `node`
    fn node_mut(&mut self, id: NodeId) -> &mut Node {
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
        // Saturating: the parser rejects or stops long before u32::MAX elements.
        let id = NodeId(u32::try_from(self.nodes.len()).unwrap_or(u32::MAX));
        let mut node = Node::new(name, ns, attrs);
        node.parent = Some(parent);
        node.prev = self.node(parent).last_child;
        self.nodes.push(node);
        match self.node(parent).last_child {
            Some(last) => self.node_mut(last).next = Some(id),
            None => self.node_mut(parent).first_child = Some(id),
        }
        self.node_mut(parent).last_child = Some(id);
        id
    }

    /// Appends character data at the end of `parent`'s content: its `text` while it has no child
    /// element, else the `tail` of its last child.
    pub fn add_text(&mut self, parent: NodeId, text: &str) {
        if text.is_empty() {
            return;
        }
        let target = match self.node(parent).last_child {
            Some(last) => &mut self.node_mut(last).tail,
            None => &mut self.node_mut(parent).text,
        };
        target.get_or_insert_with(String::new).push_str(text);
    }
}

#[cfg(test)]
mod tests;
