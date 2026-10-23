//! Duplicate detection of upstream `deduplication.py`. Only the entry point `htmlprocessing.py`
//! calls exists so far.

use crate::context::Ctx;
use crate::dom::{NodeId, Tree};

/// Whether the text of `element` repeats too often to keep. Always `false` until phase 5 ports the
/// duplicate cache.
// upstream: deduplication.py::duplicate_test
pub fn duplicate_test(_tree: &Tree, _element: NodeId, _ctx: &Ctx<'_>) -> bool {
    // todo(phase 5): deduplication
    false
}
