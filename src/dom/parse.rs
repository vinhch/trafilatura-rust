//! The parse entry points: lxml's `fromstring` and `fragment_fromstring` over the libxml2 tree builder.
//!
//! The tokens come from html5gum (`emitter`), the tree from libxml2's rules (`builder`), and the root
//! from `fromstring`, which picks it as lxml does.

use std::sync::Arc;

use web_atoms::{LocalName, local_name};

use super::{Ns, Tree};
use crate::cancel::Call;
use crate::error::ExtractError;
use crate::settings::{DEFAULT_MAX_INPUT_BYTES, DEFAULT_MAX_NODES};

/// libxml2's nesting limit (`docs/architecture.md`, Runtime contract): parsing ends at the cut.
pub const MAX_DEPTH: usize = 256;

/// Attributes one tag may carry, duplicates included; above it parsing fails with
/// `Rejection::TreeTooLarge(TreeLimit::TagAttributes)`. It bounds the tokenizer's work on one tag,
/// whose duplicate check is linear in its attributes. Real pages stay far below 100.
// deviation: D-5
pub const MAX_TAG_ATTRIBUTES: usize = 1_000;

/// Attributes the whole parse may keep per allowed node (`max_nodes`). Real pages carry about 5 per
/// element; a page of tags of `MAX_TAG_ATTRIBUTES` attributes is what the budget stops, long before
/// the node limit.
// deviation: D-5
const ATTRIBUTES_PER_NODE: usize = 10;

/// U+FEFF: one leading one is stripped before tokenizing (`emitter::tokens`).
const BOM: char = '\u{feff}';

/// Limits applied while parsing.
#[derive(Clone, Copy, Debug)]
pub struct ParseLimits {
    /// Element nesting depth at which parsing stops and the partial tree is kept.
    pub max_depth: usize,
    /// Element count above which parsing fails with `Rejection::TreeTooLarge(TreeLimit::MaxNodes)`;
    /// so does a total of more than `ATTRIBUTES_PER_NODE` kept attributes per allowed element.
    /// `None` = no limit. Defaults to `DEFAULT_MAX_NODES`.
    pub max_nodes: Option<usize>,
    /// Raw input length above which loading fails with
    /// `Rejection::InputTooLarge(InputLimit::MaxInputBytes)`, checked before decompression;
    /// `None` = no limit. Defaults to `DEFAULT_MAX_INPUT_BYTES`.
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

/// Parses a document as `lxml.html.fromstring` does: the root is the `html` element, or the single
/// element or body the input is wrapped in.
///
/// Comments, processing instructions and the doctype are dropped. Nesting deeper than
/// `limits.max_depth` ends the parse and keeps the partial tree.
pub fn parse_document(
    html: &str,
    limits: &ParseLimits,
    call: &Call<'_>,
) -> Result<Tree, ExtractError> {
    fromstring::fromstring(html, limits, call)
}

/// Parses a fragment as `lxml.html.fragment_fromstring(create_parent="div")` does; the root is that
/// `div`, holding the result. `limits.drop_script_text` is ignored: a fragment keeps every text.
pub fn parse_fragment(
    html: &str,
    limits: &ParseLimits,
    call: &Call<'_>,
) -> Result<Tree, ExtractError> {
    let limits = ParseLimits {
        drop_script_text: false,
        ..*limits
    };
    fromstring::fragment_fromstring_div(html, &limits, call)
}

mod builder;
mod emitter;
mod fromstring;
pub(in crate::dom) mod tables;
#[cfg(test)]
mod tree_cases;

#[cfg(test)]
mod tests;
