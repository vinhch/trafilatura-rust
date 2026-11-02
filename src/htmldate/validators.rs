//! htmldate's `validators.py`: date validation and conversion, the reference comparison, the year and YMD
//! candidate filters, and the search bounds.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::cancel::Call;
use crate::error::{ExtractError, Rejection};
use crate::html_processing::tick;
use crate::pydatetime::DateTime;
use crate::pystr;

use super::utils::{Extractor, Pattern};

const SECONDS_PER_DAY: i64 = 86_400;

/// The last instant the clock can reach in the port's calendar, the fallback of [`get_max_date`].
const LAST_INSTANT: DateTime = DateTime {
    year: 9999,
    month: 12,
    day: 31,
    hour: 23,
    minute: 59,
    second: 59,
    microsecond: 999_999,
    offset: None,
};

/// The output formats the validators render.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Format {
    /// `%Y-%m-%d`, the output format of the port.
    Ymd,
    /// `REFERENCE_FORMAT`, `%Y-%m-%dT%H:%M:%S.%f%z`.
    // upstream: validators.py::REFERENCE_FORMAT
    Reference,
}

impl Format {
    /// `strftime` of `date` in this format.
    // upstream: datetime.strftime (CPython)
    pub(crate) fn render(self, date: &DateTime) -> String {
        match self {
            Self::Ymd => date.strftime_ymd(),
            Self::Reference => date.strftime_reference(),
        }
    }
}

/// Python's `collections.Counter` over strings: the keys in first-occurrence order. `index` maps a key to its
/// slot in `order`, so counting stays linear.
#[derive(Debug, Default)]
pub(crate) struct Counter {
    order: Vec<(String, usize)>,
    index: HashMap<String, usize>,
}

impl Counter {
    /// Adds `count` to the count of `key`; a new key goes last.
    pub(crate) fn add(&mut self, key: &str, count: usize) {
        match self.index.get(key) {
            Some(&slot) => {
                if let Some(entry) = self.order.get_mut(slot) {
                    entry.1 = entry.1.saturating_add(count);
                }
            }
            None => {
                self.index.insert(key.to_owned(), self.order.len());
                self.order.push((key.to_owned(), count));
            }
        }
    }

    /// The number of distinct keys.
    pub(crate) fn len(&self) -> usize {
        self.order.len()
    }

    /// The keys and their counts, in first-occurrence order.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, usize)> + '_ {
        self.order.iter().map(|(key, count)| (key.as_str(), *count))
    }
}

/// `_is_in_range`: `date` is between `earliest` and `latest` inclusive, by year first, then by instant.
// upstream: validators.py::_is_in_range
// deviation: D-21 (a naive value is read as UTC; the instants are exact, not floats)
pub(crate) fn is_valid_datetime(date: &DateTime, earliest: &DateTime, latest: &DateTime) -> bool {
    if !(earliest.year..=latest.year).contains(&date.year) {
        return false;
    }
    (earliest.timestamp_micros()..=latest.timestamp_micros()).contains(&date.timestamp_micros())
}

/// `int(text)` as an `i64`, `None` where `int` or the range refuses the text.
pub(super) fn int_text(text: &str) -> Option<i64> {
    pystr::int_str(text).ok()?.parse().ok()
}

/// `int(text)` of a year group. The groups hold at most four digits, so the value fits.
// upstream: int (CPython)
fn year_value(text: &str) -> Result<i64, ExtractError> {
    pystr::int_str(text)?
        .parse()
        .map_err(|_| ExtractError::Rejected(Rejection::InvalidValue))
}

/// The positional read of `YYYY-MM-DD`: `int(s[:4])`, `int(s[5:7])` and `int(s[8:10])` over code points.
// upstream: validators.py::_parse_and_validate
fn positional_ymd(date_input: &str) -> Option<DateTime> {
    let field = |start: usize, end: usize| {
        let text: String = date_input
            .chars()
            .skip(start)
            .take(end.saturating_sub(start))
            .collect();
        int_text(&text)
    };
    DateTime::new(field(0, 4)?, field(5, 7)?, field(8, 10)?)
}

