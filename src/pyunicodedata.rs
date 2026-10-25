//! Python `unicodedata.normalize("NFC", ...)` over the Unicode data of the interpreter.
//!
//! The tables (`nfc.rs`) are generated from Python's `unicodedata` by `tools/parity/parity/gen_nfc.py`,
//! not taken from a crate: Python 3.13 carries Unicode 15.1, and a character assigned later stays
//! unchanged in Python's NFC but composes with newer data.

mod nfc;

use std::borrow::Cow;

use crate::pystr::in_ranges;
use nfc::{COMBINING_CLASSES, COMPOSITIONS, DECOMPOSITIONS, NFC_MAYBE, NFC_NO};

// Hangul syllables are composed and decomposed by arithmetic (Unicode standard, section 3.12).
const S_BASE: u32 = 0xAC00;
const L_BASE: u32 = 0x1100;
const V_BASE: u32 = 0x1161;
const T_BASE: u32 = 0x11A7;
const L_COUNT: u32 = 19;
const V_COUNT: u32 = 21;
const T_COUNT: u32 = 28;
const N_COUNT: u32 = V_COUNT * T_COUNT;
const S_COUNT: u32 = L_COUNT * N_COUNT;

/// Canonical combining class of `c`; 0 for a starter.
fn combining_class(c: char) -> u8 {
    let c = u32::from(c);
    let after = COMBINING_CLASSES.partition_point(|&(_, end, _)| end < c);
    COMBINING_CLASSES
        .get(after)
        .filter(|&&(start, _, _)| start <= c)
        .map_or(0, |&(_, _, class)| class)
}

/// Whether `c` cannot make a string differ from its NFC form: below U+0300 nothing changes, above it
/// `c` must be a starter that NFC keeps and no pair can end on.
fn is_stable(c: char) -> bool {
    c < '\u{300}' || (!in_ranges(NFC_NO, c) && !in_ranges(NFC_MAYBE, c) && combining_class(c) == 0)
}

/// The full canonical decomposition of `c` appended to `out`.
fn decompose_into(c: char, out: &mut Vec<char>) {
    let code = u32::from(c);
    if let Some(index) = code.checked_sub(S_BASE).filter(|&index| index < S_COUNT) {
        out.extend(char::from_u32(L_BASE + index / N_COUNT));
        out.extend(char::from_u32(V_BASE + index % N_COUNT / T_COUNT));
        if index % T_COUNT != 0 {
            out.extend(char::from_u32(T_BASE + index % T_COUNT));
        }
        return;
    }
    match DECOMPOSITIONS.binary_search_by_key(&c, |&(code_point, _)| code_point) {
        Ok(found) => {
            if let Some((_, decomposed)) = DECOMPOSITIONS.get(found) {
                out.extend(decomposed.chars());
            }
        }
        Err(_) => out.push(c),
    }
}

/// The character that `first` followed by `second` composes to, if any.
fn compose_pair(first: char, second: char) -> Option<char> {
    let (a, b) = (u32::from(first), u32::from(second));
    if (L_BASE..L_BASE + L_COUNT).contains(&a) && (V_BASE..V_BASE + V_COUNT).contains(&b) {
        return char::from_u32(S_BASE + (a - L_BASE) * N_COUNT + (b - V_BASE) * T_COUNT);
    }
    let is_lv_syllable = a
        .checked_sub(S_BASE)
        .is_some_and(|index| index < S_COUNT && index % T_COUNT == 0);
    if is_lv_syllable && (T_BASE + 1..T_BASE + T_COUNT).contains(&b) {
        return char::from_u32(a + (b - T_BASE));
    }
    let found = COMPOSITIONS
        .binary_search_by(|&(f, s, _)| (f, s).cmp(&(first, second)))
        .ok()?;
    COMPOSITIONS.get(found).map(|&(_, _, composite)| composite)
}

/// Canonical ordering: each run of non-starters sorted by combining class, equal classes in place.
fn reorder(chars: &mut [char]) {
    for run in chars.chunk_by_mut(|a, b| combining_class(*a) != 0 && combining_class(*b) != 0) {
        if run.len() > 1 {
            run.sort_by_cached_key(|&c| combining_class(c));
        }
    }
}

/// Canonical composition of canonically ordered, fully decomposed `chars`.
fn compose(chars: &[char]) -> String {
    let mut out: Vec<char> = Vec::with_capacity(chars.len());
    // Index in `out` of the last starter, the only character a later one can compose into.
    let mut starter: Option<usize> = None;
    for &c in chars {
        let class = combining_class(c);
        if let Some(index) = starter {
            // Blocked when a character between the starter and `c` has class 0 or a class not below
            // `c`'s; what lies between is sorted, so only the last one matters.
            let unblocked = index + 1 == out.len()
                || (class != 0
                    && out
                        .last()
                        .is_some_and(|&last| combining_class(last) < class));
            if unblocked
                && let Some(slot) = out.get_mut(index)
                && let Some(composite) = compose_pair(*slot, c)
            {
                *slot = composite;
                continue;
            }
        }
        if class == 0 {
            starter = Some(out.len());
        }
        out.push(c);
    }
    out.into_iter().collect()
}

/// Python `unicodedata.normalize("NFC", s)` with Unicode 15.1 data; borrowed when `s` is already
/// normalized by the quick check (every character below U+0300, or a stable starter), which covers
/// ASCII and most real text without allocating.
// upstream: unicodedata.normalize (CPython)
pub fn normalize_nfc(s: &str) -> Cow<'_, str> {
    if s.chars().all(is_stable) {
        return Cow::Borrowed(s);
    }
    let mut chars = Vec::with_capacity(s.len());
    for c in s.chars() {
        decompose_into(c, &mut chars);
    }
    reorder(&mut chars);
    Cow::Owned(compose(&chars))
}

#[cfg(test)]
mod cases;
#[cfg(test)]
mod tests;
