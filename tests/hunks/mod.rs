//! Line alignment of two tree dumps or outputs, for the parity check (`docs/parity-harness.md`).

use std::ops::Range;

use similar::{Algorithm, DiffTag, capture_diff_slices};

/// One run of removed and inserted lines, as line index ranges into the expected and actual lists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Hunk {
    pub(crate) expected: Range<usize>,
    pub(crate) actual: Range<usize>,
}

/// Every non-equal op of a Myers diff of the two line lists, in order.
pub(crate) fn hunks(expected: &[&str], actual: &[&str]) -> Vec<Hunk> {
    capture_diff_slices(Algorithm::Myers, expected, actual)
        .into_iter()
        .filter(|op| op.tag() != DiffTag::Equal)
        .map(|op| Hunk {
            expected: op.old_range(),
            actual: op.new_range(),
        })
        .collect()
}

#[cfg(test)]
mod tests;
