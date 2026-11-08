//! htmldate's `extractors.py`: the string extractors that read a date out of a text segment, the URL and
//! author-written dates, and the JSON patterns. The dateparser fallback (`_external_date`, D-13) gives no date,
//! and `dateutil_parse` (D-13) gives no candidate.

use std::collections::HashMap;
use std::sync::LazyLock;

use regex::{Captures, Match, Regex};

use crate::cancel::Call;
use crate::dom::{NodeId, Tree};
use crate::error::ExtractError;
use crate::html_processing::tick;
use crate::pydatetime::DateTime;
use crate::pystr::{self, compile_py, ignore_case_i};
use crate::xpaths::select::is_html;

use super::settings::MAX_SEGMENT_LEN;
use super::utils::{Extractor, Pattern, remove_if_attached, trim_text};
use super::validators::{
    Format, convert_date, correct_year, int_text, is_valid_date_str, validate_and_convert,
};

/// The tags whose text the date search reads.
// upstream: extractors.py::FAST_TAGS
pub(crate) const FAST_TAGS: &[&str] = &["div", "h2", "h3", "h4", "li", "p", "span", "time", "ul"];

// upstream: extractors.py::DAY_RE
pub(crate) const DAY_RE: &str = "[0-3]?[0-9]";
// upstream: extractors.py::MONTH_RE
pub(crate) const MONTH_RE: &str = "[0-1]?[0-9]";
// upstream: extractors.py::YEAR_RE
// keep the (?:...): interpolated bare, the "|" would split the enclosing group
pub(crate) const YEAR_RE: &str = "(?:199[0-9]|20[0-3][0-9])";
// upstream: extractors.py::TIME_TZ_RE
// optional ISO-8601 time-of-day and time zone suffix (e.g. "T08:37:00+05:30")
pub(crate) const TIME_TZ_RE: &str =
    r"[T ][0-9]{2}:[0-9]{2}:[0-9]{2}(?:\.[0-9]+)?(?:Z|[+-][0-9]{2}:?[0-9]{2})?";

/// A month word, `[^\W\d_]{3,}`: a letter run of at least three code points.
const MONTH_WORD: &str = r"[@W@--[@D@_]]{3,}";

/// `re.compile(pattern, re.I)`: `i` and the dotless and dotted letters widened by `ignore_case_i`.
fn compile_ignore_case(pattern: &str) -> Option<Regex> {
    compile_py(&format!("(?i){}", ignore_case_i(pattern)))
}

// upstream: extractors.py::YMD_NO_SEP_PATTERN
// `\b(\d{8})\b`: the boundaries are written out, as `regex` has no lookaround
pub(crate) static YMD_NO_SEP_PATTERN: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile_py(r"(?:[^@W@]|\A)([@D@]{8})(?:[^@W@]|\z)"));

// upstream: extractors.py::YMD_PATTERN
pub(crate) static YMD_PATTERN: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile_py(&format!(
        r"(?:[^@D@]|\A)(?:(?P<year>{YEAR_RE})[\-/.](?P<month>{MONTH_RE})[\-/.](?P<day>{DAY_RE})|(?P<day2>{DAY_RE})[\-/.](?P<month2>{MONTH_RE})[\-/.](?P<year2>[@D@]{{2,4}}))(?:[^@D@]|\n?\z)"
    ))
});

// upstream: extractors.py::YM_PATTERN
pub(crate) static YM_PATTERN: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile_py(&format!(
        r"(?:[^@D@]|\A)(?:(?P<year>{YEAR_RE})[\-/.](?P<month>{MONTH_RE})|(?P<month2>{MONTH_RE})[\-/.](?P<year2>{YEAR_RE}))(?:[^@D@]|\n?\z)"
    ))
});

