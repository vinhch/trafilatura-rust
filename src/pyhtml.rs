//! Python `html.unescape`, over the `web_atoms` entity table.

use std::borrow::Cow;

use web_atoms::{C1_REPLACEMENTS, NAMED_ENTITIES};

use crate::error::{ExtractError, Rejection};
use crate::pystr::MAX_STR_DIGITS;

/// One match of Python's `_charref` pattern, without the `&`: `#[0-9]+;?`, `#[xX][0-9a-fA-F]+;?`
/// or a run of 1 to 32 code points outside `\t \n \f space < & # ;` followed by `;?`. A number
/// keeps no `;`; a name keeps it, as the table has names with and without it.
// upstream: cpython Lib/html/__init__.py::_charref
enum Charref<'a> {
    Decimal(&'a str),
    Hex(&'a str),
    Name(&'a str),
}

/// Matches `_charref` at the start of `after` (the text following a `&`): the reference and the
/// bytes it spans, `;` included. `None` when the pattern fails there.
// upstream: cpython Lib/html/__init__.py::_charref
fn charref(after: &str) -> Option<(Charref<'_>, usize)> {
    let bytes = after.as_bytes();
    let with_semicolon = |end: usize| end + usize::from(bytes.get(end) == Some(&b';'));
    if bytes.first() == Some(&b'#') {
        let decimal = bytes
            .get(1..)?
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if decimal > 0 {
            let end = 1 + decimal;
            return Some((Charref::Decimal(after.get(1..end)?), with_semicolon(end)));
        }
        if matches!(bytes.get(1), Some(b'x' | b'X')) {
            let hex = bytes
                .get(2..)?
                .iter()
                .take_while(|b| b.is_ascii_hexdigit())
                .count();
            if hex > 0 {
                let end = 2 + hex;
                return Some((Charref::Hex(after.get(2..end)?), with_semicolon(end)));
            }
        }
        // `#` is outside the name class, so no third alternative applies.
        return None;
    }
    let mut count = 0;
    let mut end = 0;
    for (index, c) in after.char_indices() {
        if count == 32 || matches!(c, '\t' | '\n' | '\x0c' | ' ' | '<' | '&' | '#' | ';') {
            break;
        }
        count += 1;
        end = index + c.len_utf8();
    }
    if count == 0 {
        return None;
    }
    let len = with_semicolon(end);
    Some((Charref::Name(after.get(..len)?), len))
}

/// The entity `name` stands for (`;` included where the table has it), as one or two code points.
/// `web_atoms` also lists every proper prefix of a name with `(0, 0)`, which is not an entity.
fn entity(name: &str) -> Option<(u32, u32)> {
    NAMED_ENTITIES
        .get(name)
        .copied()
        .filter(|&codes| codes != (0, 0))
}

fn push_entity(out: &mut String, (first, second): (u32, u32)) {
    out.extend(
        [first, second]
            .into_iter()
            .filter(|&code| code != 0)
            .filter_map(char::from_u32),
    );
}

/// Python `_replace_charref` for a named reference: the exact name, else its longest prefix of
/// 2 or more code points that is a name (only the legacy names without `;` can be), else unchanged.
// upstream: cpython Lib/html/__init__.py::_replace_charref
fn push_named(out: &mut String, name: &str) {
    if let Some(codes) = entity(name) {
        push_entity(out, codes);
        return;
    }
    let starts: Vec<usize> = name.char_indices().map(|(index, _)| index).collect();
    for &end in starts.get(2..).unwrap_or_default().iter().rev() {
        if let Some((prefix, rest)) = name.split_at_checked(end)
            && let Some(codes) = entity(prefix)
        {
            push_entity(out, codes);
            out.push_str(rest);
            return;
        }
    }
    out.push('&');
    out.push_str(name);
}

/// Digits read saturating: a value past `0x10FFFF` decodes to U+FFFD whatever its size.
fn read_number(digits: &str, radix: u32) -> u32 {
    digits.chars().fold(0_u32, |value, c| {
        value
            .saturating_mul(radix)
            .saturating_add(c.to_digit(radix).unwrap_or(0))
    })
}

/// CPython's `_invalid_codepoints`: 126 values a reference decodes to the empty string.
// upstream: cpython Lib/html/__init__.py::_invalid_codepoints
fn is_invalid_codepoint(code: u32) -> bool {
    matches!(code, 0x1..=0x8 | 0xB | 0xE..=0x1F | 0x7F..=0x9F | 0xFDD0..=0xFDEF)
        || (code <= 0x10_FFFF && code & 0xFFFE == 0xFFFE)
}

/// Python `_replace_charref` for a numeric reference; `None` is the empty string.
// upstream: cpython Lib/html/__init__.py::_replace_charref, _invalid_charrefs
fn numeric(code: u32) -> Option<char> {
    match code {
        0 => Some('\u{FFFD}'),
        0x0D => Some('\r'),
        // `_invalid_charrefs`; a `None` entry of the table maps the code to itself
        0x80..=0x9F => usize::try_from(code - 0x80)
            .ok()
            .and_then(|index| C1_REPLACEMENTS.get(index).copied().flatten())
            .or_else(|| char::from_u32(code)),
        0xD800..=0xDFFF | 0x11_0000.. => Some('\u{FFFD}'),
        _ if is_invalid_codepoint(code) => None,
        _ => char::from_u32(code),
    }
}

/// Python `html.unescape(s)`.
///
/// Fails with `InvalidValue` on a decimal character reference of more than [`MAX_STR_DIGITS`]
/// digits, leading zeros counted: Python's `int` raises `ValueError` there. A hex reference has no
/// limit.
// upstream: html.unescape (CPython)
pub fn unescape(s: &str) -> Result<Cow<'_, str>, ExtractError> {
    if !s.contains('&') {
        return Ok(Cow::Borrowed(s));
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some((before, after)) = rest.split_once('&') {
        out.push_str(before);
        let Some((reference, len)) = charref(after) else {
            out.push('&');
            rest = after;
            continue;
        };
        match reference {
            Charref::Decimal(digits) => {
                if digits.len() > MAX_STR_DIGITS {
                    return Err(ExtractError::Rejected(Rejection::InvalidValue));
                }
                out.extend(numeric(read_number(digits, 10)));
            }
            Charref::Hex(digits) => out.extend(numeric(read_number(digits, 16))),
            Charref::Name(name) => push_named(&mut out, name),
        }
        rest = after.get(len..).unwrap_or_default();
    }
    out.push_str(rest);
    Ok(Cow::Owned(out))
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use web_atoms::NAMED_ENTITIES;

    use super::*;
    use crate::error::{ExtractError, Rejection};

    fn unescaped(s: &str) -> String {
        unescape(s).expect("valid references").into_owned()
    }

    #[test]
    fn unescape_matches_python() {
        // oracle: html.unescape(s)
        for (s, expected) in [
            (
                "&ampx &notit; &notin &amp &AMP; &#128; &#0; &#xD800; &#1; &#x110000;",
                "&x \u{AC}it; \u{AC}in & & \u{20AC} \u{FFFD} \u{FFFD}  \u{FFFD}",
            ),
            ("&#99999999999999999999;|&#4294967361;", "\u{FFFD}|\u{FFFD}"),
            ("&#13;", "\r"),
            ("&#0;", "\u{FFFD}"),
            ("&#x81;", "\u{81}"),
            ("&#x9D;", "\u{9D}"),
            ("&#1;", ""),
            ("&#xB;", ""),
            ("&#xFFFF;", ""),
            ("&#x10FFFF;", ""),
            ("&# ", "&# "),
            ("&;", "&;"),
            ("a&", "a&"),
            ("&frac12;x", "\u{BD}x"),
            ("&frac12x", "\u{BD}x"),
            ("&ltx", "<x"),
            ("&gtx;", ">x;"),
            ("&l", "&l"),
            ("&lt", "<"),
            ("&acE;", "\u{223E}\u{333}"),
            ("&NotEqualTilde;", "\u{2242}\u{338}"),
            ("&acEx", "&acEx"),
            ("&#x1F;", ""),
            ("&#x7F;", ""),
            ("&#x7f", ""),
            ("&#xfdd0;", ""),
            ("&#xFDEF;", ""),
            ("&#x1FFFE;", ""),
            ("&#xD7FF;", "\u{D7FF}"),
            ("&#xE000;", "\u{E000}"),
            ("&#65", "A"),
            ("&#65x", "Ax"),
            ("&#x41g", "Ag"),
            ("&#X41;", "A"),
            ("&#xZ", "&#xZ"),
            ("&#", "&#"),
            ("&#x", "&#x"),
            ("&&amp;", "&&"),
            ("&amp;&amp", "&&"),
            ("&#x;", "&#x;"),
            ("&#;", "&#;"),
            ("&a#b;", "&a#b;"),
            ("&amp;amp;", "&amp;"),
            ("&\rfoo;", "&\rfoo;"),
            ("&\x0bamp;", "&\x0bamp;"),
            ("&ampamp", "&amp"),
            ("&notinx", "\u{AC}inx"),
            ("&notin;", "\u{2209}"),
            ("&lt;&amp&amp&amp", "<&&&"),
            ("x&y", "x&y"),
            ("&\u{E9};", "&\u{E9};"),
            ("&nbsp;&nbsp", "\u{A0}\u{A0}"),
            ("&Aacute;&Aacutex", "\u{C1}\u{C1}x"),
            ("&#x0;", "\u{FFFD}"),
            ("&#x0D;", "\r"),
            ("&#xD;", "\r"),
        ] {
            assert_eq!(unescaped(s), expected, "{s:?}");
        }
    }

    #[test]
    fn unescape_name_run_is_capped_at_32_code_points() {
        // oracle: html.unescape("&" + "x" * 40) == "&" + "x" * 40
        let long = format!("&{}", "x".repeat(40));
        assert_eq!(unescaped(&long), long);
        // oracle: html.unescape("&" + "a" * 32 + ";") == "&" + "a" * 32 + ";"
        let thirty_two = format!("&{};", "a".repeat(32));
        assert_eq!(unescaped(&thirty_two), thirty_two);
        let thirty_one = format!("&{};", "a".repeat(31));
        assert_eq!(unescaped(&thirty_one), thirty_one);
    }

    #[test]
    fn unescape_digit_limits() {
        let invalid = Err(ExtractError::Rejected(Rejection::InvalidValue));
        // oracle: html.unescape("&#x" + "0" * 5000 + "41;")
        assert_eq!(unescaped(&format!("&#x{}41;", "0".repeat(5000))), "A");
        // oracle: html.unescape("&#" + "0" * 4298 + "65;"), and with 4299 zeros
        assert_eq!(unescaped(&format!("&#{}65;", "0".repeat(4298))), "A");
        assert_eq!(
            unescape(&format!("&#{}65;", "0".repeat(4299))).map(Cow::into_owned),
            invalid
        );
    }

    #[test]
    fn unescape_borrows_without_ampersand() {
        assert!(matches!(
            unescape("plain text"),
            Ok(Cow::Borrowed("plain text"))
        ));
        assert!(matches!(unescape("a &amp; b"), Ok(Cow::Owned(_))));
    }

    #[test]
    fn invalid_numeric_references_match_python() {
        // oracle: [html.unescape("&#%d;" % n) for n in range(0x110000)]: 94 empty results and
        // 2050 U+FFFD results (0, the 2048 surrogates and U+FFFD itself)
        let mut empty = 0;
        let mut replacement = 0;
        for code in 0..=0x10FFFF_u32 {
            match unescaped(&format!("&#{code};")).as_str() {
                "" => empty += 1,
                "\u{FFFD}" => replacement += 1,
                _ => {}
            }
        }
        assert_eq!((empty, replacement), (94, 2050));
        // oracle: [ord(html.unescape("&#%d;" % n)) for n in range(0x80, 0xa0)]
        let c1: Vec<u32> = (0x80..0xA0)
            .map(|code| {
                unescaped(&format!("&#{code};"))
                    .chars()
                    .map(u32::from)
                    .sum()
            })
            .collect();
        assert_eq!(
            c1,
            [
                8364, 129, 8218, 402, 8222, 8230, 8224, 8225, 710, 8240, 352, 8249, 338, 141, 381,
                143, 144, 8216, 8217, 8220, 8221, 8226, 8211, 8212, 732, 8482, 353, 8250, 339, 157,
                382, 376
            ]
        );
    }

    #[test]
    fn entity_table_matches_python() {
        // oracle:
        //   from html.entities import html5 as t; h=0xcbf29ce484222325
        //   for k in sorted(t):
        //       for b in (k+"\t"+" ".join("%X"%ord(c) for c in t[k])+"\n").encode(): h=((h^b)*0x100000001b3)%2**64
        //   print(len(t), hex(h))
        let mut rows: Vec<(&str, (u32, u32))> = NAMED_ENTITIES
            .entries()
            .map(|(name, codes)| (*name, *codes))
            .filter(|&(_, codes)| codes != (0, 0))
            .collect();
        rows.sort_unstable();
        let mut digest = 0xcbf2_9ce4_8422_2325_u64;
        for (name, (first, second)) in &rows {
            let codes = if *second == 0 {
                format!("{first:X}")
            } else {
                format!("{first:X} {second:X}")
            };
            for byte in format!("{name}\t{codes}\n").bytes() {
                digest = (digest ^ u64::from(byte)).wrapping_mul(0x100_0000_01b3);
            }
        }
        assert_eq!((rows.len(), digest), (2231, 0x816a_a585_04ba_cc0b));
    }
}
