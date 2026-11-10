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

/// Code points of the longest entity name without `;`, the only names a proper prefix of a
/// reference can match.
const LEGACY_NAME_MAX_CHARS: usize = 6;

/// The entity `name` stands for (`;` included where the table has it), as one or two code points.
/// `web_atoms` also lists every proper prefix of a name with `(0, 0)`, which is not an entity.
// upstream: cpython Lib/html/__init__.py::_replace_charref
fn entity(name: &str) -> Option<(u32, u32)> {
    NAMED_ENTITIES
        .get(name)
        .copied()
        .filter(|&codes| codes != (0, 0))
}

// upstream: cpython Lib/html/__init__.py::_replace_charref
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
    // Same result as trying every proper prefix: only a name without `;` can be a proper prefix
    // of the reference, and none of those is longer than `LEGACY_NAME_MAX_CHARS`.
    let longest = LEGACY_NAME_MAX_CHARS.min(name.chars().count().saturating_sub(1));
    for chars in (2..=longest).rev() {
        if let Some((end, _)) = name.char_indices().nth(chars)
            && let Some((prefix, rest)) = name.split_at_checked(end)
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
// upstream: cpython Lib/html/__init__.py::_replace_charref
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
/// Fails with `InvalidValue` on a decimal character reference of more than `MAX_STR_DIGITS`
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
mod tests;
