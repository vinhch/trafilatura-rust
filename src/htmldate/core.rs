//! htmldate's `core.py`: the page-text search (`search_page`) and the candidate selection behind it. The
//! entry point `find_date` stays a stub until its date stages are wired to this search.

use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::cancel::Call;
use crate::dom::{NodeId, Tree};
use crate::error::{ExtractError, Rejection};
use crate::html_processing::tick;
use crate::pydatetime::DateTime;
use crate::pystr::compile_py;
use crate::settings::DateParams;

use super::extractors::{
    DAY_RE, MONTH_RE, TIME_TZ_RE, YEAR_RE, YMD_PATTERN, regex_parse, try_date_expr,
    try_date_expr_opts,
};
use super::settings::{MAX_POSSIBLE_CANDIDATES, MAX_SEGMENT_LEN, MIN_SEGMENT_LEN};
use super::utils::{Extractor, NotAfter, Pattern, trim_text};
use super::validators::{
    Counter, Format, compare_values, correct_year, filter_ymd_candidate, int_text,
    is_valid_datetime, plausible_year_filter, validate_and_convert,
};

/// The publication date of the page at `root`, `YYYY-MM-DD`; `None` when it has none.
// upstream: htmldate/core.py::find_date
pub fn find_date(
    _tree: &mut Tree,
    _root: NodeId,
    _params: &DateParams,
    _url: Option<&str>,
    _call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    // todo(phase 4c): htmldate
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
