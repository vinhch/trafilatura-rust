//! Python string semantics where Rust's differ: whitespace, `splitlines`, `isprintable`, `isalpha`,
//! `lower`, `title`, `isupper`, `repr`, the `unicode_escape` decode, `\w` and decimal digits.
//!
//! The tables are generated from the interpreter (`tools/parity/parity/gen_printable.py` and
//! `gen_decimal.py`); the functions only search them.

mod alnum;
mod alpha;
mod decimal;
mod lower;
mod printable;
mod title;

use std::cmp::Ordering;

use regex::Regex;

use crate::error::{ExtractError, Rejection};
use alnum::{ALNUM, PUNCTUATION};
use alpha::ALPHA;
use decimal::{DIGIT, ND_ZEROS};
use lower::{CASE_IGNORABLE, CASED, LOWER};
use printable::{PRINTABLE, SPACE, WORD};
pub use printable::{PY_SPACE_CLASS, PY_WORD_CLASS};
use title::{CASED_PROPERTY, TITLE, UPPER};

/// Python's `int()` refuses a decimal string of more digits than this
/// (`sys.int_info.default_max_str_digits`); bases that are powers of two, such as hex, have no
/// limit. Leading zeros count.
pub const MAX_STR_DIGITS: usize = 4300;

/// Whether `c` lies in one of the sorted, disjoint inclusive `(start, end)` ranges of `table`.
pub(crate) fn in_ranges(table: &[(u32, u32)], c: char) -> bool {
    let c = u32::from(c);
    table
        .binary_search_by(|&(start, end)| {
            if end < c {
                Ordering::Less
            } else if start > c {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        })
        .is_ok()
}

/// Python `str.isspace` for one character: `\x1c`-`\x1f` count, which Rust's `is_whitespace` misses.
pub(crate) fn is_space(c: char) -> bool {
    if c.is_ascii() {
        return matches!(c, '\t'..='\r' | '\x1c'..=' ');
    }
    in_ranges(SPACE, c)
}

/// Python `s.isspace()`: false for the empty string.
pub(crate) fn is_all_space(s: &str) -> bool {
    !s.is_empty() && s.chars().all(is_space)
}

/// Python `s.split()`: the runs of non-whitespace, no empty parts.
pub(crate) fn split_whitespace(s: &str) -> impl Iterator<Item = &str> {
    s.split(is_space).filter(|part| !part.is_empty())
}

/// Python `s.strip()` with no argument.
pub(crate) fn strip(s: &str) -> &str {
    s.trim_matches(is_space)
}

/// Python `s.lstrip()` with no argument.
// upstream: str.lstrip (CPython)
pub(crate) fn lstrip(s: &str) -> &str {
    s.trim_start_matches(is_space)
}

/// Python `s.rstrip()` with no argument.
// upstream: str.rstrip (CPython)
pub(crate) fn rstrip(s: &str) -> &str {
    s.trim_end_matches(is_space)
}

/// The canonical decimal string of Python `int(s)`: no leading zeros, `-` only for a nonzero value.
///
/// Fails with `InvalidValue` where `int` raises `ValueError`, including more than
/// [`MAX_STR_DIGITS`] digits (leading zeros counted, sign and `_` not). `int` strips ASCII
/// `" \t\n\v\f\r"` and the non-ASCII `isspace` characters, but not `\x1c`-`\x1f`, so this is not
/// [`strip`].
// upstream: int (CPython)
pub(crate) fn int_str(s: &str) -> Result<String, ExtractError> {
    let invalid = || ExtractError::Rejected(Rejection::InvalidValue);
    let trimmed = s.trim_matches(|c: char| {
        if c.is_ascii() {
            matches!(c, ' ' | '\t' | '\n' | '\x0b' | '\x0c' | '\r')
        } else {
            is_space(c)
        }
    });
    let (negative, body) = match trimmed.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, trimmed.strip_prefix('+').unwrap_or(trimmed)),
    };
    let mut digits = String::new();
    // A `_` is valid only right after a digit, and the string must end on a digit.
    let mut after_digit = false;
    for c in body.chars() {
        if let Some(value) = decimal_value(c) {
            if digits.len() == MAX_STR_DIGITS {
                return Err(invalid());
            }
            digits.push(char::from_digit(value, 10).ok_or_else(invalid)?);
            after_digit = true;
        } else if c == '_' && after_digit {
            after_digit = false;
        } else {
            return Err(invalid());
        }
    }
    if !after_digit {
        return Err(invalid());
    }
    let magnitude = digits.trim_start_matches('0');
    Ok(if magnitude.is_empty() {
        "0".to_owned()
    } else if negative {
        format!("-{magnitude}")
    } else {
        magnitude.to_owned()
    })
}

