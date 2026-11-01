//! The pieces of Python's `datetime` and `strptime` that htmldate calls: `datetime(y, m, d)`,
//! `fromisoformat`, `strptime(s, "%Y-%m-%d")`, `fromtimestamp(s, tz=timezone.utc)`, `timestamp()` and
//! `strftime`. The behavior is CPython 3.13 on Linux, the fixture platform.
//!
//! There is no local time zone (D-21): a naive value is read as UTC.

use std::sync::LazyLock;

use regex::Regex;

use crate::pystr;

#[cfg(test)]
mod cases;
#[cfg(test)]
mod tests;

/// A `datetime`: the wall clock, and the UTC offset in microseconds east of UTC (`None` for a naive
/// value). The offset is strictly between minus and plus 24 hours, as `datetime` requires.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct DateTime {
    pub year: u16,
    pub month: u8,
    pub day: u8,
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub microsecond: u32,
    pub offset: Option<i64>,
}

const SECONDS_PER_DAY: i64 = 86_400;
const MICROS_PER_SECOND: i128 = 1_000_000;
const MICROS_PER_DAY: i64 = SECONDS_PER_DAY * 1_000_000;
/// `datetime.MINYEAR` and `MAXYEAR`.
const MIN_YEAR: i64 = 1;
const MAX_YEAR: i64 = 9_999;
/// The timestamps `datetime.fromtimestamp(s, tz=timezone.utc)` accepts on Linux: 0001-01-01 00:00:00
/// to 9999-12-31 23:59:59.
const MIN_TIMESTAMP: i64 = -62_135_596_800;
const MAX_TIMESTAMP: i64 = 253_402_300_799;

/// `_strptime`'s regex for `%Y-%m-%d`, leftmost alternative first. `\d` is `Nd` digits, and the
/// match must reach the end of the string (checked by the caller).
// upstream: _strptime.py::TimeRE (%Y, %m, %d)
static STRPTIME_YMD_PATTERN: LazyLock<Option<Regex>> = LazyLock::new(|| {
    let digits = pystr::digit_class();
    Regex::new(&format!(
        r"\A(?P<Y>[{digits}]{{4}})-(?P<m>1[0-2]|0[1-9]|[1-9])-(?P<d>3[0-1]|[1-2][{digits}]|0[1-9]|[1-9]| [1-9])"
    ))
    .ok()
});

impl DateTime {
    /// `datetime(year, month, day)`: midnight, naive. `None` where `datetime` raises ValueError.
    // upstream: datetime.datetime (CPython)
    pub(crate) fn new(year: i64, month: i64, day: i64) -> Option<DateTime> {
        if !(MIN_YEAR..=MAX_YEAR).contains(&year) || !(1..=12).contains(&month) {
            return None;
        }
        if !(1..=days_in_month(year, month)).contains(&day) {
            return None;
        }
        Some(DateTime {
            year: u16::try_from(year).ok()?,
            month: u8::try_from(month).ok()?,
            day: u8::try_from(day).ok()?,
            hour: 0,
            minute: 0,
            second: 0,
            microsecond: 0,
            offset: None,
        })
    }

    /// `datetime.fromisoformat(s)`: a date, then optionally a separator and a time with an offset.
    ///
    /// The reference is CPython's C parser, which differs from the pure-Python `_pydatetime` (the
    /// oracle runs C): one character at the end of the date is the separator, whatever it is, and
    /// a time must follow it, so `"2020-01-01 "` is rejected. The time grammar is C's too: it takes
    /// `06.8` and `10:20:30:5`, and it ignores text left before a sign-and-offset zone.
    // upstream: datetime.fromisoformat (CPython Modules/_datetimemodule.c)
    pub(crate) fn from_isoformat(s: &str) -> Option<DateTime> {
        let bytes = s.as_bytes();
        // Shorter strings fail in `_sanitize_isoformat_str`.
        if bytes.len() < 7 {
            return None;
        }
        let separator = find_separator(bytes)?;
        let (year, month, day) = parse_isoformat_date(bytes, separator)?;
        let date = DateTime::new(year, month, day)?;
        if bytes.len() <= separator {
            return date.at([0; 4], None);
        }
        // The separator is one whole UTF-8 character.
        let lead = byte(bytes, separator);
        let width = match lead {
            0x00..=0x7f => 1,
            0xe0..=0xef => 3,
            0xf0..=0xff => 4,
            _ => 2,
        };
        let (time, offset) = parse_isoformat_time(bytes, separator + width)?;
        date.at(time, offset)
    }

