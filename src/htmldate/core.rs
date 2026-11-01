//! htmldate's `core.py`: `find_date`, which runs the stages in upstream's order, and the candidate selection and
//! page-text search behind them.

use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::cancel::Call;
use crate::dom::{LocalName, NodeId, Ns, Tree};
use crate::error::{ExtractError, Rejection};
use crate::html_processing::tick;
use crate::pydatetime::DateTime;
use crate::pystr::{self, compile_py};
use crate::settings::DateParams;

use super::extractors::{
    DAY_RE, FAST_TAGS, MONTH_RE, TIME_TZ_RE, YEAR_RE, YMD_PATTERN, discard_unwanted,
    extract_url_date, free_text_expressions, idiosyncrasies_search, img_search, json_search,
    pattern_search, regex_parse, try_date_expr, try_date_expr_opts,
};
use super::settings::{
    CLEANING_LIST, MAX_POSSIBLE_CANDIDATES, MAX_SEGMENT_LEN, MIN_DATE, MIN_SEGMENT_LEN,
};
use super::utils::{Extractor, NotAfter, Pattern, clean_html, trim_text};
use super::validators::{
    Counter, Format, check_extracted_reference, compare_values, correct_year, filter_ymd_candidate,
    get_max_date, int_text, is_valid_date_str, is_valid_datetime, plausible_year_filter,
    update_reference, validate_and_convert,
};

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
    let search_root = tree.copy_into(root, &mut scratch);
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

/// A `Pattern` named `$name` over the lazily compiled regex `$regex` of `$source`, which is a `format!` or a
/// raw string.
macro_rules! pattern {
    ($name:ident, $regex:ident, $source:expr) => {
        static $regex: LazyLock<Option<Regex>> = LazyLock::new(|| compile_py(&$source));
        pub(crate) static $name: Pattern = Pattern {
            regex: &$regex,
            not_after: None,
        };
    };
}

// upstream: core.py::NON_DIGITS_REGEX
// `\D+$`: `$` is `\n?\z`; `sub` deletes the one trailing run
static NON_DIGITS_REGEX: LazyLock<Option<Regex>> = LazyLock::new(|| compile_py(r"[^@D@]+\n?\z"));

// upstream: core.py::TIMESTAMP_PATTERN
pattern!(
    TIMESTAMP_PATTERN,
    TIMESTAMP_REGEX,
    format!(r"({YEAR_RE}-{MONTH_RE}-{DAY_RE})({TIME_TZ_RE})")
);

// upstream: core.py::TIMESTAMP_LOOSE_PATTERN
pattern!(
    TIMESTAMP_LOOSE_PATTERN,
    TIMESTAMP_LOOSE_REGEX,
    format!(r"({YEAR_RE}-{MONTH_RE}-{DAY_RE})({TIME_TZ_RE})?")
);

// upstream: core.py::THREE_COMP_REGEX_A
pattern!(
    THREE_COMP_REGEX_A,
    THREE_COMP_REGEX_A_REGEX,
    format!(r"({DAY_RE})[/.-]({MONTH_RE})[/.-]({YEAR_RE})")
);

// upstream: core.py::THREE_COMP_REGEX_B
pattern!(
    THREE_COMP_REGEX_B,
    THREE_COMP_REGEX_B_REGEX,
    format!(r"({DAY_RE})/({MONTH_RE})/([0-9]{{2}})|({DAY_RE})[.-]({MONTH_RE})[.-]([0-9]{{2}})")
);

// upstream: core.py::TWO_COMP_REGEX
pattern!(
    TWO_COMP_REGEX,
    TWO_COMP_REGEX_REGEX,
    format!(r"({MONTH_RE})[/.-]({YEAR_RE})")
);

// upstream: core.py::YEAR_PATTERN
pattern!(YEAR_PATTERN, YEAR_REGEX, format!(r"^[^@D@]?({YEAR_RE})"));

// upstream: core.py::COPYRIGHT_PATTERN
// bounded gap \D{0,99} (not \D*) avoids ReDoS
pattern!(
    COPYRIGHT_PATTERN,
    COPYRIGHT_REGEX,
    format!(r"(?:©|&copy;|Copyright|\(c\))[^@D@]{{0,99}}(?:{YEAR_RE})?-?({YEAR_RE})[^@D@]")
);

// upstream: core.py::THREE_PATTERN
pattern!(
    THREE_PATTERN,
    THREE_REGEX,
    r"/([0-9]{4}/[0-9]{2}/[0-9]{2})[01/]"
);

// upstream: core.py::THREE_LOOSE_PATTERN
pattern!(
    THREE_LOOSE_PATTERN,
    THREE_LOOSE_REGEX,
    r"[^@D@]([0-9]{4}[/.-][0-9]{2}[/.-][0-9]{2})[^@D@]"
);