// upstream: extractors.py::LONG_TEXT_PATTERN
// The MONTH_WORD lookbehind `(?<![^\W\d_])` of the first alternative is not in the regex: `regex_parse` checks it.
// The literals fold case through `(?i:...)`. MONTH_WORD stays case-sensitive, as `\w` is under Python `re.I`: it
// matches the letters only, where Rust's `(?i)` would also take U+0345, U+A7CB and U+A7DC.
pub(crate) static LONG_TEXT_PATTERN: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile_py(&format!(
        r"(?P<month>{MONTH_WORD})[@S@](?P<day>{DAY_RE})(?i:st|nd|rd|th)?,? (?P<year>{YEAR_RE})|(?P<day2>{DAY_RE})(?i:st|nd|rd|th|\.)? (?i:of )?(?P<month2>{MONTH_WORD})[,.]? (?P<year2>{YEAR_RE})"
    ))
});

// upstream: extractors.py::COMPLETE_URL
pub(crate) static COMPLETE_URL: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile_py(&format!(
        r"[^@D@]({YEAR_RE})[/_-]({MONTH_RE})[/_-]({DAY_RE})(?:[^@D@]|\n?\z)"
    ))
});

// upstream: extractors.py::JSON_MODIFIED
pub(crate) static JSON_MODIFIED: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile_ignore_case(&format!(
        r#""dateModified": ?"({YEAR_RE}-{MONTH_RE}-{DAY_RE})({TIME_TZ_RE})?"#
    ))
});

// upstream: extractors.py::JSON_PUBLISHED
pub(crate) static JSON_PUBLISHED: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile_ignore_case(&format!(
        r#""datePublished": ?"({YEAR_RE}-{MONTH_RE}-{DAY_RE})({TIME_TZ_RE})?"#
    ))
});

/// English, French, German, Indonesian and Turkish month names, each row one month from January.
// upstream: extractors.py::MONTHS
pub(crate) const MONTHS: &[&[&str]] = &[
    &[
        "jan", "januar", "jänner", "january", "januari", "janvier", "ocak", "oca",
    ],
    &[
        "feb", "februar", "feber", "february", "februari", "février", "şubat", "şub",
    ],
    &["mar", "mär", "märz", "march", "maret", "mart", "mars"],
    &["apr", "april", "avril", "nisan", "nis"],
    &["may", "mai", "mei", "mayıs"],
    &["jun", "juni", "june", "juin", "haziran", "haz"],
    &["jul", "juli", "july", "juillet", "temmuz", "tem"],
    &["aug", "august", "agustus", "ağustos", "ağu", "aout"],
    &["sep", "september", "septembre", "eylül", "eyl"],
    &["oct", "oktober", "october", "octobre", "okt", "ekim", "eki"],
    &["nov", "november", "kasım", "kas", "novembre"],
    &[
        "dec",
        "dez",
        "dezember",
        "december",
        "desember",
        "décembre",
        "aralık",
        "ara",
    ],
];

// upstream: extractors.py::_fold_month_name
/// `str.lower()` of a month word; it alone mishandles the Turkish dotless i, so both are folded to `i` first.
pub(crate) fn fold_month_name(token: &str) -> String {
    pystr::lower(&token.replace(['ı', 'İ'], "i"))
}

// upstream: extractors.py::MONTH_NUMBERS
// Keys are folded. A later name overrides an earlier one, as in Python's dict display.
static MONTH_NUMBERS: LazyLock<HashMap<String, u32>> = LazyLock::new(|| {
    let mut numbers = HashMap::new();
    for (number, names) in (1_u32..).zip(MONTHS) {
        for name in names.iter() {
            numbers.insert(fold_month_name(name), number);
        }
    }
    numbers
});

/// The month number of a folded month word, `None` for a word that is no month.
pub(crate) fn month_number(word: &str) -> Option<u32> {
    MONTH_NUMBERS.get(word).copied()
}

/// `int(group)` of an optional group, `None` where the group did not take part or `int` refuses it.
fn int_of(group: Option<Match<'_>>) -> Option<i64> {
    int_text(group?.as_str())
}

