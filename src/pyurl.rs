//! CPython's `html.unescape` (and the `urllib.parse` functions the URL helpers call), ported with
//! generated tables (`tools/parity/parity/gen_url.py`).

mod tables;

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::{Captures, Regex};

use tables::HTML5_ENTITIES;

// The pattern is a literal that compiles (`charref_pattern_compiles` forces it); `None` would make
// `unescape` return its input instead of panicking.
// upstream: cpython Lib/html/__init__.py::_charref
static CHARREF: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"&(#[0-9]+;?|#[xX][0-9a-fA-F]+;?|[^\t\n\f <&#;]{1,32};?)").ok());

// oracle: sorted(html._invalid_charrefs.items())
// upstream: cpython Lib/html/__init__.py::_invalid_charrefs
const INVALID_CHARREFS: &[(u32, char)] = &[
    (0x0, '\u{FFFD}'),
    (0xD, '\u{D}'),
    (0x80, '\u{20AC}'),
    (0x81, '\u{81}'),
    (0x82, '\u{201A}'),
    (0x83, '\u{192}'),
    (0x84, '\u{201E}'),
    (0x85, '\u{2026}'),
    (0x86, '\u{2020}'),
    (0x87, '\u{2021}'),
    (0x88, '\u{2C6}'),
    (0x89, '\u{2030}'),
    (0x8A, '\u{160}'),
    (0x8B, '\u{2039}'),
    (0x8C, '\u{152}'),
    (0x8D, '\u{8D}'),
    (0x8E, '\u{17D}'),
    (0x8F, '\u{8F}'),
    (0x90, '\u{90}'),
    (0x91, '\u{2018}'),
    (0x92, '\u{2019}'),
    (0x93, '\u{201C}'),
    (0x94, '\u{201D}'),
    (0x95, '\u{2022}'),
    (0x96, '\u{2013}'),
    (0x97, '\u{2014}'),
    (0x98, '\u{2DC}'),
    (0x99, '\u{2122}'),
    (0x9A, '\u{161}'),
    (0x9B, '\u{203A}'),
    (0x9C, '\u{153}'),
    (0x9D, '\u{9D}'),
    (0x9E, '\u{17E}'),
    (0x9F, '\u{178}'),
];

// oracle: sorted(html._invalid_codepoints)
// upstream: cpython Lib/html/__init__.py::_invalid_codepoints
#[rustfmt::skip]
const INVALID_CODEPOINTS: &[u32] = &[
    0x1, 0x2, 0x3, 0x4, 0x5, 0x6, 0x7, 0x8, 0xB, 0xE, 0xF, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15,
    0x16, 0x17, 0x18, 0x19, 0x1A, 0x1B, 0x1C, 0x1D, 0x1E, 0x1F, 0x7F, 0x80, 0x81, 0x82, 0x83,
    0x84, 0x85, 0x86, 0x87, 0x88, 0x89, 0x8A, 0x8B, 0x8C, 0x8D, 0x8E, 0x8F, 0x90, 0x91, 0x92,
    0x93, 0x94, 0x95, 0x96, 0x97, 0x98, 0x99, 0x9A, 0x9B, 0x9C, 0x9D, 0x9E, 0x9F, 0xFDD0,
    0xFDD1, 0xFDD2, 0xFDD3, 0xFDD4, 0xFDD5, 0xFDD6, 0xFDD7, 0xFDD8, 0xFDD9, 0xFDDA, 0xFDDB,
    0xFDDC, 0xFDDD, 0xFDDE, 0xFDDF, 0xFDE0, 0xFDE1, 0xFDE2, 0xFDE3, 0xFDE4, 0xFDE5, 0xFDE6,
    0xFDE7, 0xFDE8, 0xFDE9, 0xFDEA, 0xFDEB, 0xFDEC, 0xFDED, 0xFDEE, 0xFDEF, 0xFFFE, 0xFFFF,
    0x1FFFE, 0x1FFFF, 0x2FFFE, 0x2FFFF, 0x3FFFE, 0x3FFFF, 0x4FFFE, 0x4FFFF, 0x5FFFE, 0x5FFFF,
    0x6FFFE, 0x6FFFF, 0x7FFFE, 0x7FFFF, 0x8FFFE, 0x8FFFF, 0x9FFFE, 0x9FFFF, 0xAFFFE, 0xAFFFF,
    0xBFFFE, 0xBFFFF, 0xCFFFE, 0xCFFFF, 0xDFFFE, 0xDFFFF, 0xEFFFE, 0xEFFFF, 0xFFFFE, 0xFFFFF,
    0x10FFFE, 0x10FFFF,
];