// upstream: core.py::THREE_LOOSE_CATCH
pattern!(
    THREE_LOOSE_CATCH,
    THREE_LOOSE_CATCH_REGEX,
    r"([0-9]{4})[/.-]([0-9]{2})[/.-]([0-9]{2})"
);

// upstream: core.py::SELECT_YMD_PATTERN
pattern!(
    SELECT_YMD_PATTERN,
    SELECT_YMD_REGEX,
    format!(r"[^@D@]({DAY_RE}[/.-]{MONTH_RE}[/.-][0-9]{{4}})[^@D@]")
);

// upstream: core.py::SELECT_YMD_YEAR
pattern!(
    SELECT_YMD_YEAR,
    SELECT_YMD_YEAR_REGEX,
    format!(r"({YEAR_RE})[^@D@]?\n?\z")
);

// upstream: core.py::DATESTRINGS_PATTERN
pattern!(
    DATESTRINGS_PATTERN,
    DATESTRINGS_REGEX,
    r"([^@D@]19[0-9]{2}[01][0-9][0-3][0-9][^@D@]|[^@D@]20[0-9]{2}[01][0-9][0-3][0-9][^@D@])"
);

// upstream: core.py::DATESTRINGS_CATCH
pattern!(
    DATESTRINGS_CATCH,
    DATESTRINGS_CATCH_REGEX,
    format!(r"({YEAR_RE})([01][0-9])([0-3][0-9])")
);

// upstream: core.py::SLASHES_PATTERN
pattern!(
    SLASHES_PATTERN,
    SLASHES_REGEX,
    format!(r"[^@D@]({DAY_RE}/{MONTH_RE}/[0129][0-9]|[0-3][0-9]\.[01][0-9]\.[0129][0-9])[^@D@]")
);

// upstream: core.py::SLASHES_YEAR
pattern!(SLASHES_YEAR, SLASHES_YEAR_REGEX, r"([0-9]{2})\n?\z");

// upstream: core.py::YYYYMM_PATTERN
pattern!(
    YYYYMM_PATTERN,
    YYYYMM_REGEX,
    r"[^@D@]([12][0-9]{3}[/.-](?:1[0-2]|0[1-9]))[^@D@]"
);

// upstream: core.py::YYYYMM_CATCH
pattern!(
    YYYYMM_CATCH,
    YYYYMM_CATCH_REGEX,
    format!(r"({YEAR_RE})[/.-](1[0-2]|0[1-9])")
);

// upstream: core.py::MMYYYY_PATTERN
pattern!(
    MMYYYY_PATTERN,
    MMYYYY_REGEX,
    format!(r"[^@D@]({MONTH_RE}[/.-][12][0-9]{{3}})[^@D@]")
);

// `(?<!w3.org)` before the `\D`: a six code point lookbehind, `.` matching any code point but `\n`
static W3_ORG_WINDOW: LazyLock<Option<Regex>> = LazyLock::new(|| compile_py(r"\Aw3[^\n]org\z"));

// upstream: core.py::SIMPLE_PATTERN
pub(crate) static SIMPLE_PATTERN: Pattern = Pattern {
    regex: &SIMPLE_REGEX,
    not_after: Some(NotAfter {
        anchored: &W3_ORG_WINDOW,
        width: 6,
    }),
};
static SIMPLE_REGEX: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile_py(&format!(r"[^@D@]({YEAR_RE})[^@D@]")));

/// `YMD_PATTERN` as a `Pattern`, for the candidates of `search_normalized`.
// upstream: extractors.py::YMD_PATTERN
static YMD_CANDIDATE: Pattern = Pattern {
    regex: &YMD_PATTERN,
    not_after: None,
};

/// `int(text)` of a group the search patterns hold to ASCII digits.
// unreachable: the groups read here are digit runs of the patterns above, which `int` always reads
fn group_int(text: &str) -> Result<i64, ExtractError> {
    int_text(text).ok_or(ExtractError::Rejected(Rejection::InvalidValue))
}

/// `catch.search(text)`: the first three groups of the match, `None` where it does not match.
// upstream: re.Pattern.search (the groups the callers read)
fn catch_groups(
    catch: &Pattern,
    text: &str,
    call: &Call<'_>,
) -> Result<Option<[Option<String>; 3]>, ExtractError> {
    let Some(captures) = catch.search(text, call)? else {
        return Ok(None);
    };
    Ok(Some([1, 2, 3].map(|group| {
        captures.get(group).map(|found| found.as_str().to_owned())
    })))
}

/// `pattern.match(item)`: the match of `pattern` that starts at the first code point of `item`.
// upstream: re.Pattern.match
fn match_at_start<'t>(pattern: &Pattern, item: &'t str) -> Option<Captures<'t>> {
    let captures = pattern.regex.as_ref()?.captures(item)?;
    (captures.get(0)?.start() == 0).then_some(captures)
}