/// `text[start:end]` in code points.
fn chars_between(text: &str, start: usize, end: usize) -> String {
    text.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

/// `str.isdigit()` of a segment: non-empty, and every code point is a digit.
fn is_all_digits(text: &str) -> bool {
    !text.is_empty() && text.chars().all(pystr::is_digit)
}

// upstream: extractors.py::extract_url_date
pub(crate) fn extract_url_date(testurl: Option<&str>, options: &Extractor) -> Option<String> {
    let captures = COMPLETE_URL.as_ref()?.captures(testurl?)?;
    let date = DateTime::new(
        int_of(captures.get(1))?,
        int_of(captures.get(2))?,
        int_of(captures.get(3))?,
    )?;
    validate_and_convert(Some(&date), Format::Ymd, &options.min, &options.max)
}

// upstream: extractors.py::try_swap_values
/// `(day, month)`, with the two swapped when the month cannot be one and the day can.
pub(crate) fn try_swap_values(day: i64, month: i64) -> (i64, i64) {
    if month > 12 && day <= 12 {
        (month, day)
    } else {
        (day, month)
    }
}

// upstream: extractors.py::_build_dmy
/// A date from day, month and year, with a two-digit year corrected and the day and month order fixed.
pub(crate) fn build_dmy(day: i64, month: i64, year: i64) -> Option<DateTime> {
    let year = correct_year(year);
    let (day, month) = try_swap_values(day, month);
    DateTime::new(year, month, day)
}

/// `[^\W\d_]`: a letter, a word code point that is no decimal digit and no underscore.
fn is_letter(c: char) -> bool {
    c != '_' && pystr::is_word(c) && pystr::decimal_value(c).is_none()
}

/// The lookbehind `(?<![^\W\d_])` at byte `index`: the code point before it is a letter.
fn preceded_by_letter(text: &str, index: usize) -> bool {
    text.get(..index)
        .and_then(|before| before.chars().next_back())
        .is_some_and(is_letter)
}

/// The byte index after the letter run that starts at byte `start`, `None` past the end of `text`.
fn letter_run_end(text: &str, start: usize) -> Option<usize> {
    let mut end = start;
    for c in text.get(start..)?.chars().take_while(|&c| is_letter(c)) {
        end = end.checked_add(c.len_utf8())?;
    }
    Some(end)
}

/// The byte index of the code point after the one at byte `index`, `None` at the end of `text`.
fn next_code_point(text: &str, index: usize) -> Option<usize> {
    let c = text.get(index..)?.chars().next()?;
    index.checked_add(c.len_utf8())
}

/// A month-first word, glued to what precedes it (`SmithMarch`): the last 12 code points, or the longest
/// suffix of them that is a month name.
// upstream: extractors.py::regex_parse (the `word[-12:]` and suffix search)
fn month_first_suffix(word: &str) -> String {
    let chars: Vec<char> = word.chars().collect();
    let tail = chars
        .get(chars.len().saturating_sub(12)..)
        .unwrap_or_default();
    let mut suffixes = (0..tail.len().saturating_sub(2))
        .filter_map(|start| tail.get(start..))
        .map(|suffix| suffix.iter().collect::<String>());
    match suffixes.find(|suffix| month_number(suffix).is_some()) {
        Some(suffix) => suffix,
        None => tail.iter().collect(),
    }
}

/// The date of a `LONG_TEXT_PATTERN` match whose month number is known: day, month and year as written.
fn named_month_date(captures: &Captures<'_>, month: u32) -> Option<DateTime> {
    let day = int_of(captures.name("day").or_else(|| captures.name("day2")))?;
    let year = int_of(captures.name("year").or_else(|| captures.name("year2")))?;
    build_dmy(day, i64::from(month), year)
}

// upstream: extractors.py::regex_parse
pub(crate) fn regex_parse(string: &str, call: &Call<'_>) -> Result<Option<DateTime>, ExtractError> {
    let Some(regex) = LONG_TEXT_PATTERN.as_ref() else {
        return Ok(None);
    };
    let mut pos = 0;
    let mut visited = 0;
    loop {
        tick(&mut visited, call)?;
        let Some(captures) = regex.captures_at(string, pos) else {
            return Ok(None);
        };
        let Some(whole) = captures.get(0) else {
            return Ok(None);
        };
        // The first alternative's month word takes the lookbehind `(?<![^\W\d_])`, not in this regex. A
        // start inside a letter run fails it, and so does every start up to the run's end (the second
        // alternative starts with a digit), so the search resumes at the first code point after the run.
        // Retrying one code point later would rescan the run each time: quadratic on a long word.
        let month_first = captures.name("month");
        if let Some(month) = month_first
            && preceded_by_letter(string, month.start())
        {
            let Some(run_end) = letter_run_end(string, month.start()) else {
                return Ok(None);
            };
            pos = run_end;
            continue;
        }
        let month_text = month_first
            .or_else(|| captures.name("month2"))
            .map_or("", |group| group.as_str());
        let mut word = fold_month_name(month_text);
        if month_first.is_some() {
            word = month_first_suffix(&word);
        }
        let Some(month) = month_number(&word) else {
            // matches can overlap
            let Some(next) = next_code_point(string, whole.start()) else {
                return Ok(None);
            };
            pos = next;
            continue;
        };
        if let Some(date) = named_month_date(&captures, month) {
            return Ok(Some(date));
        }
        pos = whole.end();
    }
}

// upstream: extractors.py::_parse_yyyymmdd
fn parse_yyyymmdd(digits: &str) -> Option<DateTime> {
    DateTime::new(
        int_text(&chars_between(digits, 0, 4))?,
        int_text(&chars_between(digits, 4, 6))?,
        int_text(&chars_between(digits, 6, 8))?,
    )
}

// upstream: extractors.py::_date_candidates, step 1
fn shortcut_candidate(string: &str) -> Option<DateTime> {
    if !is_all_digits(&chars_between(string, 0, 4)) {
        return None;
    }
    if is_all_digits(&chars_between(string, 4, 8)) {
        // a. '201709011234' is not covered by dateparser, and the regex is too slow
        return parse_yyyymmdd(string);
    }
    // b. much faster than extensive parsing. The `dateutil_parse` fallback gives no candidate (D-13).
    // deviation: D-13
    DateTime::from_isoformat(string)
}

// upstream: extractors.py::_date_candidates, step 2
fn yyyymmdd_candidate(string: &str) -> Option<DateTime> {
    let captures = YMD_NO_SEP_PATTERN.as_ref()?.captures(string)?;
    parse_yyyymmdd(captures.get(1)?.as_str())
}

// upstream: extractors.py::_date_candidates, step 3
fn ymd_candidate(string: &str) -> Option<DateTime> {
    let captures = YMD_PATTERN.as_ref()?.captures(string)?;
    if captures.name("year").is_some() {
        DateTime::new(
            int_of(captures.name("year"))?,
            int_of(captures.name("month"))?,
            int_of(captures.name("day"))?,
        )
    } else {
        build_dmy(
            int_of(captures.name("day2"))?,
            int_of(captures.name("month2"))?,
            int_of(captures.name("year2"))?,
        )
    }
}

// upstream: extractors.py::_date_candidates, step 4
fn ym_candidate(string: &str) -> Option<DateTime> {
    let captures = YM_PATTERN.as_ref()?.captures(string)?;
    let (year, month) = if captures.name("year").is_some() {
        (captures.name("year"), captures.name("month"))
    } else {
        (captures.name("year2"), captures.name("month2"))
    };
    DateTime::new(int_of(year)?, int_of(month)?, 1)
}

// upstream: extractors.py::custom_parse
// The five steps of `_date_candidates` run in order, and a step runs only when the steps before it did not
// validate, as upstream's generator is lazy.
pub(crate) fn custom_parse(
    string: &str,
    format: Format,
    min_date: &DateTime,
    max_date: &DateTime,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let accept = |candidate: Option<DateTime>| {
        validate_and_convert(candidate.as_ref(), format, min_date, max_date)
    };
    if let Some(found) = accept(shortcut_candidate(string)) {
        return Ok(Some(found));
    }
    if let Some(found) = accept(yyyymmdd_candidate(string)) {
        return Ok(Some(found));
    }
    if let Some(found) = accept(ymd_candidate(string)) {
        return Ok(Some(found));
    }
    if let Some(found) = accept(ym_candidate(string)) {
        return Ok(Some(found));
    }
    Ok(accept(regex_parse(string, call)?))
}

// upstream: extractors.py::_external_date
// deviation: D-13 (dateparser is not ported, so the string gives no date)
pub(crate) fn external_date(_string: &str) -> Option<DateTime> {
    None
}

// upstream: extractors.py::DISCARD_PATTERNS
// The patterns of the discard test. Each alternative keeps the anchor it had in upstream.
pub(crate) static DISCARD_PATTERNS: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile_py(
        r"\A[@D@]{2}:[@D@]{2}(?: |:|\n?\z)|\A[^@D@]*[@D@]{4}[^@D@]*\n?\z|[$€¥Ұ£¢₽₱฿#₹]|[A-Z]{3}[^A-Z]|(?:\A|[^@D@])(?:\+[@D@]{2}|[@D@]{3}|[@D@]{5})[^@D@]|ftps?|https?|sftp|\.(?:com|net|org|info|gov|edu|de|fr|io)(?:[^@W@]|\z)|IBAN|[A-Z]{2}[0-9]{2}|®",
    )
});