fn is_line_break(c: char) -> bool {
    matches!(
        c,
        '\n' | '\r'
            | '\x0b'
            | '\x0c'
            | '\x1c'
            | '\x1d'
            | '\x1e'
            | '\u{85}'
            | '\u{2028}'
            | '\u{2029}'
    )
}

/// Python `s.splitlines()`: lines without their terminator; `\r\n` is one break, and a final
/// terminator adds no empty line.
pub fn splitlines(s: &str) -> impl Iterator<Item = &str> {
    let mut rest = s;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let (line, after) = match rest
            .find(is_line_break)
            .and_then(|i| rest.split_at_checked(i))
        {
            Some((line, tail)) => {
                let mut chars = tail.chars();
                let separator = chars.next();
                let after = chars.as_str();
                let after = match separator {
                    Some('\r') => after.strip_prefix('\n').unwrap_or(after),
                    _ => after,
                };
                (line, after)
            }
            None => (rest, ""),
        };
        rest = after;
        Some(line)
    })
}

/// Python `str.isprintable` for one character.
pub(crate) fn is_printable(c: char) -> bool {
    if c.is_ascii() {
        return matches!(c, ' '..='~');
    }
    in_ranges(PRINTABLE, c)
}

/// Python `s.isprintable()`: true for the empty string.
pub(crate) fn is_all_printable(s: &str) -> bool {
    s.chars().all(is_printable)
}

/// Python `str.isalpha` for one character: the categories `Lm`, `Lt`, `Lu`, `Ll` and `Lo`. Rust's
/// `char::is_alphabetic` also takes `Nl` and `Other_Alphabetic` marks.
pub fn is_alpha(c: char) -> bool {
    in_ranges(ALPHA, c)
}

/// Python `str.isalnum` for one character: `isalpha`, `isdecimal`, `isdigit` or `isnumeric`. Rust's
/// `char::is_alphanumeric` follows a newer Unicode version.
pub(crate) fn is_alnum(c: char) -> bool {
    in_ranges(ALNUM, c)
}

/// Unicode general category `P*` (punctuation), as `unicodedata.category(c)[0] == "P"` gives it.
pub(crate) fn is_punctuation(c: char) -> bool {
    in_ranges(PUNCTUATION, c)
}

/// Python `str.lower()`, by the Unicode data of the interpreter, not of the Rust standard library:
/// that follows a newer Unicode version and lowercases characters Python leaves alone. U+0130 becomes
/// `i` and a combining dot above; a capital sigma becomes a final sigma after a cased letter and before
/// none, case-ignorable characters skipped.
// upstream: cpython Objects/unicodeobject.c::do_lower
pub fn lower(s: &str) -> String {
    let mut lowered = String::with_capacity(s.len());
    for (index, c) in s.char_indices() {
        push_lower(&mut lowered, s, index, c);
    }
    lowered
}

/// Appends the lowercase of `c`, the character of `s` at byte `index`.
fn push_lower(out: &mut String, s: &str, index: usize, c: char) {
    match u32::from(c) {
        CAPITAL_SIGMA => out.push(if is_final_sigma(s, index) {
            FINAL_SIGMA
        } else {
            SMALL_SIGMA
        }),
        CAPITAL_I_WITH_DOT => out.push_str("i\u{307}"),
        code if code < 0x80 => out.push(c.to_ascii_lowercase()),
        code => out.push(
            LOWER
                .binary_search_by_key(&code, |&(upper, _)| upper)
                .ok()
                .and_then(|i| LOWER.get(i))
                .and_then(|&(_, lower)| char::from_u32(lower))
                .unwrap_or(c),
        ),
    }
}

