//! Python string semantics where Rust's differ: whitespace, `splitlines`, `isprintable`, `isalpha`,
//! `lower`, `repr`, `\w` and decimal digits.
//!
//! The tables are generated from the interpreter (`tools/parity/parity/gen_printable.py` and
//! `gen_decimal.py`); the functions only search them.

mod alpha;
mod decimal;
mod lower;
mod printable;

use std::cmp::Ordering;

use crate::error::{ExtractError, Rejection};
use alpha::ALPHA;
use decimal::ND_ZEROS;
use lower::{CASE_IGNORABLE, CASED, LOWER};
use printable::{PRINTABLE, SPACE, WORD};
pub use printable::{PY_SPACE_CLASS, PY_WORD_CLASS};

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

/// Python `str.lower()`, by the Unicode data of the interpreter, not of the Rust standard library:
/// that follows a newer Unicode version and lowercases characters Python leaves alone. U+0130 becomes
/// `i` and a combining dot above; a capital sigma becomes a final sigma after a cased letter and before
/// none, case-ignorable characters skipped.
// upstream: cpython Objects/unicodeobject.c::do_lower
pub fn lower(s: &str) -> String {
    let mut lowered = String::with_capacity(s.len());
    for (index, c) in s.char_indices() {
        match u32::from(c) {
            CAPITAL_SIGMA => lowered.push(if is_final_sigma(s, index) {
                FINAL_SIGMA
            } else {
                SMALL_SIGMA
            }),
            CAPITAL_I_WITH_DOT => lowered.push_str("i\u{307}"),
            code if code < 0x80 => lowered.push(c.to_ascii_lowercase()),
            code => lowered.push(
                LOWER
                    .binary_search_by_key(&code, |&(upper, _)| upper)
                    .ok()
                    .and_then(|i| LOWER.get(i))
                    .and_then(|&(_, lower)| char::from_u32(lower))
                    .unwrap_or(c),
            ),
        }
    }
    lowered
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

// Python's `re.IGNORECASE` also matches `İ` and `ı` for `i`, which Rust's `(?i)` does not.
pub(crate) fn ignore_case_i(pattern: &str) -> String {
    pattern.replace(['i', 'I'], "[iİı]")
}

#[cfg(test)]
mod tests {
    use regex::Regex;

    use super::*;

    // oracle: [hex(c) for c in range(0x110000) if chr(c).isspace()]
    const PYTHON_SPACE: &[u32] = &[
        0x9, 0xa, 0xb, 0xc, 0xd, 0x1c, 0x1d, 0x1e, 0x1f, 0x20, 0x85, 0xa0, 0x1680, 0x2000, 0x2001,
        0x2002, 0x2003, 0x2004, 0x2005, 0x2006, 0x2007, 0x2008, 0x2009, 0x200a, 0x2028, 0x2029,
        0x202f, 0x205f, 0x3000,
    ];

    fn every_char() -> impl Iterator<Item = char> {
        (0..=0x10FFFF).filter_map(char::from_u32)
    }

    #[test]
    fn is_space_matches_python() {
        let rust_misses: Vec<char> = PYTHON_SPACE
            .iter()
            .filter_map(|&cp| char::from_u32(cp))
            .filter(|c| !c.is_whitespace())
            .collect();
        // Rust's `char::is_whitespace` disagrees on exactly these.
        assert_eq!(rust_misses, ['\x1c', '\x1d', '\x1e', '\x1f']);
        let ours: Vec<u32> = every_char()
            .filter(|&c| is_space(c))
            .map(u32::from)
            .collect();
        assert_eq!(ours, PYTHON_SPACE);
        // oracle: chr(0x200b).isspace(), chr(0xfeff).isspace()
        assert!(!is_space('\u{200b}') && !is_space('\u{feff}'));
    }

    #[test]
    fn is_all_space_is_false_for_empty() {
        assert!(!is_all_space("")); // oracle: "".isspace()
        assert!(is_all_space(" \x1c\u{a0}\n")); // oracle: " \x1c\xa0\n".isspace()
        assert!(!is_all_space(" a ")); // oracle: " a ".isspace()
    }

    #[test]
    fn split_whitespace_and_strip_match_python() {
        // oracle: "  a\xa0b\x1cc \x1f ".split()
        let parts: Vec<&str> = split_whitespace("  a\u{a0}b\x1cc \x1f ").collect();
        assert_eq!(parts, ["a", "b", "c"]);
        // oracle: " \t".split()
        assert_eq!(split_whitespace(" \t").count(), 0);
        // oracle: " \x1c\xa0a b\x85 ".strip()
        assert_eq!(strip(" \x1c\u{a0}a b\u{85} "), "a b");
        // oracle: chr(0x200b).strip() == chr(0x200b)
        assert_eq!(strip("\u{200b}"), "\u{200b}");
    }

    #[test]
    fn lstrip_rstrip_match_python() {
        // oracle: "\x1c a \x1f".lstrip(), "\x1c a \x1f".rstrip(), "\x85　".lstrip()
        assert_eq!(lstrip("\x1c a \x1f"), "a \x1f");
        assert_eq!(rstrip("\x1c a \x1f"), "\x1c a");
        assert_eq!(lstrip("\u{85}\u{3000}"), "");
        assert_eq!(rstrip("\u{85}\u{3000}"), "");
    }

    #[test]
    fn int_str_matches_python() {
        let invalid = Err(ExtractError::Rejected(Rejection::InvalidValue));
        // oracle: str(int(s)), or the exception name
        let long_underscored = "1_".repeat(2500) + "1";
        let long_negative = format!("-{}7", "0".repeat(4299));
        let too_long = format!("{}1", "0".repeat(4300));
        let (expected_long, zeros) = ("1".repeat(2501), "0".repeat(4300));
        for (s, expected) in [
            ("2", "2"),
            (" 1_0 ", "10"),
            ("-0", "0"),
            ("+5", "5"),
            ("-007", "-7"),
            ("\u{661}\u{662}", "12"),
            ("\u{FF11}", "1"),
            ("\u{85}1\u{3000}", "1"),
            (long_underscored.as_str(), expected_long.as_str()),
            (long_negative.as_str(), "-7"),
            ("1_2_3", "123"),
            (" \t\n\x0b\x0c\r7\r", "7"),
            ("\u{663}_\u{664}", "34"),
            (zeros.as_str(), "0"),
        ] {
            assert_eq!(
                int_str(s).as_deref(),
                Ok(expected),
                "{:?}",
                s.chars().take(20).collect::<String>()
            );
        }
        for s in [
            "",
            "x",
            "1__0",
            "_1",
            "1_",
            "1 2",
            "\x1c1\x1f",
            "\x1f7",
            "\u{B2}",
            "+",
            "-",
            "- 1",
            "+-1",
            too_long.as_str(),
        ] {
            assert_eq!(
                int_str(s),
                invalid,
                "{:?}",
                s.chars().take(20).collect::<String>()
            );
        }
    }

    #[test]
    fn splitlines_matches_python() {
        // oracle: "a\nb\rc\r\nd\x0be\x0cf\x1cg\x85h i j".splitlines()
        let parts: Vec<&str> = splitlines("a\nb\rc\r\nd\x0be\x0cf\x1cg\u{85}h i j").collect();
        assert_eq!(parts, ["a", "b", "c", "d", "e", "f", "g", "h i j"]);
        // oracle: ("a\x1db\x1ec" + chr(0x2028) + "d" + chr(0x2029) + "e\n").splitlines()
        let parts: Vec<&str> = splitlines("a\x1db\x1ec\u{2028}d\u{2029}e\n").collect();
        assert_eq!(parts, ["a", "b", "c", "d", "e"]);
        // oracle: ("a\x1fb" + chr(0x200b) + "c").splitlines()
        assert_eq!(
            splitlines("a\x1fb\u{200b}c").collect::<Vec<_>>(),
            ["a\x1fb\u{200b}c"]
        );
    }

    #[test]
    fn splitlines_edges_match_python() {
        let lines = |s| splitlines(s).collect::<Vec<_>>();
        assert!(lines("").is_empty()); // oracle: "".splitlines()
        assert_eq!(lines("\n"), [""]); // oracle: "\n".splitlines()
        assert_eq!(lines("a\n\nb"), ["a", "", "b"]); // oracle: "a\n\nb".splitlines()
        assert_eq!(lines("a\r\n\r\n"), ["a", ""]); // oracle: "a\r\n\r\n".splitlines()
        assert_eq!(lines("\r\n"), [""]); // oracle: "\r\n".splitlines()
    }

    #[test]
    fn is_printable_matches_python() {
        assert!(is_printable(' ')); // oracle: " ".isprintable()
        assert!(!is_printable('\u{a0}')); // oracle: "\xa0".isprintable()
        assert!(!is_printable('\u{200b}')); // oracle: chr(0x200b).isprintable()
        assert!(!is_printable('\u{378}')); // oracle: chr(0x378).isprintable()
        assert!(is_printable('\u{1F600}')); // oracle: chr(0x1F600).isprintable()
        assert!(!is_printable('\x7f')); // oracle: "\x7f".isprintable()
        assert!(!is_printable('\u{ad}')); // oracle: "\xad".isprintable()
        assert!(!is_printable('\u{2028}')); // oracle: chr(0x2028).isprintable()
        assert!(!is_printable('\u{e000}')); // oracle: chr(0xe000).isprintable()
        assert!(!is_printable('\u{e0001}')); // oracle: chr(0xe0001).isprintable()
    }

    #[test]
    fn lower_matches_python() {
        // oracle: [s.lower() for s in ("ABİc", "ΑΣ", "Σ", "Α.ΣΣ", "\U00010d50", "\U00010570")]
        assert_eq!(lower("AB\u{130}c"), "ab\u{69}\u{307}c");
        assert_eq!(lower("\u{391}\u{3A3}"), "\u{3B1}\u{3C2}");
        assert_eq!(lower("\u{3A3}"), "\u{3C3}");
        assert_eq!(lower("\u{391}.\u{3A3}\u{3A3}"), "\u{3B1}.\u{3C3}\u{3C2}");
        assert_eq!(lower("\u{10D50}"), "\u{10D50}");
        assert_eq!(lower("\u{10570}"), "\u{10597}");
        // oracle: chr(0x10d50).lower() is unchanged, while Rust's `to_lowercase` maps it
        assert_ne!("\u{10D50}".to_lowercase(), "\u{10D50}");
    }

    #[test]
    fn is_alpha_matches_python() {
        // oracle: [c.isalpha() for c in (chr(0x2160), chr(0x345), chr(0x1c5), chr(0xe9))]
        assert!(!is_alpha('\u{2160}'));
        assert!(!is_alpha('\u{345}'));
        assert!(is_alpha('\u{1c5}'));
        assert!(is_alpha('\u{e9}'));
        // oracle: chr(0x2160).isalpha() is False, while Rust's `is_alphabetic` says true
        assert!('\u{2160}'.is_alphabetic());
        // oracle: [(c, chr(c).isalpha()) for c in range(0x110000)], by the edges of its true runs
        for &(start, end) in ALPHA {
            for (code, expected) in [
                (start.checked_sub(1), false),
                (Some(start), true),
                (Some(end), true),
                (end.checked_add(1), false),
            ] {
                let Some(c) = code.and_then(char::from_u32) else {
                    continue;
                };
                assert_eq!(is_alpha(c), expected, "U+{:04X}", u32::from(c));
            }
        }
    }

    /// `repr` expected values name a backslash as `@`, so that no escape sequence stands in the source.
    fn at(s: &str) -> String {
        s.replace('@', "\\")
    }

    #[test]
    fn repr_matches_python() {
        // oracle: repr(s)
        for (s, expected) in [
            ("it's", at("\"it's\"")),
            ("say \"hi\"", at("'say \"hi\"'")),
            ("a\nb", at("'a@nb'")),
            ("\u{85}", at("'@x85'")),
            ("\u{e9}", at("'\u{e9}'")),
            ("it's \"x\"", at("'it@'s \"x\"'")),
            ("\u{2028}", at("'@u2028'")),
            ("\u{e0001}", at("'@U000e0001'")),
            ("a\\b", at("'a@@b'")),
            ("t\tc\r", at("'t@tc@r'")),
            ("\x7f\x1f\u{a0}", at("'@x7f@x1f@xa0'")),
            ("\u{ff}\u{100}", at("'\u{ff}\u{100}'")),
            ("'", at("\"'\"")),
            ("\"", at("'\"'")),
            ("", at("''")),
        ] {
            assert_eq!(repr(s), expected, "{s:?}");
        }
    }

    #[test]
    fn is_all_printable_is_true_for_empty() {
        assert!(is_all_printable("")); // oracle: "".isprintable()
        assert!(is_all_printable("a b\u{1F600}")); // oracle: ("a b" + chr(0x1F600)).isprintable()
        assert!(!is_all_printable("a\nb")); // oracle: "a\nb".isprintable()
    }

    #[test]
    fn is_word_matches_python() {
        // oracle: re.match(r"\w", c) is not None
        for (c, expected) in [
            ('\u{b2}', true),
            ('\u{301}', false),
            ('_', true),
            ('\u{203f}', false),
            ('a', true),
            ('\u{aa}', true),
            ('\u{2160}', true),
            ('\u{bd}', true),
            ('\u{660}', true),
            ('\u{200c}', false),
            ('\u{200d}', false),
            ('\u{b7}', false),
            ('-', false),
        ] {
            assert_eq!(is_word(c), expected, "U+{:04X}", u32::from(c));
        }
    }

    #[test]
    fn decimal_matches_python() {
        // oracle: unicodedata.decimal(c, None)
        for (c, expected) in [
            ('0', Some(0)),
            ('9', Some(9)),
            ('\u{663}', Some(3)),
            ('\u{FF11}', Some(1)),
            ('\u{B2}', None),
            ('a', None),
        ] {
            assert_eq!(decimal_value(c), expected, "U+{:04X}", u32::from(c));
        }
        // oracle: [s.isdecimal() for s in ("007", chr(0x661) + chr(0x662), "", "-2", "2x", "\xb2", "1\xb2")]
        for (s, expected) in [
            ("007", true),
            ("\u{661}\u{662}", true),
            ("", false),
            ("-2", false),
            ("2x", false),
            ("\u{B2}", false),
            ("1\u{B2}", false),
        ] {
            assert_eq!(is_decimal(s), expected, "{s:?}");
        }
    }

    #[test]
    fn nd_zeros_sorted_ten_apart() {
        assert!(ND_ZEROS.windows(2).all(|pair| {
            match pair {
                [a, b] => u32::from(*b)
                    .checked_sub(u32::from(*a))
                    .is_some_and(|gap| gap >= 10),
                _ => false,
            }
        }));
    }

    #[test]
    fn ascii_fast_paths_match_tables() {
        for c in '\0'..='\x7f' {
            assert_eq!(is_space(c), in_ranges(SPACE, c), "{c:?}");
            assert_eq!(is_printable(c), in_ranges(PRINTABLE, c), "{c:?}");
        }
    }

    fn class_regex(class: &str) -> Regex {
        Regex::new(&format!(r"\A[{class}]\z")).expect("class compiles")
    }

    #[test]
    fn space_class_matches_is_space() {
        let re = class_regex(PY_SPACE_CLASS);
        let mut buf = [0u8; 4];
        for c in every_char() {
            assert_eq!(
                re.is_match(c.encode_utf8(&mut buf)),
                is_space(c),
                "U+{:04X}",
                u32::from(c)
            );
        }
    }

    #[test]
    fn word_class_matches_is_word() {
        let re = class_regex(PY_WORD_CLASS);
        let mut buf = [0u8; 4];
        for c in every_char() {
            assert_eq!(
                re.is_match(c.encode_utf8(&mut buf)),
                is_word(c),
                "U+{:04X}",
                u32::from(c)
            );
        }
        // Rust's own `\w` counts these differently from Python's
        let rust_word = Regex::new(r"\A\w\z").expect("compiles");
        assert!(re.is_match("\u{b2}") && !rust_word.is_match("\u{b2}"));
        assert!(!re.is_match("\u{301}") && rust_word.is_match("\u{301}"));
    }

    #[test]
    fn digit_class_matches_decimal_value() {
        let re = class_regex(&digit_class());
        let mut buf = [0u8; 4];
        for c in every_char() {
            assert_eq!(
                re.is_match(c.encode_utf8(&mut buf)),
                decimal_value(c).is_some(),
                "U+{:04X}",
                u32::from(c)
            );
        }
    }

    #[test]
    fn classes_compose_inside_larger_classes() {
        let re = Regex::new(&format!(r"\A[{PY_SPACE_CLASS}x-]+\z")).expect("compiles");
        assert!(re.is_match("x- \x1c\u{a0}"));
        assert!(!re.is_match("y"));
    }
}
