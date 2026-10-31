//! CPython's `idna` codec, decode direction for one label: `ToUnicode` over the punycode codec, which
//! `courlan.clean.decode_punycode` calls as `part.encode("utf8").decode("idna")`.
//!
//! Nameprep (RFC 3491) is left out (D-16).

use crate::pystr::lower;

const ACE_PREFIX: &str = "xn--";
/// Longest label `ToUnicode` decodes, in bytes (protection from CPython issue 98433).
const MAX_DECODE_LABEL_BYTES: usize = 1024;
/// `ToASCII` refuses a label of this many bytes or more.
const MAX_ENCODED_LABEL_BYTES: usize = 64;

// Punycode parameters (RFC 3492, section 5).
const BASE: u64 = 36;
const TMIN: u64 = 1;
const TMAX: u64 = 26;
const INITIAL_BIAS: u64 = 72;
const INITIAL_N: u64 = 0x80;
const MAX_CODE_POINT: u64 = 0x10_FFFF;
/// Punycode digit values 0 to 35.
const DIGITS: &[u8; 36] = b"abcdefghijklmnopqrstuvwxyz0123456789";

/// Python `label.encode("utf8").decode("idna")` for one label (no `.`): an `xn--` label decodes through
/// punycode and must round-trip; any other ASCII label is returned as it is.
///
/// `None` where CPython raises `UnicodeError`, and where it raises `OverflowError` (a punycode position
/// of 2**63 or more; D-17).
// upstream: cpython Lib/encodings/idna.py::ToUnicode
pub fn decode_label(label: &str) -> Option<String> {
    let lowered = label.to_ascii_lowercase();
    // `Codec.decode` skips `ToUnicode`, and its length limit, for ASCII text without `xn--` anywhere
    if !lowered.contains(ACE_PREFIX) && label.is_ascii() {
        return Some(label.to_owned());
    }
    if label.len() > MAX_DECODE_LABEL_BYTES {
        return None;
    }
    if !lowered.starts_with(ACE_PREFIX) {
        return label.is_ascii().then(|| label.to_owned());
    }
    let decoded = punycode_decode(label.as_bytes().get(ACE_PREFIX.len()..)?)?;
    let encoded = to_ascii(&decoded)?;
    // `lowered` and `encoded` are ASCII here, so ASCII lowercase is `str.lower`. deviation: D-16: nameprep
    // lowercases the decoded text before the encode, so `encoded` is compared in lower case too.
    (lowered == encoded.to_ascii_lowercase()).then_some(decoded)
}

/// `ToASCII` of a decoded label, without nameprep. `None` where CPython raises `UnicodeError`.
// upstream: cpython Lib/encodings/idna.py::ToASCII
fn to_ascii(label: &str) -> Option<String> {
    if label.is_ascii() {
        return (1..MAX_ENCODED_LABEL_BYTES)
            .contains(&label.len())
            .then(|| label.to_owned());
    }
    // deviation: D-16: nameprep (and the ASCII check after it) would run here
    if lower(label).starts_with(ACE_PREFIX) {
        return None;
    }
    let encoded = format!("{ACE_PREFIX}{}", punycode_encode(label));
    (encoded.len() < MAX_ENCODED_LABEL_BYTES).then_some(encoded)
}

/// The count as `u64`; labels are far shorter than `u64::MAX`.
fn count(len: usize) -> u64 {
    u64::try_from(len).unwrap_or(u64::MAX)
}

/// Threshold of the digit at position `j` of a generalized integer.
// upstream: cpython Lib/encodings/punycode.py::T
fn threshold(j: u64, bias: u64) -> u64 {
    (BASE * (j + 1)).saturating_sub(bias).clamp(TMIN, TMAX)
}

/// The next bias after a delta of `delta` was handled (RFC 3492, section 6.1).
// upstream: cpython Lib/encodings/punycode.py::adapt
fn adapt(delta: u64, first: bool, numchars: u64) -> u64 {
    let mut delta = if first { delta / 700 } else { delta / 2 };
    delta += delta / numchars;
    let mut divisions = 0;
    // ((BASE - TMIN) * TMAX) / 2
    while delta > 455 {
        delta /= BASE - TMIN;
        divisions += BASE;
    }
    divisions + BASE * delta / (delta + 38)
}