/// Python `s.title()`: each character after a non-cased one takes its full title mapping (`ß` becomes
/// `Ss`), every other its lowercase, final sigma included. "Cased" counts the case-ignorable
/// characters too (U+0345 keeps the next letter lowercase), unlike the class `lower` skips.
// upstream: cpython Objects/unicodeobject.c::do_title
pub fn title(s: &str) -> String {
    let mut titled = String::with_capacity(s.len());
    let mut previous_is_cased = false;
    for (index, c) in s.char_indices() {
        if previous_is_cased {
            push_lower(&mut titled, s, index, c);
        } else {
            match TITLE
                .binary_search_by_key(&u32::from(c), |&(code, _)| code)
                .ok()
                .and_then(|i| TITLE.get(i))
            {
                Some(&(_, mapped)) => titled.push_str(mapped),
                None => titled.push(c),
            }
        }
        previous_is_cased = in_ranges(CASED_PROPERTY, c);
    }
    titled
}

/// Python `str.isupper` for one character.
pub fn is_upper(c: char) -> bool {
    in_ranges(UPPER, c)
}

const CAPITAL_SIGMA: u32 = 0x3A3;
const CAPITAL_I_WITH_DOT: u32 = 0x130;
const SMALL_SIGMA: char = '\u{3C3}';
const FINAL_SIGMA: char = '\u{3C2}';

// upstream: cpython Objects/unicodeobject.c::handle_capital_sigma
fn is_final_sigma(s: &str, sigma_index: usize) -> bool {
    let is_skipped = |c: &char| in_ranges(CASE_IGNORABLE, *c);
    let before = s
        .get(..sigma_index)
        .and_then(|head| head.chars().rev().find(|c| !is_skipped(c)));
    let after = s
        .get(sigma_index..)
        .and_then(|tail| tail.chars().skip(1).find(|c| !is_skipped(c)));
    before.is_some_and(|c| in_ranges(CASED, c)) && !after.is_some_and(|c| in_ranges(CASED, c))
}

/// Python `repr(s)` of a `str`: single quotes unless `s` holds a `'` and no `"`, backslash escapes
/// for the quote, `\\`, `\t`, `\n`, `\r` and every character that is not printable
/// (`\xNN`, `\uNNNN` or `\UNNNNNNNN`, lowercase hex).
// upstream: repr (CPython)
pub fn repr(s: &str) -> String {
    let quote = if s.contains('\'') && !s.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::with_capacity(s.len().saturating_add(2));
    out.push(quote);
    for c in s.chars() {
        let code = u32::from(c);
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ if c == quote => {
                out.push('\\');
                out.push(c);
            }
            ' '..='~' => out.push(c),
            _ if c.is_ascii() => out.push_str(&format!("\\x{code:02x}")),
            _ if is_printable(c) => out.push(c),
            _ if code <= 0xff => out.push_str(&format!("\\x{code:02x}")),
            _ => {
                let (marker, width) = if code <= 0xffff { ('u', 4) } else { ('U', 8) };
                out.push('\\');
                out.push(marker);
                out.push_str(&format!("{code:0width$x}"));
            }
        }
    }
    out.push(quote);
    out
}

/// Python `re` `\w` in Unicode mode: alphanumerics (`isalnum`, so `\u{b2}` counts) and `_`; Rust's
/// `regex` `\w` adds marks and connector punctuation instead.
pub(crate) fn is_word(c: char) -> bool {
    in_ranges(WORD, c)
}

/// Python `str.isdigit` for one character: `Nd` and the digits that `int` refuses, such as the
/// superscripts `\u{b2}` and `\u{b9}`.
// upstream: str.isdigit (CPython)
pub(crate) fn is_digit(c: char) -> bool {
    in_ranges(DIGIT, c)
}

/// `unicodedata.decimal(c, None)`: the value of a decimal digit (category `Nd`) of any script.
pub fn decimal_value(c: char) -> Option<u32> {
    // Every digit sits in a run of ten consecutive code points that starts at its zero.
    let after = ND_ZEROS.partition_point(|&zero| zero <= c);
    let zero = ND_ZEROS.get(after.checked_sub(1)?)?;
    Some(u32::from(c) - u32::from(*zero)).filter(|&value| value < 10)
}

/// Python `s.isdecimal()`: false for the empty string.
pub fn is_decimal(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| decimal_value(c).is_some())
}

