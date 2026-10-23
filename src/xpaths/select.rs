//! Building blocks of the selector functions: attribute reads, the XPath functions the upstream
//! expressions use, and the node walks. Each walk checks the `Call` every `CHECK_INTERVAL` nodes.
//!
//! A predicate reads only the node's own name and attributes, never walks the tree. A structural
//! test inside an XPath predicate (`not(.//h1)`) runs as a second pass over the matches, through
//! `first_descendant`, so every node loop stays cancellable.

use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

use crate::cancel::{CHECK_INTERVAL, Call};
use crate::dom::{NodeId, Tree};
use crate::error::ExtractError;

/// Counts visited nodes and checks the `Call` every `CHECK_INTERVAL` of them.
struct Ticker<'c, 'a> {
    call: &'c Call<'a>,
    visited: usize,
}

impl Ticker<'_, '_> {
    fn tick(&mut self) -> Result<(), ExtractError> {
        if self.visited.is_multiple_of(CHECK_INTERVAL) {
            self.call.check()?;
        }
        self.visited = self.visited.wrapping_add(1);
        Ok(())
    }
}

/// The value of attribute `name`, `""` when missing.
pub(crate) fn attr<'t>(tree: &'t Tree, id: NodeId, name: &str) -> &'t str {
    tree.get(id, name).unwrap_or("")
}

/// The value of the first attribute in source order whose name is in `names`, `""` when none is.
/// This is `@*[name()='a' or name()='b']` taken as a string, which reads the first node of the set.
pub(crate) fn first_attr<'t>(tree: &'t Tree, id: NodeId, names: &[&str]) -> &'t str {
    tree.node(id)
        .attrs
        .iter()
        .find(|(key, _)| names.contains(&&**key))
        .map_or("", |(_, value)| value)
}

/// Whether attribute `name` is present, whatever its value.
pub(crate) fn has_attr(tree: &Tree, id: NodeId, name: &str) -> bool {
    tree.get(id, name).is_some()
}

/// Whether `id` is an HTML element named one of `names`; SVG and MathML elements never are.
pub(crate) fn is_html(tree: &Tree, id: NodeId, names: &[&str]) -> bool {
    tree.tag(id).is_some_and(|name| names.contains(&name))
}

/// XPath `translate(value, from, to)`: each char of `from` becomes the char of `to` at the same
/// index, or is dropped when `to` is shorter; other chars stay.
pub(crate) fn translate<'s>(value: &'s str, from: &str, to: &str) -> Cow<'s, str> {
    if !value.contains(|c| from.contains(c)) {
        return Cow::Borrowed(value);
    }
    let map = |c: char| match from.chars().position(|f| f == c) {
        Some(index) => to.chars().nth(index),
        None => Some(c),
    };
    Cow::Owned(value.chars().filter_map(map).collect())
}

/// EXSLT `re:test(value, pattern)`: false when the pattern did not compile.
pub(crate) fn re_test(value: &str, pattern: &LazyLock<Option<Regex>>) -> bool {
    pattern.as_ref().is_some_and(|re| re.is_match(value))
}

/// `.//*[pred]`: the descendants of `ctx` that satisfy `pred`, in document order.
pub(crate) fn descendants(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
    pred: impl Fn(&Tree, NodeId) -> bool,
) -> Result<Vec<NodeId>, ExtractError> {
    let mut ticker = Ticker { call, visited: 0 };
    let mut found = Vec::new();
    for id in tree.iterdescendants(ctx, &[]) {
        ticker.tick()?;
        if pred(tree, id) {
            found.push(id);
        }
    }
    Ok(found)
}

/// `(.//*[pred])[1]`: the first descendant of `ctx` that satisfies `pred`.
pub(crate) fn first_descendant(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
    pred: impl Fn(&Tree, NodeId) -> bool,
) -> Result<Option<NodeId>, ExtractError> {
    let mut ticker = Ticker { call, visited: 0 };
    for id in tree.iterdescendants(ctx, &[]) {
        ticker.tick()?;
        if pred(tree, id) {
            return Ok(Some(id));
        }
    }
    Ok(None)
}

/// `.//*[pred][1]`: the descendants of `ctx` that satisfy `pred` and have no preceding sibling
/// element that does, in document order.
pub(crate) fn first_among_siblings(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
    pred: impl Fn(&Tree, NodeId) -> bool,
) -> Result<Vec<NodeId>, ExtractError> {
    let mut ticker = Ticker { call, visited: 0 };
    let mut found = Vec::new();
    for id in tree.iterdescendants(ctx, &[]) {
        ticker.tick()?;
        if !pred(tree, id) {
            continue;
        }
        // The scan stops at the nearest earlier match, so the scans of one sibling list add up to
        // one pass over it.
        let mut earlier = tree.node(id).prev_sibling();
        let mut is_first = true;
        while let Some(sibling) = earlier {
            ticker.tick()?;
            if pred(tree, sibling) {
                is_first = false;
                break;
            }
            earlier = tree.node(sibling).prev_sibling();
        }
        if is_first {
            found.push(id);
        }
    }
    Ok(found)
}

/// The XPath union of node sets: the nodes of `parts` in the order of one walk of the subtree of
/// `ctx`, each once. A node outside that subtree is dropped.
pub(crate) fn union(
    tree: &Tree,
    ctx: NodeId,
    call: &Call<'_>,
    parts: &[NodeId],
) -> Result<Vec<NodeId>, ExtractError> {
    let wanted: HashSet<NodeId> = parts.iter().copied().collect();
    let mut ticker = Ticker { call, visited: 0 };
    let mut found = Vec::with_capacity(wanted.len());
    for id in tree.iter(ctx, &[]) {
        ticker.tick()?;
        if wanted.contains(&id) {
            found.push(id);
        }
    }
    Ok(found)
}