/// `DISCARD_PATTERNS.search(string)`: the segment only holds a time, a year, a currency, a code or a URL.
fn is_discarded(string: &str) -> bool {
    DISCARD_PATTERNS
        .as_ref()
        .is_some_and(|regex| regex.is_match(string))
}

// upstream: extractors.py::try_date_expr
pub(crate) fn try_date_expr(
    string: Option<&str>,
    format: Format,
    extensive_search: bool,
    min_date: &DateTime,
    max_date: &DateTime,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let Some(raw) = string.filter(|text| !text.is_empty()) else {
        return Ok(None);
    };
    let string: String = trim_text(raw).chars().take(MAX_SEGMENT_LEN).collect();
    // formal constraint: 4 to 18 digits
    let digits = string.chars().filter(|&c| pystr::is_digit(c)).count();
    if string.is_empty() || !(4..=18).contains(&digits) {
        return Ok(None);
    }
    if is_discarded(&string) {
        return Ok(None);
    }
    if let Some(found) = custom_parse(&string, format, min_date, max_date, call)? {
        return Ok(Some(found));
    }
    // deviation: D-13 (the gate before `_external_date` is not ported: it only decides whether that call runs, and
    // the call gives no date, so the result is None either way)
    if extensive_search {
        return Ok(validate_and_convert(
            external_date(&string).as_ref(),
            format,
            min_date,
            max_date,
        ));
    }
    Ok(None)
}

