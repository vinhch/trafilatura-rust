//! Helpers of htmldate's `utils.py`: the search options, `trim_text`, the element removal and the regex
//! `Pattern` with Python's `findall` and `search`.

use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::cancel::Call;
use crate::dom::{LiveIter, NodeId, Tree};
use crate::error::ExtractError;
use crate::html_processing::tick;
use crate::pydatetime::DateTime;
use crate::pystr;
use crate::xml::delete_element;

/// The options of one date search (`utils.py::Extractor`). The output format is always `%Y-%m-%d` in the
/// port, so it is not held.
// upstream: utils.py::Extractor
#[derive(Clone, Copy, Debug)]
pub(crate) struct Extractor {
    pub extensive: bool,
    pub max: DateTime,
    pub min: DateTime,
    pub original: bool,
}

/// `" ".join(string.split())`: the words of `string`, one space between them.
// upstream: utils.py::trim_text
pub(crate) fn trim_text(string: &str) -> String {
    pystr::split_whitespace(string)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Deletes each element of `root`'s subtree named in `elemlist`, keeping its tail. The walk is live: the
/// successor of an element is fixed before the element goes, as lxml's `iter` does.
// upstream: utils.py::clean_html
pub(crate) fn clean_html(
    tree: &mut Tree,
    root: NodeId,
    elemlist: &[&str],
    call: &Call<'_>,
) -> Result<(), ExtractError> {
    let mut walk = LiveIter::new(tree, root, elemlist);
    let mut visited = 0;
    while let Some(element) = walk.next(tree) {
        tick(&mut visited, call)?;
        delete_element(tree, element, true);
    }
    Ok(())
}

/// Detaches `element` from its parent together with its tail, as lxml's `parent.remove` does. Nothing
/// happens without a parent.
// upstream: utils.py::remove_if_attached
pub(crate) fn remove_if_attached(tree: &mut Tree, element: NodeId) {
    tree.remove(element);
}

/// A negative lookbehind `(?<!...)` that `regex` lacks. `anchored` is `\A(?:...)\z`, tested on the `width` code
/// points before a match start: a match it matches there is rejected, and the search retries one code point
/// later, as Python's engine does. Not upstream: it stands in for the lookbehind of the pattern it belongs to.
pub(crate) struct NotAfter {
    pub anchored: &'static LazyLock<Option<Regex>>,
    pub width: usize,
}

/// A regex of upstream, with Python's `findall` and `search`: group 1 is the value of a match.
// upstream: re.Pattern
pub(crate) struct Pattern {
    pub regex: &'static LazyLock<Option<Regex>>,
    pub not_after: Option<NotAfter>,
}

impl NotAfter {
    /// Whether the `width` code points before byte `start` of `text` match the lookbehind body. Fewer code
    /// points before `start` never match, as the lookbehind fails there.
    fn rejects(&self, text: &str, start: usize) -> bool {
        let Some(regex) = self.anchored.as_ref() else {
            return false;
        };
        let Some(before) = text.get(..start) else {
            return false;
        };
        let mut window: Vec<char> = before.chars().rev().take(self.width).collect();
        if window.len() < self.width {
            return false;
        }
        window.reverse();
        regex.is_match(&window.into_iter().collect::<String>())
    }
}

/// The byte index after the code point at byte `index` of `text`; `None` at the end.
fn next_code_point(text: &str, index: usize) -> Option<usize> {
    let c = text.get(index..)?.chars().next()?;
    index.checked_add(c.len_utf8())
}

impl Pattern {
    /// The first match at or after byte `start` that `not_after` does not reject, with its groups. Ticks
    /// `visited` per attempt, so a long run of rejected matches stays cancellable.
    fn next_match<'t>(
        &self,
        text: &'t str,
        start: usize,
        visited: &mut usize,
        call: &Call<'_>,
    ) -> Result<Option<Captures<'t>>, ExtractError> {
        let Some(regex) = self.regex.as_ref() else {
            return Ok(None);
        };
        let mut at = start;
        loop {
            tick(visited, call)?;
            let Some(captures) = regex.captures_at(text, at) else {
                return Ok(None);
            };
            let Some(whole) = captures.get(0) else {
                return Ok(None);
            };
            if !self
                .not_after
                .as_ref()
                .is_some_and(|rule| rule.rejects(text, whole.start()))
            {
                return Ok(Some(captures));
            }
            match next_code_point(text, whole.start()) {
                Some(next) => at = next,
                None => return Ok(None),
            }
        }
    }

    /// Python `findall` with one group: group 1 of each match, left to right, `""` where the group did not
    /// take part. After an empty match the search moves on one code point.
    // upstream: re.Pattern.findall
    pub(crate) fn findall<'t>(
        &self,
        text: &'t str,
        call: &Call<'_>,
    ) -> Result<Vec<&'t str>, ExtractError> {
        let mut found = Vec::new();
        let mut visited = 0;
        let mut start = 0;
        while let Some(captures) = self.next_match(text, start, &mut visited, call)? {
            let Some(whole) = captures.get(0) else {
                break;
            };
            found.push(captures.get(1).map_or("", |group| group.as_str()));
            start = if whole.is_empty() {
                match next_code_point(text, whole.end()) {
                    Some(next) => next,
                    None => break,
                }
            } else {
                whole.end()
            };
        }
        Ok(found)
    }

    /// Python `search`: the first match of `text`, with its groups.
    // upstream: re.Pattern.search
    pub(crate) fn search<'t>(
        &self,
        text: &'t str,
        call: &Call<'_>,
    ) -> Result<Option<Captures<'t>>, ExtractError> {
        self.next_match(text, 0, &mut 0, call)
    }
}