    /// `datetime.strptime(s, "%Y-%m-%d")`: `%Y` is four `Nd` digits, and the whole string must match.
    // upstream: datetime.strptime (CPython Lib/_strptime.py)
    pub(crate) fn strptime_ymd(s: &str) -> Option<DateTime> {
        let captures = STRPTIME_YMD_PATTERN.as_ref()?.captures(s)?;
        if captures.get(0)?.end() != s.len() {
            return None;
        }
        DateTime::new(
            decimal_number(captures.name("Y")?.as_str())?,
            decimal_number(captures.name("m")?.as_str())?,
            decimal_number(captures.name("d")?.as_str())?,
        )
    }

    /// `datetime.fromtimestamp(seconds, tz=timezone.utc)`: the UTC wall clock with offset 0.
    // upstream: datetime.fromtimestamp (CPython)
    pub(crate) fn from_timestamp_utc(seconds: i64) -> Option<DateTime> {
        if !(MIN_TIMESTAMP..=MAX_TIMESTAMP).contains(&seconds) {
            return None;
        }
        let (year, month, day) = civil_from_days(seconds.div_euclid(SECONDS_PER_DAY));
        let second_of_day = seconds.rem_euclid(SECONDS_PER_DAY);
        let time = [
            second_of_day / 3_600,
            (second_of_day / 60) % 60,
            second_of_day % 60,
            0,
        ];
        DateTime::new(year, month, day)?.at(time, Some(0))
    }

    /// `timestamp()` in microseconds. A naive value is read as UTC (D-21).
    // upstream: datetime.timestamp (CPython)
    pub(crate) fn timestamp_micros(&self) -> i128 {
        let days = i128::from(days_from_civil(
            i64::from(self.year),
            i64::from(self.month),
            i64::from(self.day),
        ));
        let seconds = days * i128::from(SECONDS_PER_DAY)
            + i128::from(self.hour) * 3_600
            + i128::from(self.minute) * 60
            + i128::from(self.second);
        seconds * MICROS_PER_SECOND + i128::from(self.microsecond)
            - i128::from(self.offset.unwrap_or(0))
    }

    /// The fields in `datetime.replace(tzinfo=None)` order, the wall clock without the offset.
    pub(crate) fn wall_clock(&self) -> (u16, u8, u8, u8, u8, u8, u32) {
        (
            self.year,
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second,
            self.microsecond,
        )
    }

