//! Metadata extraction of upstream `metadata.py`. Only the entry point `core.py` calls exists so far.

use std::collections::HashSet;

use crate::dom::{NodeId, Tree};
use crate::settings::{DateParams, Document};

/// The metadata of the page at `root`, as a [`Document`] with empty body and comments. Every field is
/// unset until phase 4.
// upstream: metadata.py::extract_metadata
pub fn extract_metadata(
    _tree: &Tree,
    _root: NodeId,
    _default_url: Option<&str>,
    _date_config: &DateParams,
    _extensive: bool,
    _author_blacklist: &HashSet<String>,
) -> Document {
    // todo(phase 4): metadata
    Document::new()
}