/// `is_valid_date` with a `%Y-%m-%d` string. The positional read decides unless it fails; then `strptime`
/// reads unpadded parts such as `2020-1-15`.
// upstream: validators.py::_parse_and_validate
pub(crate) fn is_valid_date_str(date_input: &str, earliest: &DateTime, latest: &DateTime) -> bool {
    positional_ymd(date_input)
        .or_else(|| DateTime::strptime_ymd(date_input))
        .is_some_and(|date| is_valid_datetime(&date, earliest, latest))
}

/// `validate_and_convert`: `date` in `format` when it is in range, `None` otherwise.
// upstream: validators.py::validate_and_convert
pub(crate) fn validate_and_convert(
    date_input: Option<&DateTime>,
    format: Format,
    earliest: &DateTime,
    latest: &DateTime,
) -> Option<String> {
    let date = date_input?;
    is_valid_datetime(date, earliest, latest).then(|| format.render(date))
}

/// `correct_year`: a two-digit year below 90 is 20xx, from 90 on 19xx; other years stay.
// upstream: validators.py::correct_year
pub(crate) fn correct_year(year: i64) -> i64 {
    if year < 100 {
        year + if year >= 90 { 1900 } else { 2000 }
    } else {
        year
    }
}

/// `plausible_year_filter`: the matches of `pattern` in `htmlstring`, counted in first-occurrence order, with
/// the ones whose year (group 1 of `yearpat`, corrected) lies outside `earliest` to `latest` removed.
// upstream: validators.py::plausible_year_filter
pub(crate) fn plausible_year_filter(
    htmlstring: &str,
    pattern: &Pattern,
    yearpat: &Pattern,
    earliest: &DateTime,
    latest: &DateTime,
    call: &Call<'_>,
) -> Result<Counter, ExtractError> {
    let mut occurrences = Counter::default();
    let mut visited = 0;
    for item in pattern.findall(htmlstring, call)? {
        tick(&mut visited, call)?;
        occurrences.add(item, 1);
    }
    let mut plausible = Counter::default();
    for (item, count) in occurrences.iter() {
        tick(&mut visited, call)?;
        // not a year pattern: the item is dropped
        let Some(year_match) = yearpat.search(item, call)? else {
            continue;
        };
        let year_text = year_match.get(1).map_or("", |group| group.as_str());
        let potential_year = correct_year(year_value(year_text)?);
        if (i64::from(earliest.year)..=i64::from(latest.year)).contains(&potential_year) {
            plausible.add(item, count);
        }
    }
    Ok(plausible)
}

/// `update_reference`: the older date when `original`, else the newer one, compared by wall clock. A tie keeps
/// the reference, as Python's `min` and `max` return their first item.
// upstream: validators.py::update_reference
pub(crate) fn update_reference(
    reference: Option<DateTime>,
    candidate: DateTime,
    original: bool,
) -> DateTime {
    let Some(reference) = reference else {
        return candidate;
    };
    let (candidate_clock, reference_clock) = (candidate.wall_clock(), reference.wall_clock());
    let candidate_wins = if original {
        candidate_clock < reference_clock
    } else {
        candidate_clock > reference_clock
    };
    if candidate_wins { candidate } else { reference }
}

/// `compare_values`: folds the date of `attempt` (in `REFERENCE_FORMAT`) into `reference`.
// upstream: validators.py::compare_values
pub(crate) fn compare_values(
    reference: Option<DateTime>,
    attempt: &str,
    options: &Extractor,
) -> Option<DateTime> {
    let Some(candidate) = DateTime::from_isoformat(attempt) else {
        // The `strptime(REFERENCE_FORMAT)` fallback reads only what `strftime_reference` writes for a year in
        // range, and `fromisoformat` reads all of that, so it cannot succeed here.
        return reference;
    };
    Some(update_reference(reference, candidate, options.original))
}