// upstream: extractors.py::try_date_expr_opts
// The output format is always `%Y-%m-%d`, so the port passes `Format::Ymd`.
pub(crate) fn try_date_expr_opts(
    string: Option<&str>,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    try_date_expr(
        string,
        Format::Ymd,
        options.extensive,
        &options.min,
        &options.max,
        call,
    )
}

// upstream: extractors.py::pattern_search
// The TIME_TZ_DIRECTIVES branch, which keeps a time or zone of the match, is not ported: the output format is
// `%Y-%m-%d`, and it has no such directive.
pub(crate) fn pattern_search(
    text: &str,
    date_pattern: &Pattern,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let Some(captures) = date_pattern.search(text, call)? else {
        return Ok(None);
    };
    let Some(date) = captures.get(1).map(|group| group.as_str()) else {
        return Ok(None);
    };
    if !is_valid_date_str(date, &options.min, &options.max) {
        return Ok(None);
    }
    Ok(convert_date(date))
}

/// The code points of a window on each side of a date core.
const CORE_WINDOW: usize = 60;

/// The date cores the window probe reads before it gives up and searches the whole text.
const MAX_CORES: usize = 1000;

/// The byte index `CORE_WINDOW` code points before byte `core_start`, or 0 when there are fewer.
fn window_start(text: &str, core_start: usize) -> usize {
    text.get(..core_start)
        .and_then(|before| before.char_indices().rev().nth(CORE_WINDOW - 1))
        .map_or(0, |(index, _)| index)
}

/// The byte index `CORE_WINDOW` code points after byte `core_end`, or the end of the text.
fn window_end(text: &str, core_end: usize) -> usize {
    text.get(core_end..)
        .and_then(|after| after.char_indices().nth(CORE_WINDOW))
        .and_then(|(offset, _)| core_end.checked_add(offset))
        .unwrap_or(text.len())
}

