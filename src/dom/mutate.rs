//! Tree creation, moves and removal with lxml's text/tail semantics: the tail travels with a node
//! on every move, `remove` leaves it on the detached node, and a move under the node itself or one
//! of its descendants is refused. Nodes are only detached, never freed.

use std::sync::Arc;

use super::{LocalName, Node, NodeId, Ns, Tree};

impl Tree {
    /// A detached element in the HTML namespace, without attributes.
    pub fn new_element(&mut self, name: &str) -> NodeId {
        self.push(Node::new(LocalName::from(name), Ns::Html, Vec::new()))
    }

    /// The first child element of `id`.
    pub fn first_child(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).first_child
    }

    /// The last child element of `id`.
    pub fn last_child(&self, id: NodeId) -> Option<NodeId> {
        self.node(id).last_child
    }

    /// Number of child elements of `id`.
    pub fn child_count(&self, id: NodeId) -> usize {
        self.children(id).count()
    }

    /// The child element of `id` at `index`, `None` past the end.
    pub fn child_at(&self, id: NodeId, index: usize) -> Option<NodeId> {
        self.children(id).nth(index)
    }

    /// Whether `ancestor` is a proper ancestor of `node`: `node` itself does not count.
    pub fn is_ancestor(&self, ancestor: NodeId, node: NodeId) -> bool {
        std::iter::successors(self.node(node).parent, |&up| self.node(up).parent)
            .any(|up| up == ancestor)
    }

    /// Moves `child` to the end of `parent`'s children, tail included. Does nothing when `child`
    /// is `parent` or one of its ancestors.
    pub fn append(&mut self, parent: NodeId, child: NodeId) {
        if self.would_cycle(child, parent) {
            return;
        }
        self.detach(child);
        self.link_last(parent, child);
    }

    /// Moves `child` to position `index` among `parent`'s children, tail included; an index past
    /// the end appends. The position is resolved before the move, so inserting a child of `parent`
    /// counts it at its old place, as lxml does. Does nothing when `child` is `parent` or one of
    /// its ancestors.
    pub fn insert(&mut self, parent: NodeId, index: usize, child: NodeId) {
        if self.would_cycle(child, parent) {
            return;
        }
        match self.child_at(parent, index) {
            Some(at) if at == child => {}
            Some(at) => {
                self.detach(child);
                self.link_before(at, child);
            }
            None => {
                self.detach(child);
                self.link_last(parent, child);
            }
        }
    }

    /// Moves `node` to just after `anchor`, tail included; the anchor keeps its own tail, as in
    /// lxml. Does nothing when `anchor` has no parent or when `node` is `anchor` or one of its
    /// ancestors.
    pub fn add_next(&mut self, anchor: NodeId, node: NodeId) {
        let Some(parent) = self.node(anchor).parent else {
            return;
        };
        if self.would_cycle(node, anchor) {
            return;
        }
        self.detach(node);
        match self.node(anchor).next {
            Some(next) => self.link_before(next, node),
            None => self.link_last(parent, node),
        }
    }

    /// Moves `node` to just before `anchor`, tail included. Does nothing when `anchor` has no
    /// parent or when `node` is `anchor` or one of its ancestors.
    pub fn add_previous(&mut self, anchor: NodeId, node: NodeId) {
        if self.node(anchor).parent.is_none() || self.would_cycle(node, anchor) {
            return;
        }
        self.detach(node);
        self.link_before(anchor, node);
    }

    /// Detaches `child` from its parent. Its tail stays on it; the parent's text and the previous
    /// sibling's tail are not changed. Does nothing for a node without a parent.
    pub fn remove(&mut self, child: NodeId) {
        self.detach(child);
    }

    /// Renames the element, keeping its namespace.
    pub fn set_name(&mut self, id: NodeId, name: &str) {
        self.node_mut(id).name = LocalName::from(name);
    }

    /// The value of attribute `attr`.
    pub fn get(&self, id: NodeId, attr: &str) -> Option<&str> {
        self.node(id)
            .attrs
            .iter()
            .find(|(key, _)| &**key == attr)
            .map(|(_, value)| &**value)
    }

    /// Sets attribute `attr`: an existing one keeps its place, a new one goes last.
    pub fn set(&mut self, id: NodeId, attr: &str, value: &str) {
        let value: Arc<str> = Arc::from(value);
        let attrs = &mut self.node_mut(id).attrs;
        match attrs.iter_mut().find(|(key, _)| &**key == attr) {
            Some(slot) => slot.1 = value,
            None => attrs.push((LocalName::from(attr), value)),
        }
    }

    /// Removes attribute `attr`, returning its value.
    pub fn remove_attr(&mut self, id: NodeId, attr: &str) -> Option<Arc<str>> {
        let attrs = &mut self.node_mut(id).attrs;
        let at = attrs.iter().position(|(key, _)| &**key == attr)?;
        Some(attrs.remove(at).1)
    }

    /// Empties the element like lxml's `clear()`: text, tail and attributes are dropped and the
    /// children are detached with their tails. The element keeps its place in its parent.
    pub fn clear(&mut self, id: NodeId) {
        while let Some(child) = self.node(id).first_child {
            self.detach(child);
        }
        let node = self.node_mut(id);
        node.text = None;
        node.tail = None;
        node.attrs.clear();
    }

    /// Whether moving `node` under `target` (or next to it) would put a node inside itself.
    fn would_cycle(&self, node: NodeId, target: NodeId) -> bool {
        node == target || self.is_ancestor(node, target)
    }

    /// Unlinks `id` from its parent and siblings; its own links are cleared.
    fn detach(&mut self, id: NodeId) {
        let node = self.node(id);
        let (Some(parent), prev, next) = (node.parent, node.prev, node.next) else {
            return;
        };
        match prev {
            Some(prev) => self.node_mut(prev).next = next,
            None => self.node_mut(parent).first_child = next,
        }
        match next {
            Some(next) => self.node_mut(next).prev = prev,
            None => self.node_mut(parent).last_child = prev,
        }
        let node = self.node_mut(id);
        node.parent = None;
        node.prev = None;
        node.next = None;
    }

    /// Links a detached `id` as the last child of `parent`.
    pub(super) fn link_last(&mut self, parent: NodeId, id: NodeId) {
        let last = self.node(parent).last_child;
        let node = self.node_mut(id);
        node.parent = Some(parent);
        node.prev = last;
        node.next = None;
        match last {
            Some(last) => self.node_mut(last).next = Some(id),
            None => self.node_mut(parent).first_child = Some(id),
        }
        self.node_mut(parent).last_child = Some(id);
    }

    /// Links a detached `id` just before `at`, which has a parent.
    fn link_before(&mut self, at: NodeId, id: NodeId) {
        let (parent, prev) = (self.node(at).parent, self.node(at).prev);
        let node = self.node_mut(id);
        node.parent = parent;
        node.prev = prev;
        node.next = Some(at);
        match (prev, parent) {
            (Some(prev), _) => self.node_mut(prev).next = Some(id),
            (None, Some(parent)) => self.node_mut(parent).first_child = Some(id),
            (None, None) => {}
        }
        self.node_mut(at).prev = Some(id);
    }
}