    /// `strftime("%Y-%m-%d")`. glibc writes `%Y` without padding, so a year below 1000 is short.
    // upstream: datetime.strftime (CPython, glibc strftime)
    pub(crate) fn strftime_ymd(&self) -> String {
        format!("{}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// `strftime("%Y-%m-%dT%H:%M:%S.%f%z")`, with the offset as `%z` writes it.
    // upstream: datetime.strftime (CPython, glibc strftime)
    pub(crate) fn strftime_reference(&self) -> String {
        format!(
            "{}-{:02}-{:02}T{:02}:{:02}:{:02}.{:06}{}",
            self.year,
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second,
            self.microsecond,
            self.zone(),
        )
    }

    /// The fields at `time` (`hour`, `minute`, `second`, `microsecond`) with `offset`, each checked
    /// the way `datetime` checks it. `None` where one is out of range.
    fn at(self, time: [i64; 4], offset: Option<i64>) -> Option<DateTime> {
        let [hour, minute, second, microsecond] = time;
        if !(0..=23).contains(&hour)
            || !(0..=59).contains(&minute)
            || !(0..=59).contains(&second)
            || !(0..=999_999).contains(&microsecond)
            || offset.is_some_and(|offset| offset.abs() >= MICROS_PER_DAY)
        {
            return None;
        }
        Some(DateTime {
            hour: u8::try_from(hour).ok()?,
            minute: u8::try_from(minute).ok()?,
            second: u8::try_from(second).ok()?,
            microsecond: u32::try_from(microsecond).ok()?,
            offset,
            ..self
        })
    }

    /// `%z`: `+HHMM`, with `SS` and `.ffffff` only where the offset has them; empty when naive.
    // upstream: datetime.strftime (CPython _wrap_strftime, _format_offset)
    fn zone(&self) -> String {
        let Some(offset) = self.offset else {
            return String::new();
        };
        let sign = if offset < 0 { '-' } else { '+' };
        let magnitude = offset.unsigned_abs();
        let hours = magnitude / 3_600_000_000;
        let minutes = (magnitude / 60_000_000) % 60;
        let seconds = (magnitude / 1_000_000) % 60;
        let micros = magnitude % 1_000_000;
        let mut zone = format!("{sign}{hours:02}{minutes:02}");
        if seconds != 0 || micros != 0 {
            zone.push_str(&format!("{seconds:02}"));
        }
        if micros != 0 {
            zone.push_str(&format!(".{micros:06}"));
        }
        zone
    }
}

/// `int(text)` for the digit groups of [`STRPTIME_YMD_PATTERN`]: `Nd` digits, and the one leading
/// space that `%d` may take, which `int` strips.
fn decimal_number(text: &str) -> Option<i64> {
    pystr::strip(text).chars().try_fold(0_i64, |value, c| {
        value
            .checked_mul(10)?
            .checked_add(i64::from(pystr::decimal_value(c)?))
    })
}

/// A byte as C reads it: past the end, the NUL that ends the string.
fn byte(s: &[u8], index: usize) -> u8 {
    s.get(index).copied().unwrap_or(0)
}

/// `parse_digits`: `count` ASCII digits from `index`, the index after them and their value.
// upstream: _datetimemodule.c::parse_digits
fn parse_digits(s: &[u8], mut index: usize, count: usize) -> Option<(usize, i64)> {
    let mut value = 0_i64;
    for _ in 0..count {
        let digit = byte(s, index).checked_sub(b'0')?;
        if digit > 9 {
            return None;
        }
        value = value.checked_mul(10)?.checked_add(i64::from(digit))?;
        index = index.checked_add(1)?;
    }
    Some((index, value))
}

/// `_find_isoformat_datetime_separator`: the byte where the date ends. The string is at least 7
/// bytes. `None` where the C parser fails outright.
// upstream: _datetimemodule.c::_find_isoformat_datetime_separator
fn find_separator(s: &[u8]) -> Option<usize> {
    let len = s.len();
    if len == 7 {
        return Some(7);
    }
    if byte(s, 4) == b'-' {
        if byte(s, 5) != b'W' {
            return Some(10);
        }
        if len > 8 && byte(s, 8) == b'-' {
            if len == 9 {
                return None;
            }
            let digit_at_10 = len > 10 && byte(s, 10).is_ascii_digit();
            return Some(if digit_at_10 { 8 } else { 10 });
        }
        return Some(8);
    }
    if byte(s, 4) == b'W' {
        let mut end = 7;
        while byte(s, end).is_ascii_digit() {
            end += 1;
        }
        return Some(if end < 9 {
            end
        } else if end % 2 == 0 {
            7
        } else {
            8
        });
    }
    Some(8)
}

/// `parse_isoformat_date`: `YYYY-MM-DD`, `YYYYMMDD`, `YYYY-Www-d` or `YYYYWwwd`. `separator` is the
/// byte where the date ends, which bounds the optional weekday.
// upstream: _datetimemodule.c::parse_isoformat_date
fn parse_isoformat_date(s: &[u8], separator: usize) -> Option<(i64, i64, i64)> {
    let (mut pos, year) = parse_digits(s, 0, 4)?;
    let uses_separator = byte(s, pos) == b'-';
    if uses_separator {
        pos += 1;
    }
    if byte(s, pos) == b'W' {
        pos += 1;
        let (next, week) = parse_digits(s, pos, 2)?;
        pos = next;
        let mut weekday = 1;
        if pos < separator {
            if uses_separator {
                if byte(s, pos) != b'-' {
                    return None;
                }
                pos += 1;
            }
            weekday = parse_digits(s, pos, 1)?.1;
        }
        return iso_to_ymd(year, week, weekday);
    }
    let (next, month) = parse_digits(s, pos, 2)?;
    pos = next;
    if uses_separator {
        if byte(s, pos) != b'-' {
            return None;
        }
        pos += 1;
    }
    let (_, day) = parse_digits(s, pos, 2)?;
    Some((year, month, day))
}

/// `iso_to_ymd`: the date of ISO `week` and `weekday` (1 for Monday) in `year`. Week 53 exists only
/// in a year that starts on a Thursday, or on a Wednesday when leap.
// upstream: _datetimemodule.c::iso_to_ymd
fn iso_to_ymd(year: i64, week: i64, weekday: i64) -> Option<(i64, i64, i64)> {
    if !(MIN_YEAR..=MAX_YEAR).contains(&year) || !(1..=7).contains(&weekday) {
        return None;
    }
    let jan1 = days_from_civil(year, 1, 1);
    let jan1_weekday = monday_based_weekday(jan1);
    if week == 53 {
        if !(jan1_weekday == 3 || (jan1_weekday == 2 && is_leap(year))) {
            return None;
        }
    } else if !(1..=52).contains(&week) {
        return None;
    }
    let week1_monday = if jan1_weekday > 3 {
        jan1 - jan1_weekday + 7
    } else {
        jan1 - jan1_weekday
    };
    Some(civil_from_days(
        week1_monday + (week - 1) * 7 + (weekday - 1),
    ))
}

/// `parse_hh_mm_ss_ff` over `s[start..end]`: `HH[:MM[:SS]]` with either every separator `:` or
/// none, then an optional fraction after any field. Reads past `end` see the bytes that follow. Returns
/// hour, minute, second and microsecond, and whether text is left over, which the caller decides on.
// upstream: _datetimemodule.c::parse_hh_mm_ss_ff
fn parse_hh_mm_ss_ff(s: &[u8], start: usize, end: usize) -> Option<([i64; 4], bool)> {
    let mut clock = [0_i64; 4];
    let mut index = start;
    let mut has_separator = true;
    for (field, slot) in clock.iter_mut().take(3).enumerate() {
        let (next, value) = parse_digits(s, index, 2)?;
        *slot = value;
        index = next;
        let c = byte(s, index);
        index += 1;
        if field == 0 {
            has_separator = c == b':';
        }
        if c == b'.' || c == b',' {
            if index >= end {
                return None;
            }
            break;
        } else if index >= end {
            return Some((clock, c != 0));
        } else if has_separator && c == b':' {
            continue;
        } else if !has_separator {
            index -= 1;
        } else {
            return None;
        }
    }
    let to_parse = end.saturating_sub(index).min(6);
    let (next, mut microsecond) = parse_digits(s, index, to_parse)?;
    index = next;
    for _ in to_parse..6 {
        microsecond *= 10;
    }
    clock[3] = microsecond;
    while byte(s, index).is_ascii_digit() {
        index += 1;
    }
    Some((clock, byte(s, index) != 0))
}

/// `parse_isoformat_time`: the time from `start` to the end, then optionally `Z` (last) or a sign and
/// an offset of the same shape. Returns the time fields and the offset in microseconds (`None` when
/// naive).
// upstream: _datetimemodule.c::parse_isoformat_time
fn parse_isoformat_time(s: &[u8], start: usize) -> Option<([i64; 4], Option<i64>)> {
    let end = s.len();
    let zone = (start..end)
        .find(|&i| matches!(byte(s, i), b'Z' | b'+' | b'-'))
        .unwrap_or(end);
    let (clock, leftover) = parse_hh_mm_ss_ff(s, start, zone)?;
    if zone == end {
        return (!leftover).then_some((clock, None));
    }
    if byte(s, zone) == b'Z' {
        return (byte(s, zone + 1) == 0).then_some((clock, Some(0)));
    }
    let sign = if byte(s, zone) == b'-' { -1 } else { 1 };
    let (offset_clock, offset_leftover) = parse_hh_mm_ss_ff(s, zone + 1, end)?;
    if offset_leftover {
        return None;
    }
    // Two-digit fields and a microsecond count: the sum stays far inside i64.
    let seconds = offset_clock[0] * 3_600 + offset_clock[1] * 60 + offset_clock[2];
    Some((clock, Some(sign * (seconds * 1_000_000 + offset_clock[3]))))
}

fn is_leap(year: i64) -> bool {
    year % 4 == 0 && (year % 100 != 0 || year % 400 == 0)
}

/// Days in `month` (1 to 12) of `year`.
fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Monday is 0 and Sunday 6. 1970-01-01 was a Thursday.
fn monday_based_weekday(days: i64) -> i64 {
    (days + 3).rem_euclid(7)
}

/// Days from 1970-01-01 to the proleptic Gregorian date, Howard Hinnant's algorithm. The fields
/// are in range: callers check them first.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_from_march = (month + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// The (year, month, day) of `days` after 1970-01-01, the inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_from_march = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_from_march + 2) / 5 + 1;
    let month = if month_from_march < 10 {
        month_from_march + 3
    } else {
        month_from_march - 9
    };
    let year = year_of_era + era * 400;
    (if month <= 2 { year + 1 } else { year }, month, day)
}
