//! The XPath expressions of upstream `xpaths.py`, as Rust selector functions over the tree.
//!
//! Upstream compiles each expression once and evaluates it against a context element; a selector
//! here is the hand-written equivalent of one such expression.

pub(crate) mod select;

use crate::cancel::Call;
use crate::dom::{NodeId, Tree};
use crate::error::ExtractError;

/// Evaluates one XPath expression against a context node: the matches in document order, the
/// context node never among them.
pub type Selector = fn(&Tree, NodeId, &Call<'_>) -> Result<Vec<NodeId>, ExtractError>;

/// Joins XPath alternatives with `|`; `None` for an empty list, where upstream raises `ValueError`.
// upstream: xpaths.py::_alt
pub fn alt(tokens: &[&str]) -> Option<String> {
    (!tokens.is_empty()).then(|| tokens.join("|"))
}

#[cfg(test)]
mod tests;