/// Prepares a text segment and tries to read a date from it.
// upstream: core.py::examine_text
pub(crate) fn examine_text(
    text: &str,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let text = trim_text(text);
    if text.chars().count() <= MIN_SEGMENT_LEN {
        return Ok(None);
    }
    let head: String = text.chars().take(MAX_SEGMENT_LEN).collect();
    let Some(non_digits) = NON_DIGITS_REGEX.as_ref() else {
        return Ok(None);
    };
    let segment = non_digits.replace(&head, "");
    try_date_expr_opts(Some(&segment), options, call)
}

/// Whether a candidate count is neither zero nor excessive.
// upstream: core.py::has_plausible_candidates
pub(crate) fn has_plausible_candidates(count: usize) -> bool {
    (1..=MAX_POSSIBLE_CANDIDATES).contains(&count)
}

/// Whether the year group of `year` lies between the bounds of `options`, both inclusive.
// upstream: core.py::select_candidate (the `min_year <= int(year) <= max_year` test)
fn year_in_range(year: &str, options: &Extractor) -> Result<bool, ExtractError> {
    let year = group_int(year)?;
    Ok(i64::from(options.min.year) <= year && year <= i64::from(options.max.year))
}

/// Selects one candidate among the most frequent matches, and returns the first three groups of its `catch`
/// match.
// upstream: core.py::select_candidate
// The `call` argument is not upstream: `Pattern::search` needs it.
pub(crate) fn select_candidate(
    occurrences: &Counter,
    catch: &Pattern,
    yearpat: &Pattern,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<Option<[Option<String>; 3]>, ExtractError> {
    if !has_plausible_candidates(occurrences.len()) {
        return Ok(None);
    }
    if occurrences.len() == 1 {
        let Some((only, _)) = occurrences.iter().next() else {
            return Ok(None);
        };
        return catch_groups(catch, only, call);
    }

    // most_common(10): the ten most frequent, ties in first-occurrence order
    let mut ranked: Vec<(&str, usize)> = occurrences.iter().collect();
    ranked.sort_by_key(|entry| std::cmp::Reverse(entry.1));
    ranked.truncate(10);
    // sorted(..., reverse=not original)[:2]: by text, the two largest, or the two smallest when original
    ranked.sort_by(|a, b| a.0.cmp(b.0));
    if !options.original {
        ranked.reverse();
    }
    ranked.truncate(2);
    let [best, runner] = ranked.as_slice() else {
        // unreachable: two or more distinct candidates are ranked
        return Err(ExtractError::Rejected(Rejection::InvalidValue));
    };
    let (best, best_count, runner, runner_count) = (best.0, best.1, runner.0, runner.1);

    // unreachable: plausible_year_filter keeps only keys whose year pattern matched
    let mut years = Vec::with_capacity(2);
    for pattern in [best, runner] {
        if let Some(year) = yearpat
            .search(pattern, call)?
            .and_then(|found| found.get(1))
        {
            years.push(year.as_str());
        }
    }
    let [first_year, second_year] = years.as_slice() else {
        return Err(ExtractError::Rejected(Rejection::InvalidValue));
    };

    // plausibility heuristics
    let [best_plausible, runner_plausible] = [
        year_in_range(first_year, options)?,
        year_in_range(second_year, options)?,
    ];
    let chosen = if best_plausible && runner_plausible {
        // the runner-up (older, or newer if original) wins when from another year and more than half as frequent,
        // except on ties
        let runner_up_wins = best_count != runner_count
            && first_year != second_year
            && runner_count.saturating_mul(2) > best_count;
        if runner_up_wins { runner } else { best }
    } else if best_plausible {
        best
    } else if runner_plausible {
        runner
    } else {
        return Ok(None);
    };
    catch_groups(catch, chosen, call)
}

/// Searches `htmlstring` for the candidates of `pattern`, then selects one of them.
// upstream: core.py::search_pattern
pub(crate) fn search_pattern(
    htmlstring: &str,
    pattern: &Pattern,
    catch: &Pattern,
    yearpat: &Pattern,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<Option<[Option<String>; 3]>, ExtractError> {
    let candidates = plausible_year_filter(
        htmlstring,
        pattern,
        yearpat,
        &options.min,
        &options.max,
        call,
    )?;
    select_candidate(&candidates, catch, yearpat, options, call)
}

/// Folds the date of `expression` into `reference`, when `expression` reads as a date.
// upstream: core.py::compare_reference
pub(crate) fn compare_reference(
    reference: Option<DateTime>,
    expression: &str,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<Option<DateTime>, ExtractError> {
    let attempt = try_date_expr(
        Some(expression),
        Format::Reference,
        options.extensive,
        &options.min,
        &options.max,
        call,
    )?;
    Ok(match attempt {
        Some(attempt) => compare_values(reference, &attempt, options),
        None => reference,
    })
}

/// A matched `D[/.-]M[/.-]Y` (or `M[/.-]Y`) run as `YYYY-MM-DD`: components zero-padded, a two-digit year
/// corrected.
// upstream: core.py::normalize_match
pub(crate) fn normalize_match(pattern: &Pattern, item: &str) -> Option<String> {
    let captures = match_at_start(pattern, item)?;
    let parts: Vec<&str> = captures
        .iter()
        .skip(1)
        .flatten()
        .map(|group| group.as_str())
        .filter(|group| !group.is_empty())
        .collect();
    // unreachable: each of the two patterns this takes has three non-empty groups for every match
    let [day, month, year] = parts.as_slice() else {
        return None;
    };
    let (day, month) = (format!("{day:0>2}"), format!("{month:0>2}"));
    let year = format!("{year:0>2}");
    let year = if year.chars().count() == 2 {
        correct_year(int_text(&year)?).to_string()
    } else {
        year
    };
    Some(format!("{year}-{month}-{day}"))
}

/// `MM-YYYY` as `YYYY-MM-01`.
// upstream: core.py::normalize_two_comp
pub(crate) fn normalize_two_comp(item: &str) -> Option<String> {
    let captures = match_at_start(&TWO_COMP_REGEX, item)?;
    let month = captures.get(1)?.as_str();
    let year = captures.get(2)?.as_str();
    Some(format!("{year}-{month:0>2}-01"))
}

/// The candidates of `pattern`, normalized and counted, then the best one, validated.
// upstream: core.py::search_normalized
pub(crate) fn search_normalized(
    htmlstring: &str,
    pattern: &Pattern,
    yearpat: &Pattern,
    normalizer: fn(&str) -> Option<String>,
    copyear: i64,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let candidates = plausible_year_filter(
        htmlstring,
        pattern,
        yearpat,
        &options.min,
        &options.max,
        call,
    )?;
    // revert DD-MM-YYYY patterns before sorting
    let mut normalized = Counter::default();
    let mut visited = 0;
    for (item, count) in candidates.iter() {
        tick(&mut visited, call)?;
        // unreachable: the normalizers read every candidate the year filter keeps
        let key = normalizer(item).ok_or(ExtractError::Rejected(Rejection::InvalidValue))?;
        normalized.add(&key, count);
    }
    let bestmatch = select_candidate(&normalized, &YMD_CANDIDATE, &YEAR_PATTERN, options, call)?;
    filter_ymd(bestmatch, copyear, options)
}

/// The YMD groups of the best match as `%Y-%m-%d`, when the date is in range and its year is at least `copyear`.
// upstream: core.py::_filter_ymd
fn filter_ymd(
    bestmatch: Option<[Option<String>; 3]>,
    copyear: i64,
    options: &Extractor,
) -> Result<Option<String>, ExtractError> {
    let groups = bestmatch
        .as_ref()
        .map(|groups| groups.each_ref().map(|group| group.as_deref()));
    filter_ymd_candidate(groups, copyear, &options.min, &options.max)
}

/// Searches `htmlstring` with `pattern`, then validates the best match as a YMD date.
// upstream: core.py::_search_and_filter
fn search_and_filter(
    htmlstring: &str,
    pattern: &Pattern,
    catch: &Pattern,
    copyear: i64,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let bestmatch = search_pattern(htmlstring, pattern, catch, &YEAR_PATTERN, options, call)?;
    filter_ymd(bestmatch, copyear, options)
}

/// The candidate date as `%Y-%m-%d`, when it is in range and its year is at least `copyear` (`0` for none).
// upstream: core.py::_finalize_candidate
pub(crate) fn finalize_candidate(
    dateobject: Option<DateTime>,
    copyear: i64,
    options: &Extractor,
) -> Option<String> {
    let date = dateobject?;
    if copyear == 0 || i64::from(date.year) >= copyear {
        validate_and_convert(Some(&date), Format::Ymd, &options.min, &options.max)
    } else {
        None
    }
}

/// Searches the page text for a date with the common text patterns, in upstream's order. Checks `call` before
/// each stage.
// upstream: core.py::search_page
pub(crate) fn search_page(
    htmlstring: &str,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    // copyright symbol
    call.check()?;
    let mut copyear = 0;
    if let Some([Some(year), _, _]) = search_pattern(
        htmlstring,
        &COPYRIGHT_PATTERN,
        &YEAR_PATTERN,
        &YEAR_PATTERN,
        options,
        call,
    )? {
        let year = group_int(&year)?;
        if DateTime::new(year, 1, 1)
            .is_some_and(|date| is_valid_datetime(&date, &options.min, &options.max))
        {
            copyear = year;
        }
    }

    // 3 components: target URL characteristics, then more loosely structured data
    for pattern in [&THREE_PATTERN, &THREE_LOOSE_PATTERN] {
        call.check()?;
        if let Some(result) = search_and_filter(
            htmlstring,
            pattern,
            &THREE_LOOSE_CATCH,
            copyear,
            options,
            call,
        )? {
            return Ok(Some(result));
        }
    }

    // YYYY-MM-DD/DD-MM-YYYY
    call.check()?;
    if let Some(result) = search_normalized(
        htmlstring,
        &SELECT_YMD_PATTERN,
        &SELECT_YMD_YEAR,
        |item| normalize_match(&THREE_COMP_REGEX_A, item),
        copyear,
        options,
        call,
    )? {
        return Ok(Some(result));
    }

    // valid dates strings
    call.check()?;
    if let Some(result) = search_and_filter(
        htmlstring,
        &DATESTRINGS_PATTERN,
        &DATESTRINGS_CATCH,
        copyear,
        options,
        call,
    )? {
        return Ok(Some(result));
    }

    // DD?/MM?/YY
    call.check()?;
    if let Some(result) = search_normalized(
        htmlstring,
        &SLASHES_PATTERN,
        &SLASHES_YEAR,
        |item| normalize_match(&THREE_COMP_REGEX_B, item),
        copyear,
        options,
        call,
    )? {
        return Ok(Some(result));
    }

    // 2 components, first option
    call.check()?;
    if let Some([Some(year), Some(month), _]) = search_pattern(
        htmlstring,
        &YYYYMM_PATTERN,
        &YYYYMM_CATCH,
        &YEAR_PATTERN,
        options,
        call,
    )? {
        let date = DateTime::new(group_int(&year)?, group_int(&month)?, 1);
        if let Some(result) = finalize_candidate(date, copyear, options) {
            return Ok(Some(result));
        }
    }

    // 2 components, second option
    call.check()?;
    if let Some(result) = search_normalized(
        htmlstring,
        &MMYYYY_PATTERN,
        &SELECT_YMD_YEAR,
        normalize_two_comp,
        copyear,
        options,
        call,
    )? {
        return Ok(Some(result));
    }

    // try full-blown text regex on all HTML
    call.check()?;
    if let Some(result) = finalize_candidate(regex_parse(htmlstring, call)?, copyear, options) {
        return Ok(Some(result));
    }

    // catchall: copyright mention
    if copyear != 0 {
        // unreachable: copyear is a year of YEAR_RE, and `datetime(copyear, 1, 1)` reads it
        let date =
            DateTime::new(copyear, 1, 1).ok_or(ExtractError::Rejected(Rejection::InvalidValue))?;
        return Ok(Some(Format::Ymd.render(&date)));
    }

    // last resort: 1 component
    call.check()?;
    if let Some([Some(year), _, _]) = search_pattern(
        htmlstring,
        &SIMPLE_PATTERN,
        &YEAR_PATTERN,
        &YEAR_PATTERN,
        options,
        call,
    )? {
        return Ok(finalize_candidate(
            DateTime::new(group_int(&year)?, 1, 1),
            copyear,
            options,
        ));
    }

    Ok(None)
}

/// The meta keys that hold a publication date, by `name`, `property` or `http-equiv`; lowercase.
// upstream: core.py::DATE_ATTRIBUTES
pub(crate) const DATE_ATTRIBUTES: &[&str] = &[
    "analyticsattributes.articledate",
    "article.created",
    "article_date_original",
    "article:post_date",
    "article.published",
    "article:published",
    "article:published_date",
    "article:published_time",
    "article:publicationdate",
    "bt:pubdate",
    "citation_date",
    "citation_publication_date",
    "content_create_date",
    "created",
    "cxenseparse:recs:publishtime",
    "date",
    "date_created",
    "date_published",
    "datecreated",
    "dateposted",
    "datepublished",
    // Dublin Core: https://wiki.whatwg.org/wiki/MetaExtensions
    "dc.date",
    "dc.created",
    "dc.date.created",
    "dc.date.issued",
    "dc.date.publication",
    "dcsext.articlefirstpublished",
    "dcterms.created",
    "dcterms.date",
    "dcterms.issued",
    "dc:created",
    "dc:date",
    "displaydate",
    "doc_date",
    "field-name-post-date",
    "gentime",
    "mediator_published_time",
    "meta", // too loose?
    // Open Graph: https://opengraphprotocol.org/
    "og:article:published",
    "og:article:published_time",
    "og:datepublished",
    "og:pubdate",
    "og:publish_date",
    "og:published_time",
    "og:question:published_time",
    "og:regdate",
    "originalpublicationdate",
    "parsely-pub-date",
    "pdate",
    "ptime",
    "pubdate",
    "publishdate",
    "publish_date",
    "publish_time",
    "publish-date",
    "published-date",
    "published_date",
    "published_time",
    "publisheddate",
    "publication_date",
    "rbpubdate",
    "release_date",
    "rnews:datepublished",
    "sailthru.date",
    "shareaholic:article_published_time",
    "timestamp",
    "twt-published-at",
    "video:release_date",
    "vr:published_time",
];

/// The `name` values of meta keys that hold a modified date.
// upstream: core.py::NAME_MODIFIED
pub(crate) const NAME_MODIFIED: &[&str] = &[
    "lastdate",
    "lastmod",
    "lastmodified",
    "last-modified",
    "modified",
    "utime",
];

/// The `property` values of meta keys that hold a modified date.
// upstream: core.py::PROPERTY_MODIFIED
pub(crate) const PROPERTY_MODIFIED: &[&str] = &[
    "article:modified",
    "article:modified_date",
    "article:modified_time",
    "article:post_modified",
    "bt:moddate",
    "datemodified",
    "dc.modified",
    "dcterms.modified",
    "lastmodified",
    "modified_time",
    "modificationdate",
    "og:article:modified_time",
    "og:modified_time",
    "og:updated_time",
    "release_date",
    "revision_date",
    "updated_time",
];

/// The `itemprop` values of original (publication) dates.
// upstream: core.py::ITEMPROP_ATTRS_ORIGINAL
pub(crate) const ITEMPROP_ATTRS_ORIGINAL: &[&str] = &["datecreated", "datepublished", "pubyear"];

/// The `itemprop` values of modified dates.
// upstream: core.py::ITEMPROP_ATTRS_MODIFIED
pub(crate) const ITEMPROP_ATTRS_MODIFIED: &[&str] = &["datemodified", "dateupdate"];

/// The `class` values of `abbr` and `time` elements that mark a publication date.
// upstream: core.py::CLASS_ATTRS
pub(crate) const CLASS_ATTRS: &[&str] = &["date-published", "published", "time published"];

/// Tags whose elements are date candidates whatever their attributes.
// upstream: core.py::ALWAYS_TAGS
pub(crate) const ALWAYS_TAGS: &[&str] = &["footer", "small"];

/// Substrings of an `id` that make an element a date candidate (`[Mm]eta|time|publish|footer`).
// upstream: core.py::ID_CLASS_CUES
pub(crate) const ID_CLASS_CUES: &[&str] = &["Meta", "meta", "time", "publish", "footer"];

/// Substrings of a `class` that make an element a date candidate.
// upstream: core.py::CLASS_CUES
pub(crate) const CLASS_CUES: &[&str] = &[
    "info",
    "post_detail",
    "block-content",
    "byline",
    "subline",
    "posted",
    "submitted",
    "created-post",
    "publication",
    "author",
    "autor",
    "field-content",
    "fa-clock-o",
    "fa-calendar",
    "fecha",
    "parution",
];
/// Whether `elem` is a date candidate by its attributes: the first of `id` and `class` in source order is a
/// date cue, or `itemprop` is, or the `id` or `class` names a cue.
// upstream: core.py::is_date_candidate
pub(crate) fn is_date_candidate(tree: &Tree, elem: NodeId) -> bool {
    let mut first: Option<&str> = None;
    let mut itemprop: Option<&str> = None;
    for (key, value) in &tree.node(elem).attrs {
        if &**key == "itemprop" {
            itemprop = Some(&**value);
        } else if first.is_none() && matches!(&**key, "id" | "class") {
            first = Some(&**value);
        }
    }
    for cue in [first, itemprop].into_iter().flatten() {
        // `replace("D", "d")` folds only the capital D
        let folded = cue.replace('D', "d");
        if folded.contains("date") || folded.contains("datum") {
            return true;
        }
    }
    let Some(first) = first else {
        return false;
    };
    if ID_CLASS_CUES.iter().any(|cue| first.contains(*cue)) {
        return true;
    }
    if tree
        .get(elem, "class")
        .is_some_and(|class| CLASS_CUES.iter().any(|cue| class.contains(*cue)))
    {
        return true;
    }
    tree.get(elem, "id")
        .is_some_and(|id| id.contains("footer-info-lastmod"))
}

/// The HTML elements of `root`'s subtree named in `tags`, in document order; checks `call` at each node.
fn tagged_descendants(
    tree: &Tree,
    root: NodeId,
    tags: &[&str],
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    let mut found = Vec::new();
    let mut visited = 0;
    for id in tree.iterdescendants(root, &[]) {
        tick(&mut visited, call)?;
        if tree.tag(id).is_some_and(|name| tags.contains(&name)) {
            found.push(id);
        }
    }
    Ok(found)
}

/// The elements of `root`'s subtree that may carry a date, in document order: every element when
/// `extensive_search`, else the fast tags and the always-tags, of which the first is kept by its attributes.
// upstream: core.py::date_candidates
pub(crate) fn date_candidates(
    tree: &Tree,
    root: NodeId,
    extensive_search: bool,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    let mut found = Vec::new();
    let mut visited = 0;
    for id in tree.iterdescendants(root, &[]) {
        tick(&mut visited, call)?;
        let tag = tree.tag(id);
        let always = tag.is_some_and(|name| ALWAYS_TAGS.contains(&name));
        let in_scope =
            extensive_search || always || tag.is_some_and(|name| FAST_TAGS.contains(&name));
        if in_scope && (always || is_date_candidate(tree, id)) {
            found.push(id);
        }
    }
    Ok(found)
}

/// The `title` and `h1` elements of `root`'s subtree, in document order: the union `.//title|.//h1`.
// upstream: core.py::find_date (`search_tree.xpath(".//title|.//h1")`)
pub(crate) fn title_h1_elements(
    tree: &Tree,
    root: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    tagged_descendants(tree, root, &["title", "h1"], call)
}

/// `examine_text` of the text and of the `title` attribute of each element, the first date found; nothing when
/// there are no candidates or more than `MAX_POSSIBLE_CANDIDATES`.
// upstream: core.py::examine_elements
pub(crate) fn examine_elements(
    tree: &Tree,
    elements: &[NodeId],
    options: &Extractor,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    if !has_plausible_candidates(elements.len()) {
        return Ok(None);
    }
    let mut visited = 0;
    for &elem in elements {
        tick(&mut visited, call)?;
        // the element text, then the link title (Blogspot)
        let content = tree.text_content(elem);
        let title = tree.get(elem, "title").unwrap_or("");
        for text in [content.as_str(), title] {
            if let Some(attempt) = examine_text(text, options, call)? {
                return Ok(Some(attempt));
            }
        }
    }
    Ok(None)
}

/// `YEAR-01-01` for a copyright year `content`, when that date is in the extractor's range.
// upstream: core.py::examine_header (the copyrightyear branch)
fn copyright_year_date(content: &str, options: &Extractor) -> Result<Option<String>, ExtractError> {
    let attempt = format!("{content}-01-01");
    if !is_valid_date_str(&attempt, &options.min, &options.max) {
        return Ok(None);
    }
    // unreachable: a string that validates reads its first four code points as `int` does
    let year = int_text(&attempt.chars().take(4).collect::<String>())
        .ok_or(ExtractError::Rejected(Rejection::InvalidValue))?;
    // unreachable: a validated year is in range, so the date exists
    let date = DateTime::new(year, 1, 1).ok_or(ExtractError::Rejected(Rejection::InvalidValue))?;
    Ok(Some(Format::Ymd.render(&date)))
}

/// The date of the header metas of `root`'s subtree: a date by name, property, itemprop, pubdate or http-equiv
/// wins; a URL date, a modified or copyright-year date is kept as a reserve.
// upstream: core.py::examine_header
pub(crate) fn examine_header(
    tree: &Tree,
    root: NodeId,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let mut headerdate: Option<String> = None;
    let mut reserve: Option<String> = None;
    let mut visited = 0;
    for elem in tree.iterdescendants(root, &[]) {
        tick(&mut visited, call)?;
        if tree.tag(elem) != Some("meta") {
            continue;
        }
        if tree.get(elem, "content").is_none() && tree.get(elem, "datetime").is_none() {
            continue;
        }
        let content = tree.get(elem, "content");
        if let Some(name) = tree.get(elem, "name") {
            let attribute = pystr::lower(name);
            if attribute == "og:url" {
                reserve = extract_url_date(content, options).or(reserve);
            } else if DATE_ATTRIBUTES.contains(&attribute.as_str()) {
                headerdate = try_date_expr_opts(content, options, call)?;
            } else if NAME_MODIFIED.contains(&attribute.as_str()) {
                if options.original {
                    reserve = try_date_expr_opts(content, options, call)?.or(reserve);
                } else {
                    headerdate = try_date_expr_opts(content, options, call)?;
                }
            }
        } else if let Some(property) = tree.get(elem, "property") {
            let attribute = pystr::lower(property);
            if (DATE_ATTRIBUTES.contains(&attribute.as_str())
                || PROPERTY_MODIFIED.contains(&attribute.as_str()))
                && let Some(attempt) = try_date_expr_opts(content, options, call)?
            {
                let stored = if options.original {
                    DATE_ATTRIBUTES
                } else {
                    PROPERTY_MODIFIED
                };
                // the other kind holds a date that hurts precision: a reserve
                if stored.contains(&attribute.as_str()) {
                    headerdate = Some(attempt);
                } else {
                    reserve = Some(attempt);
                }
            }
        } else if let Some(itemprop) = tree.get(elem, "itemprop") {
            let attribute = pystr::lower(itemprop);
            if ITEMPROP_ATTRS_ORIGINAL.contains(&attribute.as_str())
                || ITEMPROP_ATTRS_MODIFIED.contains(&attribute.as_str())
            {
                let value = tree
                    .get(elem, "datetime")
                    .filter(|value| !value.is_empty())
                    .or(content);
                if let Some(attempt) = try_date_expr_opts(value, options, call)? {
                    let stored = if options.original {
                        ITEMPROP_ATTRS_ORIGINAL
                    } else {
                        ITEMPROP_ATTRS_MODIFIED
                    };
                    if stored.contains(&attribute.as_str()) {
                        headerdate = Some(attempt);
                    }
                }
            } else if attribute == "copyrightyear"
                && let Some(year) = content
                && let Some(date) = copyright_year_date(year, options)?
            {
                reserve = Some(date);
            }
        } else if let Some(pubdate) = tree.get(elem, "pubdate") {
            if pystr::lower(pubdate) == "pubdate" {
                headerdate = try_date_expr_opts(content, options, call)?;
            }
        } else if let Some(http_equiv) = tree.get(elem, "http-equiv") {
            let attribute = pystr::lower(http_equiv);
            if attribute == "date" || attribute == "last-modified" {
                let attempt = try_date_expr_opts(content, options, call)?;
                // "date" is original, "last-modified" is updated
                if (attribute == "date") == options.original {
                    headerdate = attempt;
                } else {
                    reserve = attempt.or(reserve);
                }
            }
        }
        if headerdate.is_some() {
            break;
        }
    }
    Ok(headerdate.or(reserve))
}

/// The date of an `abbr` `data-utime` in UTC, `None` where Python's `fromtimestamp` refuses the value.
// upstream: core.py::examine_abbr_elements (the `data-utime` conversion)
fn utime_date(text: &str) -> Option<DateTime> {
    DateTime::from_timestamp_utc(int_text(text)?)
}

/// The date of the `abbr` elements of `root`'s subtree: a `data-utime`, a published `title`, or a long text; the
/// element text is the fallback.
// upstream: core.py::examine_abbr_elements
pub(crate) fn examine_abbr_elements(
    tree: &Tree,
    root: NodeId,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let elements = tagged_descendants(tree, root, &["abbr"], call)?;
    if !has_plausible_candidates(elements.len()) {
        return Ok(None);
    }
    let mut reference: Option<DateTime> = None;
    let mut visited = 0;
    for &elem in &elements {
        tick(&mut visited, call)?;
        if let Some(utime) = tree.get(elem, "data-utime") {
            // untrusted: a value outside the platform timestamp range is skipped
            let Some(candidate) = utime_date(utime) else {
                continue;
            };
            reference = Some(update_reference(reference, candidate, options.original));
        } else if tree
            .get(elem, "class")
            .is_some_and(|class| CLASS_ATTRS.contains(&class))
        {
            if let Some(trytext) = tree.get(elem, "title") {
                if options.original {
                    if let Some(attempt) = try_date_expr_opts(Some(trytext), options, call)? {
                        return Ok(Some(attempt));
                    }
                } else {
                    reference = compare_reference(reference, trytext, options, call)?;
                    // faster execution
                    if reference.is_some() {
                        break;
                    }
                }
            } else if let Some(text) = tree
                .node(elem)
                .text
                .as_deref()
                .filter(|text| text.chars().count() > 10)
            {
                reference = compare_reference(reference, text, options, call)?;
            }
        }
    }
    if let Some(found) = check_extracted_reference(reference.as_ref(), options) {
        return Ok(Some(found));
    }
    examine_elements(tree, &elements, options, call)
}

/// The newest (or, when `original`, the oldest) date of the `time` elements of `root`'s subtree. The shortcut
/// attributes return their date at once.
// upstream: core.py::examine_time_elements
pub(crate) fn examine_time_elements(
    tree: &Tree,
    root: NodeId,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let elements = tagged_descendants(tree, root, &["time"], call)?;
    if !has_plausible_candidates(elements.len()) {
        return Ok(None);
    }
    let mut reference: Option<DateTime> = None;
    let mut visited = 0;
    for &elem in &elements {
        tick(&mut visited, call)?;
        let datetime_attr = tree.get(elem, "datetime").unwrap_or("");
        if datetime_attr.chars().count() > 6 {
            let class_attr = tree.get(elem, "class").unwrap_or("");
            let shortcut_flag = if options.original {
                tree.get(elem, "pubdate") == Some("pubdate")
                    || class_attr.starts_with("entry-date")
                    || class_attr.starts_with("entry-time")
            } else {
                class_attr == "updated"
            };
            if shortcut_flag {
                if let Some(attempt) = try_date_expr_opts(Some(datetime_attr), options, call)? {
                    return Ok(Some(attempt));
                }
            } else {
                reference = compare_reference(reference, datetime_attr, options, call)?;
            }
        } else if let Some(text) = tree
            .node(elem)
            .text
            .as_deref()
            .filter(|text| text.chars().count() > 6)
        {
            reference = compare_reference(reference, text, options, call)?;
        }
    }
    Ok(check_extracted_reference(reference.as_ref(), options))
}
