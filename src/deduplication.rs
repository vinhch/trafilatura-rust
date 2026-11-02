//! Duplicate detection of upstream `deduplication.py`: the content fingerprint, with its token
//! sampling and Simhash. `duplicate_test` is a stub until the duplicate cache is ported.

use blake2::digest::consts::U8;
use blake2::{Blake2b, Digest};

use crate::context::Ctx;
use crate::dom::{NodeId, Tree};
use crate::pystr;

/// Bits of a simhash, which is also the length upstream's `Simhash` defaults to. Fills a `u64`.
const HASH_BITS: usize = 64;

/// Whether the text of `element` repeats too often to keep. Always `false` until phase 5 ports the
/// duplicate cache.
// upstream: deduplication.py::duplicate_test
pub fn duplicate_test(_tree: &Tree, _element: NodeId, _ctx: &Ctx<'_>) -> bool {
    // todo(phase 5): deduplication
    false
}

/// The fingerprint of `content` that metadata carries: the hex simhash of its sampled tokens, with
/// no zero padding.
// upstream: deduplication.py::content_fingerprint
pub fn content_fingerprint(content: &str) -> String {
    Simhash::new(content).to_hex()
}

/// The tokens of `input` that a simhash is taken over: alphanumeric words, punctuation stripped
/// from their ends. Falls back to splitting on punctuation for text without spaces, such as
/// Chinese. Keeps the tokens longer than the largest of 4, 3, 2 and 1 characters whose survivors
/// reach half of `length`, and every token when none does.
// upstream: deduplication.py::sample_tokens
pub fn sample_tokens(input: &str, length: usize) -> Vec<&str> {
    let mut tokens: Vec<&str> = pystr::split_whitespace(input)
        .map(|token| token.trim_matches(|c: char| c.is_ascii_punctuation()))
        .filter(|token| is_alnum_token(token))
        .collect();
    if tokens.is_empty() {
        // non-latin punctuation, e.g. mandarin 。
        tokens = input
            .split(|c: char| pystr::is_space(c) || pystr::is_punctuation(c))
            .filter(|token| is_alnum_token(token))
            .collect();
    }

    // the strictest minimum length first, so short tokens are dropped whenever enough longer ones remain
    for min_chars in (1..=4).rev() {
        let sample: Vec<&str> = tokens
            .iter()
            .copied()
            .filter(|token| token.chars().count() > min_chars)
            .collect();
        if sample.len().saturating_mul(2) >= length {
            return sample;
        }
    }
    tokens
}

/// Python `str.isalnum` on a whole token: false for the empty string.
fn is_alnum_token(token: &str) -> bool {
    !token.is_empty() && token.chars().all(pystr::is_alnum)
}

/// Charikar simhash over the sampled tokens of a text, `HASH_BITS` wide.
// upstream: deduplication.py::Simhash
struct Simhash {
    hash: u64,
}

impl Simhash {
    // upstream: deduplication.py::Simhash.create_hash
    fn new(input: &str) -> Self {
        let mut sums = [0i64; HASH_BITS];
        for token in sample_tokens(input, HASH_BITS) {
            for (sum, contribution) in sums.iter_mut().zip(vector_to_add(token)) {
                *sum = sum.saturating_add(contribution);
            }
        }
        // a bit is set when its sum is not negative, so no token at all sets every bit
        let hash = sums
            .iter()
            .enumerate()
            .filter(|(_, sum)| **sum >= 0)
            .fold(0u64, |hash, (bit, _)| hash | (1u64 << bit));
        Self { hash }
    }

    // upstream: deduplication.py::Simhash.to_hex
    fn to_hex(&self) -> String {
        format!("{:x}", self.hash)
    }
}

/// The contribution of `token` to each bit of a simhash: `+1` where its hash has the bit set, `-1`
/// where it does not.
// upstream: deduplication.py::_vector_to_add
fn vector_to_add(token: &str) -> [i64; HASH_BITS] {
    let token_hash = u64::from_be_bytes(Blake2b::<U8>::digest(token.as_bytes()).into());
    std::array::from_fn(|bit| {
        if token_hash & (1u64 << bit) != 0 {
            1
        } else {
            -1
        }
    })
}

#[cfg(test)]
mod cases;
#[cfg(test)]
mod tests;
