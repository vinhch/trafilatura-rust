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

/// Table cell elements: html5lib's `InCellPhase` has no `processSpaceCharacters`, so whitespace-only
/// text in a cell skips "reconstruct the active formatting elements", which html5ever (and WHATWG)
/// does.
const CELL_TAGS: [&str; 2] = ["td", "th"];

/// The elements that step reconstructs.
const FORMATTING_TAGS: [&str; 14] = [
    "a", "b", "big", "code", "em", "font", "i", "nobr", "s", "small", "strike", "strong", "tt", "u",
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

fn element_name(line: &str) -> &str {
    let name = line.trim_start().split(' ').next().unwrap_or_default();
    name.split('@').next().unwrap_or(name)
}

/// True when some ancestor of `lines[index]` that lies before `start` (so unchanged) is one of `tags`.
/// Ancestors are found by walking back to each strictly smaller indent.
fn has_ancestor_in(lines: &[&str], index: usize, start: usize, tags: &[&str]) -> bool {
    let mut depth = lines.get(index).map_or(0, |line| indent_of(line));
    let before = lines.get(..index).unwrap_or_default();
    for (position, line) in before.iter().enumerate().rev() {
        if indent_of(line) >= depth {
            continue;
        }
        depth = indent_of(line);
        if position < start && tags.contains(&element_name(line)) {
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
/// unchanged `PYTHON_PARSER_TAGS` element, and the text of the two ranges is the same. html5lib only
/// drops tags, so the expected range holds no element line and the actual range holds no element line
/// of `PYTHON_PARSER_TAGS`: a dropped or added `option`, or a changed attribute, is not this class.
pub(crate) fn is_python_parser_hunk(expected: &[&str], actual: &[&str], hunk: &Hunk) -> bool {
    let inside = |lines: &[&str], range: &Range<usize>| {
        range
            .clone()
            .all(|index| has_ancestor_in(lines, index, range.start, &PYTHON_PARSER_TAGS))
    };
    let is_element = |line: &&str| !line.trim_start().starts_with('#');
    let no_element = lines_in(expected, &hunk.expected)
        .iter()
        .all(|line| !is_element(line));
    let no_parser_element = lines_in(actual, &hunk.actual)
        .iter()
        .filter(|line| is_element(line))
        .all(|line| !PYTHON_PARSER_TAGS.contains(&element_name(line)));
    no_element
        && no_parser_element
        && inside(expected, &hunk.expected)
        && inside(actual, &hunk.actual)
        && text_of(expected, &hunk.expected) == text_of(actual, &hunk.actual)
}

fn text_payload(line: &str) -> Option<&str> {
    line.trim_start()
        .strip_prefix("#text \"")?
        .strip_suffix('"')
}

/// Whitespace as the dump writes it: spaces and the backslash-n and backslash-t escapes.
fn is_dumped_whitespace(payload: &str) -> bool {
    let bare = payload.replace("\\n", "").replace("\\t", "");
    !payload.is_empty() && bare.trim_matches(' ').is_empty()
}

/// Rule for the html5lib whitespace bug in a table cell: one text line `ws+rest` on the expected side
/// against a formatting element holding the whitespace `ws`, then the text `rest`, on the actual side
/// (the element html5ever reconstructs), inside an unchanged `td` or `th`.
pub(crate) fn is_html5lib_cell_space_hunk(expected: &[&str], actual: &[&str], hunk: &Hunk) -> bool {
    let (left, right) = (
        lines_in(expected, &hunk.expected),
        lines_in(actual, &hunk.actual),
    );
    let (&[text], &[element, space, rest]) = (left.as_slice(), right.as_slice()) else {
        return false;
    };
    let depth = indent_of(text);
    let shape = indent_of(element) == depth
        && indent_of(rest) == depth
        && indent_of(space) > depth
        && FORMATTING_TAGS.contains(&element_name(element));
    let payloads = (text_payload(text), text_payload(space), text_payload(rest));
    let (Some(whole), Some(blank), Some(tail)) = payloads else {
        return false;
    };
    shape
        && is_dumped_whitespace(blank)
        && whole.strip_prefix(blank) == Some(tail)
        && has_ancestor_in(
            expected,
            hunk.expected.start,
            hunk.expected.start,
            &CELL_TAGS,
        )
        && has_ancestor_in(actual, hunk.actual.start, hunk.actual.start, &CELL_TAGS)
}

#[cfg(test)]
mod tests;
