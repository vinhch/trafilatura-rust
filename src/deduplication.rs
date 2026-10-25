//! Duplicate detection of upstream `deduplication.py`. Only the entry points `htmlprocessing.py` and
//! `core.py` call exist so far.

use crate::context::Ctx;
use crate::dom::{NodeId, Tree};

/// Whether the text of `element` repeats too often to keep. Always `false` until phase 5 ports the
/// duplicate cache.
// upstream: deduplication.py::duplicate_test
pub fn duplicate_test(_tree: &Tree, _element: NodeId, _ctx: &Ctx<'_>) -> bool {
    // todo(phase 5): deduplication
    false
}

/// The fingerprint of `content` that metadata carries. Empty until phase 5 ports the hash.
// upstream: deduplication.py::content_fingerprint
pub fn content_fingerprint(_content: &str) -> String {
    // todo(phase 5): deduplication
    String::new()
}