/// `convert_date`: a `%Y-%m-%d` string read and written again. `None` where `strptime` refuses it.
// upstream: validators.py::convert_date
pub(crate) fn convert_date(datestring: &str) -> Option<String> {
    DateTime::strptime_ymd(datestring).map(|date| date.strftime_ymd())
}

/// `filter_ymd_candidate`: the best match's YMD groups as `%Y-%m-%d`, when that date is in range and its year
/// is at least `copyear` (`0` for none).
// upstream: validators.py::filter_ymd_candidate
pub(crate) fn filter_ymd_candidate(
    bestmatch: Option<[Option<&str>; 3]>,
    copyear: i64,
    min_date: &DateTime,
    max_date: &DateTime,
) -> Result<Option<String>, ExtractError> {
    let Some(groups) = bestmatch else {
        return Ok(None);
    };
    // unreachable: `plausible_year_filter` keeps only matches whose year pattern matched, so no group is None
    let [Some(year), Some(month), Some(day)] = groups else {
        return Err(ExtractError::Rejected(Rejection::InvalidValue));
    };
    let pagedate = format!("{year}-{month}-{day}");
    if !is_valid_date_str(&pagedate, min_date, max_date) {
        return Ok(None);
    }
    if copyear != 0 && year_value(year)? < copyear {
        return Ok(None);
    }
    // unreachable: the digit groups of a candidate read the same by position and by `strptime`
    convert_date(&pagedate)
        .map(Some)
        .ok_or(ExtractError::Rejected(Rejection::InvalidValue))
}

/// `check_extracted_reference`: `reference` in `%Y-%m-%d` when it is in the extractor's range.
// upstream: validators.py::check_extracted_reference
pub(crate) fn check_extracted_reference(
    reference: Option<&DateTime>,
    options: &Extractor,
) -> Option<String> {
    validate_and_convert(reference, Format::Ymd, &options.min, &options.max)
}

/// `check_date_input`: the date of an ISO string, else `default` (no input, or a string that is no ISO date).
// upstream: validators.py::check_date_input
// Test-only: the table tests the rule; `get_max_date` inlines it so that the clock is read lazily.
#[cfg(test)]
pub(crate) fn check_date_input(date_object: Option<&str>, default: DateTime) -> DateTime {
    date_object
        .and_then(DateTime::from_isoformat)
        .unwrap_or(default)
}

/// The end of the current UTC day, naive: 23:59:59.999999.
// deviation: D-21 (UTC, not the local day; the clock is read here and by `today_utc`, D-15)
fn end_of_current_utc_day() -> DateTime {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    end_of_utc_day(i64::try_from(now).unwrap_or(i64::MAX))
}

/// The end of the UTC day that holds `now` (seconds since the Unix epoch), naive: 23:59:59.999999.
// Split from the clock read so that tests can fix the time of day.
pub(crate) fn end_of_utc_day(now: i64) -> DateTime {
    let day_start = now - now.rem_euclid(SECONDS_PER_DAY);
    DateTime::from_timestamp_utc(day_start).map_or(LAST_INSTANT, |day| DateTime {
        hour: 23,
        minute: 59,
        second: 59,
        microsecond: 999_999,
        offset: None,
        ..day
    })
}

/// `get_max_date`: the ISO `max_date` when it reads, else the end of the current UTC day. A string with an offset
/// keeps it. The clock is read only for the default.
// upstream: validators.py::get_max_date
// deviation: D-21 (the end of the current UTC day, not of the local day)
pub(crate) fn get_max_date(max_date: Option<&str>) -> DateTime {
    max_date
        .and_then(DateTime::from_isoformat)
        .unwrap_or_else(end_of_current_utc_day)
}
