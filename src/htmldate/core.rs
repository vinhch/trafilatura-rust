//! htmldate's `core.py`: `find_date`, which runs the stages in upstream's order, and the candidate selection and
//! page-text search behind them.

use crate::cancel::Call;
use crate::dom::{LocalName, NodeId, Ns, Tree};
use crate::error::ExtractError;
use crate::html_processing::tick;
use crate::pydatetime::DateTime;
use crate::pystr;
use crate::settings::DateParams;

use super::extractors::{
    discard_unwanted, extract_url_date, free_text_expressions, idiosyncrasies_search, img_search,
    json_search, pattern_search,
};
use super::settings::{CLEANING_LIST, MAX_SEGMENT_LEN, MIN_DATE, MIN_SEGMENT_LEN};
use super::utils::{Extractor, clean_html};
use super::validators::{check_extracted_reference, get_max_date};

mod examiners;
mod patterns;
mod search;

use patterns::{
    COPYRIGHT_PATTERN, DATESTRINGS_CATCH, DATESTRINGS_PATTERN, MMYYYY_PATTERN, SELECT_YMD_PATTERN,
    SELECT_YMD_YEAR, SIMPLE_PATTERN, SLASHES_PATTERN, SLASHES_YEAR, THREE_COMP_REGEX_A,
    THREE_COMP_REGEX_B, THREE_LOOSE_PATTERN, THREE_PATTERN, TIMESTAMP_LOOSE_PATTERN,
    TIMESTAMP_PATTERN, TWO_COMP_REGEX, YYYYMM_CATCH, YYYYMM_PATTERN,
};
use search::{compare_reference, has_plausible_candidates};

pub(super) use examiners::{
    date_candidates, examine_abbr_elements, examine_elements, examine_header,
    examine_time_elements, title_h1_elements,
};
pub(super) use patterns::{THREE_LOOSE_CATCH, YEAR_PATTERN};
#[cfg(test)]
pub(super) use search::select_candidate;
pub(super) use search::{examine_text, search_page};

/// The publication date of the page at `root`, `YYYY-MM-DD`; `None` when it has none. `tree` is not changed: the
/// pruning works on a copy of `root`.
// upstream: htmldate/core.py::find_date
pub fn find_date(
    tree: &Tree,
    root: NodeId,
    params: &DateParams,
    url: Option<&str>,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let options = Extractor {
        extensive: params.extensive_search,
        max: get_max_date(params.max_date.as_deref()),
        min: MIN_DATE,
        original: params.original_date,
    };
    // the canonical link is probed only when the caller gives no URL
    let canonical = match url {
        Some(_) => None,
        None => canonical_href(tree, root, call)?,
    };
    let url = url.or(canonical.as_deref());
    // direct processing of URL info; upstream's deferred_url_extractor is not ported: trafilatura never passes it
    // (settings.py::set_date_params), so the URL date always wins
    if let Some(date) = extract_url_date(url, &options) {
        return Ok(Some(date));
    }
    // first the header, then the JSON data
    if let Some(date) = examine_header(tree, root, &options, call)? {
        return Ok(Some(date));
    }
    if let Some(date) = json_search(tree, root, &options, call)? {
        return Ok(Some(date));
    }
    if let Some(date) = examine_abbr_elements(tree, root, &options, call)? {
        return Ok(Some(date));
    }

    // prune a copy: the caller's tree is not changed
    let mut scratch = Tree::new(LocalName::from("html"), Ns::Html, Vec::new());
    let search_root = tree.copy_into(root, &mut scratch, call)?;
    clean_html(&mut scratch, search_root, CLEANING_LIST, call)?;
    discard_unwanted(&mut scratch, search_root, call)?;

    let candidates = date_candidates(&scratch, search_root, options.extensive, call)?;
    if let Some(date) = examine_elements(&scratch, &candidates, &options, call)? {
        return Ok(Some(date));
    }
    let headings = title_h1_elements(&scratch, search_root, call)?;
    if let Some(date) = examine_elements(&scratch, &headings, &options, call)? {
        return Ok(Some(date));
    }
    if let Some(date) = examine_time_elements(&scratch, search_root, &options, call)? {
        return Ok(Some(date));
    }

    let htmlstring = scratch.tostring_html(search_root, true);
    // date regex timestamp rescue, then image elements, then precise patterns and idiosyncrasies
    if let Some(date) = pattern_search(&htmlstring, &TIMESTAMP_PATTERN, &options, call)? {
        return Ok(Some(date));
    }
    if let Some(date) = img_search(&scratch, search_root, &options, call)? {
        return Ok(Some(date));
    }
    if let Some(date) = idiosyncrasies_search(&htmlstring, &options, call)? {
        return Ok(Some(date));
    }

    // fast mode has no search_page fallback: a bare date, from text only
    if !options.extensive {
        clean_html(&mut scratch, search_root, &["script", "style"], call)?;
        let text = scratch
            .itertext_lxml(search_root)
            .collect::<Vec<_>>()
            .join(" ");
        return pattern_search(&text, &TIMESTAMP_LOOSE_PATTERN, &options, call);
    }

    // upstream: core.py::find_date (the free-text pass of the extensive search)
    let mut reference: Option<DateTime> = None;
    for segment in free_text_expressions(&scratch, search_root, call)? {
        let segment = pystr::strip(segment);
        let length = segment.chars().count();
        if MIN_SEGMENT_LEN < length && length < MAX_SEGMENT_LEN {
            reference = compare_reference(reference, segment, &options, call)?;
        }
    }
    if let Some(date) = check_extracted_reference(reference.as_ref(), &options) {
        return Ok(Some(date));
    }
    search_page(&htmlstring, &options, call)
}

/// The `href` of the first `link` element with `rel` `canonical` in `root`'s subtree, when it has one.
// upstream: core.py::find_date (`tree.find('.//link[@rel="canonical"]')`, then `get("href")`)
fn canonical_href(
    tree: &Tree,
    root: NodeId,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let mut visited = 0;
    for elem in tree.iterdescendants(root, &[]) {
        tick(&mut visited, call)?;
        if tree.tag(elem) == Some("link") && tree.get(elem, "rel") == Some("canonical") {
            return Ok(tree.get(elem, "href").map(str::to_owned));
        }
    }
    Ok(None)
}