/// Python `re` `\d` in Unicode mode as the body of a regex class: the characters of
/// [`decimal_value`], where Rust's `\d` follows a newer Unicode version.
pub(crate) fn digit_class() -> String {
    ND_ZEROS
        .iter()
        .filter_map(|&zero| {
            let nine = u32::from(zero).checked_add(9)?;
            Some(format!(r"\x{{{:X}}}-\x{{{nine:X}}}", u32::from(zero)))
        })
        .collect()
}

/// Python `s.encode().decode("unicode_escape")`: the UTF-8 bytes read as Latin-1 between escapes, so a
/// non-ASCII character comes out as mojibake, as upstream's does. `None` where Python raises
/// `UnicodeDecodeError` (a truncated or invalid `\x`, `\u` or `\U`, a trailing backslash, a code point
/// past U+10FFFF) and for `\N{name}`, which needs the Unicode name database.
// deviation: D-20 (`\N{name}` is `None`; an escape that yields a surrogate decodes to U+FFFF)
// upstream: cpython Objects/unicodeobject.c::_PyUnicode_DecodeUnicodeEscapeInternal
pub fn unicode_escape(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while let Some(&byte) = bytes.get(i) {
        i += 1;
        if byte != b'\\' {
            out.push(char::from(byte));
            continue;
        }
        let escape = *bytes.get(i)?;
        i += 1;
        let simple = match escape {
            b'\n' => Some(None),
            b'\\' => Some(Some('\\')),
            b'\'' => Some(Some('\'')),
            b'"' => Some(Some('"')),
            b'a' => Some(Some('\x07')),
            b'b' => Some(Some('\x08')),
            b'f' => Some(Some('\x0c')),
            b'n' => Some(Some('\n')),
            b'r' => Some(Some('\r')),
            b't' => Some(Some('\t')),
            b'v' => Some(Some('\x0b')),
            _ => None,
        };
        if let Some(decoded) = simple {
            out.extend(decoded);
            continue;
        }
        let digits = match escape {
            b'0'..=b'7' => {
                let mut value = u32::from(escape - b'0');
                for _ in 0..2 {
                    let Some(&d @ b'0'..=b'7') = bytes.get(i) else {
                        break;
                    };
                    value = value * 8 + u32::from(d - b'0');
                    i += 1;
                }
                out.push(char::from_u32(value)?);
                continue;
            }
            b'x' => 2,
            b'u' => 4,
            b'U' => 8,
            b'N' => return None,
            _ => {
                // An unknown escape keeps its backslash; the next byte is read as itself.
                out.push('\\');
                i -= 1;
                continue;
            }
        };
        let hex = bytes.get(i..i + digits)?;
        i += digits;
        let mut value: u32 = 0;
        for &d in hex {
            value = value * 16 + char::from(d).to_digit(16)?;
        }
        if value > 0x10FFFF {
            return None;
        }
        out.push(char::from_u32(value).unwrap_or('\u{FFFF}'));
    }
    Some(out)
}

// Python's `re.IGNORECASE` also matches `i` with `İ` and `ı`, and `ı` and `İ` with `i`, `I`, `ı` and `İ`; Rust's
// `(?i)` does not. Each `i` and each `ı` or `İ` becomes the class of its Python matches.
pub(crate) fn ignore_case_i(pattern: &str) -> String {
    let mut out = String::with_capacity(pattern.len());
    for c in pattern.chars() {
        match c {
            'i' | 'I' => out.push_str("[iİı]"),
            'ı' | 'İ' => out.push_str("[iIıİ]"),
            _ => out.push(c),
        }
    }
    out
}

/// A pattern of upstream with `\w`, `\s` and `\d` written as `@W@`, `@S@` and `@D@`, which stand for Python's
/// classes. `None` where the pattern does not compile.
// upstream: re.compile (Unicode str patterns)
pub(crate) fn compile_py(pattern: &str) -> Option<Regex> {
    Regex::new(
        &pattern
            .replace("@W@", PY_WORD_CLASS)
            .replace("@S@", PY_SPACE_CLASS)
            .replace("@D@", &digit_class()),
    )
    .ok()
}

#[cfg(test)]
mod tests;