/// `pattern.search(text, start, end)`: the first match from byte `start` that ends by byte `end`.
fn search_window<'t>(
    regex: &Regex,
    text: &'t str,
    start: usize,
    end: usize,
) -> Option<Captures<'t>> {
    regex.captures_at(text.get(..end)?, start)
}

/// The date of the groups of an `idiosyncrasies` match: the three groups that took part, in order.
fn cue_date(parts: &[&str]) -> Option<DateTime> {
    // unreachable: each of the four alternatives has three groups, and one of them takes part
    let [first, second, third] = parts else {
        return None;
    };
    let (a, b, c) = (int_text(first)?, int_text(second)?, int_text(third)?);
    if first.chars().count() == 4 {
        DateTime::new(a, b, c)
    } else {
        build_dmy(a, b, c)
    }
}

// upstream: extractors.py::idiosyncrasies_search
// The windows are code point counts, converted to byte indexes; the 1,000-core cap is upstream's.
pub(crate) fn idiosyncrasies_search(
    htmlstring: &str,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let (Some(cores), Some(texts)) = (DATE_CORE_PATTERN.as_ref(), TEXT_PATTERNS.as_ref()) else {
        return Ok(None);
    };
    let mut visited = 0;
    let mut found: Option<Captures<'_>> = None;
    let mut probed_hit = false;
    let mut core_matches = cores.find_iter(htmlstring);
    // probe ±60-code-point windows around the numeric cores, then search unbounded so the result equals a
    // full scan
    for core in core_matches.by_ref().take(MAX_CORES) {
        tick(&mut visited, call)?;
        let start = window_start(htmlstring, core.start());
        let end = window_end(htmlstring, core.end());
        if search_window(texts, htmlstring, start, end).is_some() {
            found = texts.captures_at(htmlstring, start);
            probed_hit = true;
            break;
        }
    }
    // the cap was hit on a date-dense document: search the whole text
    if !probed_hit && core_matches.next().is_some() {
        found = texts.captures(htmlstring);
    }
    let Some(captures) = found else {
        return Ok(None);
    };
    let parts: Vec<&str> = (1..captures.len())
        .filter_map(|index| captures.get(index))
        .map(|group| group.as_str())
        .filter(|part| !part.is_empty())
        .collect();
    Ok(cue_date(&parts).and_then(|date| {
        validate_and_convert(Some(&date), Format::Ymd, &options.min, &options.max)
    }))
}

/// `\d{1,4}[./]\d{1,2}[./]\d{2,4}`: the numeric core shared by all `TEXT_PATTERNS` alternatives.
// upstream: extractors.py::DATE_CORE_PATTERN
pub(crate) static DATE_CORE_PATTERN: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile_py(r"[0-9]{1,4}[./][0-9]{1,2}[./][0-9]{2,4}"));

// upstream: extractors.py::TEXT_PATTERNS
// gap-runs bounded to {0,9} (ReDoS, and the matches stay within the probe windows)
pub(crate) static TEXT_PATTERNS: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile_ignore_case(
        r#"(?:date[^0-9"]{0,20}|updated|last-modified|published|posted|on)[ :]{0,9}?([0-9]{1,4})[./]([0-9]{1,2})[./]([0-9]{2,4})|(?:Datum|Stand|Veröffentlicht am):? ?([0-9]{1,2})\.([0-9]{1,2})\.([0-9]{2,4})|(?:güncellen?me|yayı(?:m|n)lan?ma) {0,9}?(?:tarihi)? {0,9}?:? {0,9}?([0-9]{1,2})[./]([0-9]{1,2})[./]([0-9]{2,4})|([0-9]{1,2})[./]([0-9]{1,2})[./]([0-9]{2,4}) {0,9}?(?:'de|'da|'te|'ta|’de|’da|’te|’ta|tarihinde) {0,9}(?:güncellendi|yayı(?:m|n)landı)"#,
    )
});