/// Python `bytes.decode("punycode")` (strict). `None` where it raises `UnicodeDecodeError`, and where
/// a value passes `u64` or a code point is a surrogate (CPython raises either way: the code point
/// limit or the nameprep prohibition of surrogates, which Rust cannot hold).
// upstream: cpython Lib/encodings/punycode.py::punycode_decode
fn punycode_decode(encoded: &[u8]) -> Option<String> {
    let (base, extended) = match encoded.iter().rposition(|&byte| byte == b'-') {
        Some(delimiter) => {
            let (base, rest) = encoded.split_at_checked(delimiter)?;
            (base, rest.get(1..)?)
        }
        None => (&[][..], encoded),
    };
    if !base.is_ascii() {
        return None;
    }
    let mut text: Vec<char> = base.iter().map(|&byte| char::from(byte)).collect();
    let mut code = INITIAL_N;
    // CPython's `pos` plus one, so it never goes below zero
    let mut next_pos = 0_u64;
    let mut bias = INITIAL_BIAS;
    let mut start = 0;
    while start < extended.len() {
        let (end, delta) = decode_generalized_number(extended, start, bias)?;
        // deviation: D-17: upstream raises `OverflowError` for a position of 2**63 or more; every value
        // that large ends in `None` here, through the checks below and the checked operations
        let pos = next_pos.checked_add(delta)?;
        let slots = count(text.len()) + 1;
        code = code.checked_add(pos / slots)?;
        // upstream raises `UnicodeDecodeError` here (`OverflowError` from 2**63, D-17)
        if code > MAX_CODE_POINT {
            return None;
        }
        let index = usize::try_from(pos % slots).ok()?;
        text.insert(index, char::from_u32(u32::try_from(code).ok()?)?);
        next_pos = count(index) + 1;
        bias = adapt(delta, start == 0, count(text.len()));
        start = end;
    }
    Some(text.into_iter().collect())
}

/// One generalized variable-length integer of `extended` from `start`: the index after it and its
/// value. `None` for an invalid or missing digit and for a value past `u64`.
// upstream: cpython Lib/encodings/punycode.py::decode_generalized_number
fn decode_generalized_number(extended: &[u8], start: usize, bias: u64) -> Option<(usize, u64)> {
    let mut value = 0_u64;
    let mut weight = 1_u64;
    for (j, (index, &byte)) in (0..).zip(extended.iter().enumerate().skip(start)) {
        let digit = match byte.to_ascii_uppercase() {
            letter @ b'A'..=b'Z' => u64::from(letter - b'A'),
            number @ b'0'..=b'9' => u64::from(number - b'0') + 26,
            _ => return None,
        };
        let digit_threshold = threshold(j, bias);
        // deviation: D-17
        value = value.checked_add(digit.checked_mul(weight)?)?;
        if digit < digit_threshold {
            return Some((index + 1, value));
        }
        // deviation: D-17
        weight = weight.checked_mul(BASE - digit_threshold)?;
    }
    None
}

/// Python `text.encode("punycode")`: the basic characters, `-` when there are any, then the deltas of
/// the others. A label is at most 1024 bytes here, so no sum passes `u64`.
// upstream: cpython Lib/encodings/punycode.py::punycode_encode
fn punycode_encode(text: &str) -> String {
    let code_points: Vec<u64> = text.chars().map(|c| u64::from(u32::from(c))).collect();
    let mut output: String = text.chars().filter(char::is_ascii).collect();
    let basic_count = count(output.len());
    if basic_count > 0 {
        output.push('-');
    }
    let total = count(code_points.len());
    let mut handled = basic_count;
    let mut code = INITIAL_N;
    let mut delta = 0_u64;
    let mut bias = INITIAL_BIAS;
    while handled < total {
        let Some(next) = code_points.iter().copied().filter(|&c| c >= code).min() else {
            break;
        };
        delta += (next - code) * (handled + 1);
        code = next;
        for &c in &code_points {
            if c < code {
                delta += 1;
            } else if c == code {
                push_generalized_number(&mut output, delta, bias);
                bias = adapt(delta, handled == basic_count, handled + 1);
                delta = 0;
                handled += 1;
            }
        }
        delta += 1;
        code += 1;
    }
    output
}

/// Appends `number` as a generalized variable-length integer.
// upstream: cpython Lib/encodings/punycode.py::generate_generalized_integer
fn push_generalized_number(output: &mut String, mut number: u64, bias: u64) {
    let mut j = 0;
    loop {
        let digit_threshold = threshold(j, bias);
        if number < digit_threshold {
            output.push(digit_char(number));
            return;
        }
        let range = BASE - digit_threshold;
        output.push(digit_char(
            digit_threshold + (number - digit_threshold) % range,
        ));
        number = (number - digit_threshold) / range;
        j += 1;
    }
}

/// The punycode character of a digit value below [`BASE`].
fn digit_char(digit: u64) -> char {
    usize::try_from(digit)
        .ok()
        .and_then(|index| DIGITS.get(index))
        .map_or('-', |&byte| char::from(byte))
}

#[cfg(test)]
mod tests;
