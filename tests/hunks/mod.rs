//! Line alignment of two tree dumps or outputs and the per-hunk rules of the parity check
//! (`docs/parity-harness.md`).

use std::ops::Range;

use similar::{Algorithm, DiffTag, capture_diff_slices};

/// Elements whose tree differs because html5lib (the fixture parser) predates a WHATWG change that
/// html5ever has: the 2025 "customizable select" parsing, where `select` and `option` keep phrasing
/// content such as `<span>` instead of dropping the tags, and the `search`, `hgroup` and
/// `selectedcontent` elements.
pub(crate) const PYTHON_PARSER_TAGS: [&str; 6] = [
    "search",
    "select",
    "option",
    "optgroup",
    "hgroup",
    "selectedcontent",
];

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

pub(crate) fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

fn lines_in<'a>(lines: &[&'a str], range: &Range<usize>) -> Vec<&'a str> {
    lines.get(range.clone()).unwrap_or_default().to_vec()
}

/// Both ranges hold the same number of lines and each pair is equal once `drop` chars are removed.
fn pairs_equal_without(
    expected: &[&str],
    actual: &[&str],
    hunk: &Hunk,
    drop: impl Fn(char) -> bool,
) -> bool {
    let (left, right) = (
        lines_in(expected, &hunk.expected),
        lines_in(actual, &hunk.actual),
    );
    let keep = |line: &str| line.chars().filter(|c| !drop(*c)).collect::<String>();
    !left.is_empty()
        && left.len() == right.len()
        && left.iter().zip(&right).all(|(a, b)| keep(a) == keep(b))
}

/// Tree dump rule for a page that is not valid UTF-8: equal line counts and each pair equal after
/// removing every non-ASCII char. The caller checks the page encoding.
pub(crate) fn is_encoding_hunk(expected: &[&str], actual: &[&str], hunk: &Hunk) -> bool {
    pairs_equal_without(expected, actual, hunk, |c| !c.is_ascii())
}

/// Output rule: as `is_encoding_hunk`, and whitespace is removed too. Upstream decodes some bytes to
/// characters Python's `str.split()` counts as whitespace (latin-1 0x85 is U+0085), which `trim` turns
/// into a space, where the crate's cp1252 guess gives a printable character (`…`); both are D-3.
pub(crate) fn is_encoding_text_hunk(expected: &[&str], actual: &[&str], hunk: &Hunk) -> bool {
    pairs_equal_without(expected, actual, hunk, |c| {
        !c.is_ascii() || c.is_whitespace()
    })
}

fn element_name(line: &str) -> &str {
    let name = line.trim_start().split(' ').next().unwrap_or_default();
    name.split('@').next().unwrap_or(name)
}

/// True when some ancestor of `lines[index]` that lies before `start` (so unchanged) is a
/// `PYTHON_PARSER_TAGS` element. Ancestors are found by walking back to each strictly smaller indent.
fn has_parser_ancestor(lines: &[&str], index: usize, start: usize) -> bool {
    let mut depth = lines.get(index).map_or(0, |line| indent_of(line));
    let before = lines.get(..index).unwrap_or_default();
    for (position, line) in before.iter().enumerate().rev() {
        if indent_of(line) >= depth {
            continue;
        }
        depth = indent_of(line);
        if position < start && PYTHON_PARSER_TAGS.contains(&element_name(line)) {
            return true;
        }
    }
    false
}

/// The `#text` payloads of a range joined; any other `#` line (comment, doctype) counts whole, so a
/// changed one is a difference. Element lines carry no text and are dropped.
fn text_of(lines: &[&str], range: &Range<usize>) -> String {
    lines_in(lines, range)
        .into_iter()
        .map(str::trim_start)
        .filter(|line| line.starts_with('#'))
        .map(|line| {
            line.strip_prefix("#text \"")
                .map_or(line, |text| text.strip_suffix('"').unwrap_or(text))
        })
        .collect()
}