// upstream: cpython Lib/html/__init__.py::_replace_charref
fn numeric_charref(digits: &str, radix: u32) -> String {
    // A value that does not fit `u32` is above U+10FFFF, as Python's unbounded `int` would be.
    let num = u32::from_str_radix(digits, radix).unwrap_or(u32::MAX);
    if let Some(&(_, replacement)) = INVALID_CHARREFS.iter().find(|&&(key, _)| key == num) {
        return replacement.to_string();
    }
    // Surrogates and values above U+10FFFF are not a `char`: U+FFFD, as upstream.
    let Some(c) = char::from_u32(num) else {
        return '\u{FFFD}'.to_string();
    };
    if INVALID_CODEPOINTS.binary_search(&num).is_ok() {
        return String::new();
    }
    c.to_string()
}

fn entity(name: &str) -> Option<&'static str> {
    HTML5_ENTITIES
        .binary_search_by(|&(key, _)| key.as_bytes().cmp(name.as_bytes()))
        .ok()
        .and_then(|i| HTML5_ENTITIES.get(i))
        .map(|&(_, value)| value)
}

/// `name` is the reference without its `&`, `;` included when present: the exact entity, else the
/// longest proper prefix of at least 2 characters that is one, followed by the rest, else `&name`.
// upstream: cpython Lib/html/__init__.py::_replace_charref
fn named_charref(name: &str) -> String {
    if let Some(value) = entity(name) {
        return value.to_owned();
    }
    let offsets: Vec<usize> = name.char_indices().map(|(offset, _)| offset).collect();
    for prefix_chars in (2..offsets.len()).rev() {
        let Some(&split) = offsets.get(prefix_chars) else {
            continue;
        };
        if let (Some(prefix), Some(rest)) = (name.get(..split), name.get(split..))
            && let Some(value) = entity(prefix)
        {
            return format!("{value}{rest}");
        }
    }
    format!("&{name}")
}

// upstream: cpython Lib/html/__init__.py::_replace_charref
fn replace_charref(caps: &Captures<'_>) -> String {
    let reference = caps.get(1).map_or("", |m| m.as_str());
    let Some(body) = reference.strip_prefix('#') else {
        return named_charref(reference);
    };
    let without_semicolon = |text: &str| text.strip_suffix(';').unwrap_or(text).to_owned();
    match body.strip_prefix(['x', 'X']) {
        Some(hex) => numeric_charref(&without_semicolon(hex), 16),
        None => numeric_charref(&without_semicolon(body), 10),
    }
}

/// Python `html.unescape`: replaces named and numeric character references with the characters they
/// stand for, by the HTML 5 rules (a name without `;` matches by its longest known prefix).
// upstream: cpython Lib/html/__init__.py::unescape
pub fn unescape(s: &str) -> Cow<'_, str> {
    if !s.contains('&') {
        return Cow::Borrowed(s);
    }
    match CHARREF.as_ref() {
        Some(charref) => charref.replace_all(s, replace_charref),
        None => Cow::Borrowed(s),
    }
}

#[cfg(test)]
pub(crate) mod cases;
#[cfg(test)]
mod tests;
