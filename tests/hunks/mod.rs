//! Line alignment of two tree dumps or outputs and the per-hunk rules of the parity check
//! (`docs/parity-harness.md`).

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

fn lines_in<'a>(lines: &[&'a str], range: &Range<usize>) -> Vec<&'a str> {
    lines.get(range.clone()).unwrap_or_default().to_vec()
}

/// Both ranges hold the same number of lines and each pair satisfies `same`.
fn pairs_match(
    expected: &[&str],
    actual: &[&str],
    hunk: &Hunk,
    same: impl Fn(&str, &str) -> bool,
) -> bool {
    let (left, right) = (
        lines_in(expected, &hunk.expected),
        lines_in(actual, &hunk.actual),
    );
    !left.is_empty()
        && left.len() == right.len()
        && left.iter().zip(&right).all(|(a, b)| same(a, b))
}

fn without_non_ascii(line: &str) -> String {
    line.chars().filter(char::is_ascii).collect()
}

/// Each run of whitespace and non-ASCII chars that holds a non-ASCII char becomes one space; a run of
/// ASCII whitespace alone stays, so whitespace not next to a non-ASCII char still has to match.
fn spaced_non_ascii(line: &str) -> String {
    fn flush(out: &mut String, run: &mut String, has_non_ascii: &mut bool) {
        if *has_non_ascii {
            out.push(' ');
        } else {
            out.push_str(run);
        }
        run.clear();
        *has_non_ascii = false;
    }
    let (mut out, mut run, mut has_non_ascii) = (String::new(), String::new(), false);
    for c in line.chars() {
        if c.is_whitespace() || !c.is_ascii() {
            run.push(c);
            has_non_ascii |= !c.is_ascii();
        } else {
            flush(&mut out, &mut run, &mut has_non_ascii);
            out.push(c);
        }
    }
    flush(&mut out, &mut run, &mut has_non_ascii);
    out
}

/// Tree dump rule for a page that is not valid UTF-8: equal line counts and each pair equal after
/// removing every non-ASCII char. The caller checks the page encoding.
pub(crate) fn is_encoding_hunk(expected: &[&str], actual: &[&str], hunk: &Hunk) -> bool {
    pairs_match(expected, actual, hunk, |a, b| {
        without_non_ascii(a) == without_non_ascii(b)
    })
}

/// Output rule: as `is_encoding_hunk`, or equal once a whitespace run next to a non-ASCII char counts
/// as one space. Upstream decodes some bytes to characters Python's `str.split()` counts as whitespace
/// (latin-1 0x85 is U+0085), which `trim` turns into a space, where the crate's cp1252 guess gives a
/// printable character (`…`); both are D-3.
pub(crate) fn is_encoding_text_hunk(expected: &[&str], actual: &[&str], hunk: &Hunk) -> bool {
    pairs_match(expected, actual, hunk, |a, b| {
        without_non_ascii(a) == without_non_ascii(b) || spaced_non_ascii(a) == spaced_non_ascii(b)
    })
}

#[cfg(test)]
mod tests;