/// Deletes the archive.org banner, the `div` with `id` `wm-ipp-base` or `wm-ipp`, from `root`'s subtree, with the
/// tail of each. The matches are collected first, then removed.
// upstream: extractors.py::discard_unwanted
pub(crate) fn discard_unwanted(
    tree: &mut Tree,
    root: NodeId,
    call: &Call<'_>,
) -> Result<(), ExtractError> {
    let mut doomed = Vec::new();
    let mut visited = 0;
    for id in tree.iterdescendants(root, &[]) {
        tick(&mut visited, call)?;
        if tree.tag(id) == Some("div")
            && matches!(tree.get(id, "id"), Some("wm-ipp-base" | "wm-ipp"))
        {
            doomed.push(id);
        }
    }
    for id in doomed {
        remove_if_attached(tree, id);
    }
    Ok(())
}

/// The date in the URL of the first `og:image` meta of `root`'s subtree that has a `content` attribute.
// upstream: extractors.py::img_search
pub(crate) fn img_search(
    tree: &Tree,
    root: NodeId,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let mut visited = 0;
    for elem in tree.iterdescendants(root, &[]) {
        tick(&mut visited, call)?;
        if tree.tag(elem) == Some("meta")
            && tree.get(elem, "property") == Some("og:image")
            && let Some(content) = tree.get(elem, "content")
        {
            return Ok(extract_url_date(Some(content), options));
        }
    }
    Ok(None)
}

/// The date of the first JSON-LD or settings script of `root`'s subtree whose text the JSON pattern reads:
/// `datePublished` when `original`, else `dateModified`.
// upstream: extractors.py::json_search
pub(crate) fn json_search(
    tree: &Tree,
    root: NodeId,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let pattern = Pattern {
        regex: if options.original {
            &JSON_PUBLISHED
        } else {
            &JSON_MODIFIED
        },
        not_after: None,
    };
    let mut visited = 0;
    for elem in tree.iterdescendants(root, &[]) {
        tick(&mut visited, call)?;
        let is_json = matches!(
            tree.get(elem, "type"),
            Some("application/ld+json" | "application/settings+json")
        );
        if tree.tag(elem) != Some("script") || !is_json {
            continue;
        }
        // a script without text, or without a `"date` key, is skipped
        let Some(text) = tree
            .node(elem)
            .text
            .as_deref()
            .filter(|text| text.contains("\"date"))
        else {
            continue;
        };
        if let Some(found) = pattern_search(text, &pattern, options, call)? {
            return Ok(Some(found));
        }
    }
    Ok(None)
}

/// One step of the walk of `free_text_expressions`: an element is entered before its children and left after them,
/// where its tail comes.
enum FreeTextStep {
    Enter(NodeId),
    Leave(NodeId),
}

/// Pushes an `Enter` step for each child of `id`, the last first, so the first child is popped first.
fn push_enter_children(tree: &Tree, id: NodeId, stack: &mut Vec<FreeTextStep>) {
    let children: Vec<NodeId> = tree.children(id).collect();
    stack.extend(children.into_iter().rev().map(FreeTextStep::Enter));
}

/// The text and tail pieces of `root`'s subtree, in document order: the text of each fast-tag element, and the
/// tail of each element whose parent is a fast-tag element below `root`.
// upstream: extractors.py::FREE_TEXT_EXPRESSIONS (`.//*[self::div or ...]/text()`)
pub(crate) fn free_text_expressions<'t>(
    tree: &'t Tree,
    root: NodeId,
    call: &Call<'_>,
) -> Result<Vec<&'t str>, ExtractError> {
    let is_fast = |id: NodeId| is_html(tree, id, FAST_TAGS);
    let mut stack = Vec::new();
    let mut found = Vec::new();
    let mut visited = 0;
    push_enter_children(tree, root, &mut stack);
    while let Some(step) = stack.pop() {
        tick(&mut visited, call)?;
        match step {
            FreeTextStep::Enter(id) => {
                if is_fast(id) {
                    found.extend(tree.node(id).text.as_deref());
                }
                stack.push(FreeTextStep::Leave(id));
                push_enter_children(tree, id, &mut stack);
            }
            FreeTextStep::Leave(id) => {
                if tree
                    .node(id)
                    .parent()
                    .is_some_and(|parent| parent != root && is_fast(parent))
                {
                    found.extend(tree.node(id).tail.as_deref());
                }
            }
        }
    }
    Ok(found)
}
