//! Tree walks in lxml's order and `copy`. Every walk follows the parent and sibling links with
//! explicit state, so the stack never grows with the depth of the tree.

use super::{Node, NodeId, Tree};

/// A live `tree.iter(tags)`: it reads the tree on each step, so the caller may edit the tree
/// between steps. As in lxml, the successor of a node is computed before the node is returned.
/// When the caller then removes that node with its subtree, the walk still continues at the
/// precomputed successor, and the nodes below the removed one are never visited. The walk ends
/// when it climbs above `start` or above the root of the subtree the successor lies in.
#[derive(Clone, Debug)]
pub struct LiveIter {
    start: NodeId,
    next: Option<NodeId>,
    tags: &'static [&'static str],
}

impl LiveIter {
    /// Walks `start` and its descendants in document order, yielding the elements named in `tags`
    /// (every element when `tags` is empty).
    pub fn new(tree: &Tree, start: NodeId, tags: &'static [&'static str]) -> Self {
        let next = std::iter::successors(Some(start), |&id| tree.step(id, start))
            .find(|&id| tree.matches(id, tags));
        Self { start, next, tags }
    }

    /// The next element; its successor is fixed before it is returned, as in lxml.
    pub fn next(&mut self, tree: &Tree) -> Option<NodeId> {
        let current = self.next?;
        self.next = std::iter::successors(tree.step(current, self.start), |&id| {
            tree.step(id, self.start)
        })
        .find(|&id| tree.matches(id, self.tags));
        Some(current)
    }
}

impl Tree {
    /// Whether the name of `id` is one of `tags`; an empty list matches every element.
    pub(super) fn matches(&self, id: NodeId, tags: &[&str]) -> bool {
        let name = &*self.node(id).name;
        tags.is_empty() || tags.contains(&name)
    }

    /// The node after `current` in document order inside the subtree of `start`: its first child,
    /// else the first node `after` it.
    pub(super) fn step(&self, current: NodeId, start: NodeId) -> Option<NodeId> {
        self.node(current)
            .first_child
            .or_else(|| self.after(current, start))
    }

    /// The node after the whole subtree of `current` in document order, `None` at the end of the
    /// subtree of `start` or at the root of a detached subtree.
    pub(super) fn after(&self, current: NodeId, start: NodeId) -> Option<NodeId> {
        let mut up = current;
        loop {
            if up == start {
                return None;
            }
            let node = self.node(up);
            if node.next.is_some() {
                return node.next;
            }
            up = node.parent?;
        }
    }

    /// `id` and its descendants in document order, `id` first.
    fn walk(&self, id: NodeId) -> impl Iterator<Item = NodeId> + '_ {
        std::iter::successors(Some(id), move |&node| self.step(node, id))
    }

    /// `id` and its descendants in document order, only those named in `tags`; every element when
    /// `tags` is empty. For a walk that edits the tree use `LiveIter`.
    pub fn iter<'a>(
        &'a self,
        id: NodeId,
        tags: &'a [&'a str],
    ) -> impl Iterator<Item = NodeId> + 'a {
        self.walk(id).filter(move |&node| self.matches(node, tags))
    }

    /// The descendants of `id` in document order, `id` itself excluded; filtered like `iter`.
    pub fn iterdescendants<'a>(
        &'a self,
        id: NodeId,
        tags: &'a [&'a str],
    ) -> impl Iterator<Item = NodeId> + 'a {
        self.walk(id)
            .skip(1)
            .filter(move |&node| self.matches(node, tags))
    }

    /// The ancestors of `id`, parent first, `id` itself excluded; filtered like `iter`.
    pub fn iterancestors<'a>(
        &'a self,
        id: NodeId,
        tags: &'a [&'a str],
    ) -> impl Iterator<Item = NodeId> + 'a {
        std::iter::successors(self.node(id).parent, |&up| self.node(up).parent)
            .filter(move |&node| self.matches(node, tags))
    }

    /// The text of `id` and its descendants in document order: own text, then each descendant's
    /// text and tail. The tail of `id` itself is not part of it.
    pub fn itertext(&self, id: NodeId) -> impl Iterator<Item = &str> + '_ {
        // `(node, true)` is the end of `node`, where its tail comes.
        std::iter::successors(Some((id, false)), move |&(node, ended)| {
            let data = self.node(node);
            match (ended, data.first_child) {
                (false, Some(child)) => Some((child, false)),
                (false, None) => Some((node, true)),
                (true, _) if node == id => None,
                (true, _) => match data.next {
                    Some(next) => Some((next, false)),
                    None => data.parent.map(|parent| (parent, true)),
                },
            }
        })
        .filter_map(move |(node, ended)| match (ended, node == id) {
            (false, _) => self.node(node).text.as_deref(),
            (true, false) => self.node(node).tail.as_deref(),
            (true, true) => None,
        })
        .filter(|text| !text.is_empty())
    }

    /// All text under `id`, as lxml's `text_content()`.
    pub fn text_content(&self, id: NodeId) -> String {
        self.itertext(id).collect()
    }

    /// The first descendant of `id` named `tag`, as `.//tag`; `id` itself is not considered.
    pub fn find(&self, id: NodeId, tag: &str) -> Option<NodeId> {
        self.iterdescendants(id, &[tag]).next()
    }

    /// A deep copy of `id` and its subtree as a detached tree, tail included, like lxml's
    /// `copy.deepcopy`. The copy is one contiguous block of new nodes in document order, so the
    /// k-th node of the copy pairs with the k-th node of the original. Returns the copy of `id`.
    pub fn copy(&mut self, id: NodeId) -> NodeId {
        self.nodes.reserve(self.walk(id).count());
        let root = self.copy_node(id);
        // The copies of the ancestors of the node being copied, up to the copy of `id`.
        let mut path = vec![root];
        let mut current = self.node(id).first_child;
        while let Some(node) = current {
            let copied = self.copy_node(node);
            if let Some(&parent) = path.last() {
                self.link_last(parent, copied);
            }
            current = match self.node(node).first_child {
                Some(child) => {
                    path.push(copied);
                    Some(child)
                }
                None => self.next_outside(node, id, &mut path),
            };
        }
        root
    }

    /// The next node to copy after the leaf `node`, climbing out of finished parents and popping
    /// their copies off `path`; `None` once the subtree of `start` is done.
    fn next_outside(&self, node: NodeId, start: NodeId, path: &mut Vec<NodeId>) -> Option<NodeId> {
        let mut up = node;
        loop {
            if let Some(next) = self.node(up).next {
                return Some(next);
            }
            let parent = self.node(up).parent?;
            if parent == start {
                return None;
            }
            path.pop();
            up = parent;
        }
    }

    /// A detached node with the data of `id` and none of its links.
    fn copy_node(&mut self, id: NodeId) -> NodeId {
        let source = self.node(id);
        let mut node = Node::new(source.name.clone(), source.ns, source.attrs.clone());
        node.text.clone_from(&source.text);
        node.tail.clone_from(&source.tail);
        self.push(node)
    }
}