/// Rule for the customizable-select parse difference: every line of both ranges sits inside an
/// unchanged `PYTHON_PARSER_TAGS` element, and the text of the two ranges is the same.
pub(crate) fn is_python_parser_hunk(expected: &[&str], actual: &[&str], hunk: &Hunk) -> bool {
    let inside = |lines: &[&str], range: &Range<usize>| {
        range
            .clone()
            .all(|index| has_parser_ancestor(lines, index, range.start))
    };
    inside(expected, &hunk.expected)
        && inside(actual, &hunk.actual)
        && text_of(expected, &hunk.expected) == text_of(actual, &hunk.actual)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hunks_align_an_inserted_line() {
        let found = hunks(&["a", "b", "c"], &["a", "x", "b", "c"]);
        assert_eq!(
            found,
            vec![Hunk {
                expected: 1..1,
                actual: 1..2
            }]
        );
    }

    #[test]
    fn hunks_of_equal_lists_is_empty() {
        assert!(hunks(&["a", "b"], &["a", "b"]).is_empty());
    }

    fn whole(expected: &[&str], actual: &[&str]) -> Hunk {
        Hunk {
            expected: 0..expected.len(),
            actual: 0..actual.len(),
        }
    }

    #[test]
    fn encoding_hunk_accepts_non_ascii_only_change() {
        let (e, a) = (["#text \"bliżej\""], ["#text \"bli¿ej\""]);
        assert!(is_encoding_hunk(&e, &a, &whole(&e, &a)));
    }

    #[test]
    fn encoding_hunk_rejects_ascii_change() {
        let e = ["#text \"bliżej\"", "#text \"abc\""];
        let a = ["#text \"bli¿ej\"", "#text \"abd\""];
        assert!(!is_encoding_hunk(&e, &a, &whole(&e, &a)));
        let insert = Hunk {
            expected: 0..0,
            actual: 0..1,
        };
        assert!(!is_encoding_hunk(&[], &["#text \"x\""], &insert));
    }

    #[test]
    fn encoding_text_hunk_accepts_whitespace_from_decoding() {
        let (e, a) = (["Idealen ( ), hinter"], ["Idealen (…), hinter"]);
        let hunk = whole(&e, &a);
        assert!(is_encoding_text_hunk(&e, &a, &hunk));
        assert!(!is_encoding_hunk(&e, &a, &hunk));
        let ascii = ["Idealen (x), hinter"];
        let hunk = whole(&e, &ascii);
        assert!(!is_encoding_text_hunk(&e, &ascii, &hunk));
        assert!(!is_encoding_hunk(&e, &ascii, &hunk));
    }

    const SELECT: [&str; 2] = ["    select", "      option value=\"1\""];

    fn tail(lines: &[&'static str]) -> Vec<&'static str> {
        SELECT.iter().chain(lines).copied().collect()
    }

    fn tail_hunk(expected: &[&str], actual: &[&str]) -> Hunk {
        Hunk {
            expected: 2..expected.len(),
            actual: 2..actual.len(),
        }
    }

    #[test]
    fn python_parser_hunk_accepts_kept_phrasing() {
        let e = tail(&["        #text \"1 Person\""]);
        let a = tail(&[
            "        #text \"1 \"",
            "        span",
            "          #text \"Person\"",
        ]);
        assert!(is_python_parser_hunk(&e, &a, &tail_hunk(&e, &a)));
    }

    #[test]
    fn python_parser_hunk_rejects_changed_text() {
        let e = tail(&["        #text \"1 Person\""]);
        let a = tail(&[
            "        #text \"1 \"",
            "        span",
            "          #text \"Persons\"",
        ]);
        assert!(!is_python_parser_hunk(&e, &a, &tail_hunk(&e, &a)));
    }

    #[test]
    fn python_parser_hunk_rejects_line_outside_subtree() {
        let e = tail(&["        #text \"1 Person\""]);
        let a = tail(&["        #text \"1 \"", "    span", "      #text \"Person\""]);
        assert!(!is_python_parser_hunk(&e, &a, &tail_hunk(&e, &a)));
    }

    #[test]
    fn python_parser_hunk_rejects_dropped_parser_element() {
        let e = ["  div", "    select name=\"q\""];
        let a = ["  div"];
        let hunk = Hunk {
            expected: 1..2,
            actual: 1..1,
        };
        assert!(!is_python_parser_hunk(&e, &a, &hunk));
    }
}
