//! Duplicate detection of upstream `deduplication.py`: the content fingerprint, with its token
//! sampling and Simhash, and the duplicate test over a caller-owned cache.

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, PoisonError};

use blake2::digest::consts::{self, U8};
use blake2::digest::typenum::Unsigned;
use blake2::{Blake2b, Digest};

use crate::cancel::Call;
use crate::context::Ctx;
use crate::dom::{NodeId, Tree};
use crate::error::ExtractError;
use crate::pystr;
use crate::utils::trim;

mod cache;

pub use cache::{DedupCache, DedupError};
use cache::{Key, key_of};

/// Bits of a simhash, which is also the length upstream's `Simhash` defaults to. Fills a `u64`, the
/// most a simhash can have.
const HASH_BITS: usize = 64;

/// Where the duplicate counts of one call go.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) enum Dedup<'a> {
    /// No cache: no text is flagged.
    #[default]
    Off,
    /// A call outside any entry point: each count writes through to the cache.
    Direct(&'a DedupCache),
    /// A call inside an entry point: the counts wait in the scope until it commits them.
    Scoped(&'a DedupScope<'a>),
}

impl Dedup<'_> {
    /// Records one occurrence of `key` and returns the count before it, `None` when there was none.
    fn record(self, key: Key) -> Option<u64> {
        match self {
            Self::Off => None,
            Self::Direct(cache) => cache.increment_key(key),
            Self::Scoped(scope) => scope.count_and_record(key),
        }
    }
}

/// The duplicate counts of one entry-point call, held until the call commits them to the cache.
#[derive(Debug)]
pub(crate) struct DedupScope<'a> {
    cache: &'a DedupCache,
    pending: Mutex<Pending>,
}

/// The increments a scope has made, in order, and how many it made to each text.
#[derive(Debug, Default)]
struct Pending {
    ops: Vec<Key>,
    added: HashMap<Key, u64>,
}

impl<'a> DedupScope<'a> {
    fn new(cache: &'a DedupCache) -> Self {
        Self {
            cache,
            pending: Mutex::new(Pending::default()),
        }
    }

    /// Upstream's `increment` as the scope sees the cache: the count the cache holds for `key` plus
    /// the increments this scope made, or `None` when neither has it. Records one increment.
    fn count_and_record(&self, key: Key) -> Option<u64> {
        let stored = self.cache.peek_key(&key);
        let mut pending = self.lock();
        let added = pending.added.get(&key).copied();
        let previous = match (stored, added) {
            (None, None) => None,
            (stored, added) => Some(stored.unwrap_or(0).saturating_add(added.unwrap_or(0))),
        };
        let count = pending.added.entry(key).or_insert(0);
        *count = count.saturating_add(1);
        pending.ops.push(key);
        previous
    }

    /// Applies the buffered increments to the cache, in order, once.
    fn commit(&self) {
        let ops = std::mem::take(&mut self.lock().ops);
        self.cache.apply(&ops);
    }

    /// Drops the buffered increments, for the rerun of a pass.
    pub(crate) fn reset(&self) {
        let mut pending = self.lock();
        pending.ops.clear();
        pending.added.clear();
    }

    fn lock(&self) -> MutexGuard<'_, Pending> {
        // deviation: D-26 (upstream's RLock never poisons; a panic elsewhere leaves the scope usable)
        self.pending.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Runs `body` with the duplicate counts of `call` buffered in a scope, and commits them to the
/// cache when the body returns `Ok` or a rejection. Any other error, and a panic, drop them. Only a
/// call that holds a cache and is not yet scoped opens a scope; every other call runs `body` on
/// `call` itself, so the outermost entry point is the one that commits.
// deviation: D-23 (counts reach the cache at commit, not per text: upstream's LRU updates in place)
pub(crate) fn run_scoped<T>(
    call: &Call<'_>,
    body: impl FnOnce(&Call<'_>) -> Result<T, ExtractError>,
) -> Result<T, ExtractError> {
    let Dedup::Direct(cache) = call.dedup_state() else {
        return body(call);
    };
    let scope = DedupScope::new(cache);
    let result = body(&call.with_dedup(Dedup::Scoped(&scope)));
    if matches!(result, Ok(_) | Err(ExtractError::Rejected(_))) {
        scope.commit();
    }
    result
}

/// Whether the text of `element` repeats too often to keep: it was seen more than `max_repetitions`
/// times before, and its trimmed text is longer than `min_duplcheck_size` characters. Every call
/// counts the text, short ones included. Always `false` on a call without a cache. Checks no
/// cancellation: its one walk is bounded by the element's subtree. Callers check: the node loops that
/// reach it through `process_node` and `handle_textnode` tick the call, and `bare_extraction_pass`
/// checks before its body-level call.
// deviation: D-22 (no global cache: upstream's LRU_TEST is a module global, the cache is on Call)
// upstream: deduplication.py::duplicate_test
pub fn duplicate_test(tree: &Tree, element: NodeId, ctx: &Ctx<'_>) -> bool {
    let dedup = ctx.call.dedup_state();
    if matches!(dedup, Dedup::Off) {
        return false;
    }
    let pieces: Vec<&str> = tree.itertext_lxml(element).collect();
    let text = trim(&pieces.join(" "));
    let previous = dedup.record(key_of(&text));
    previous.is_some_and(|previous| previous > ctx.options.max_repetitions as u64)
        && text.chars().count() > ctx.options.min_duplcheck_size
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

/// The existing hash that [`Simhash::from_parts`] and [`Simhash::validate`] take: an integer, or a
/// string in one of upstream's two encodings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(feature = "__internals"), allow(dead_code))]
pub enum HashInput<'a> {
    /// An integer, which must lie in `0..2**length` to be a hash.
    Int(i128),
    /// The decimal digits of an integer when there are 18 to 22 of them, else the hex digits that
    /// [`Simhash::to_hex`] writes. Any other string is not a hash.
    Str(&'a str),
}

/// Charikar simhash of a text: `length` bits, each the sign of a sum over the sampled tokens.
// upstream: deduplication.py::Simhash
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Simhash {
    hash: u64,
    length: usize,
}

impl Simhash {
    /// The 64-bit simhash of `input`.
    pub fn new(input: &str) -> Self {
        Self {
            hash: create_hash(input, HASH_BITS),
            length: HASH_BITS,
        }
    }

    /// The simhash of `input` over `length` bits. A valid `existing` hash is kept, and anything
    /// else, a valid zero included, is re-hashed from `input`. Fails with `SimhashLength` unless
    /// `length` is in 1..=64.
    // deviation: D-25 (upstream accepts any length, a hash longer than 64 bits does not fit a `u64`)
    // upstream: deduplication.py::Simhash.__init__
    #[cfg_attr(not(feature = "__internals"), allow(dead_code))]
    pub fn from_parts(
        input: &str,
        length: usize,
        existing: Option<HashInput<'_>>,
    ) -> Result<Self, DedupError> {
        if !(1..=HASH_BITS).contains(&length) {
            return Err(DedupError::SimhashLength);
        }
        let hash = existing
            .and_then(|existing| Self::validate(length, existing))
            .filter(|&hash| hash != 0)
            .unwrap_or_else(|| create_hash(input, length));
        Ok(Self { hash, length })
    }

    /// The hash of `length` bits that `input` names, or `None` when it names none. `length` must be in
    /// 1..=64, else the result is `None`.
    // upstream: deduplication.py::Simhash.validate
    #[cfg_attr(not(feature = "__internals"), allow(dead_code))]
    pub fn validate(length: usize, input: HashInput<'_>) -> Option<u64> {
        // deviation: D-25
        if !(1..=HASH_BITS).contains(&length) {
            return None;
        }
        let value = match input {
            HashInput::Int(value) => value,
            HashInput::Str(text) => parse_hash_str(text)?,
        };
        let hash = u64::try_from(value).ok()?;
        (length == HASH_BITS || hash >> length == 0).then_some(hash)
    }

    /// The bits of the hash as an integer.
    #[cfg_attr(not(feature = "__internals"), allow(dead_code))]
    pub fn hash(&self) -> u64 {
        self.hash
    }

    /// The number of bits of the hash.
    #[cfg_attr(not(feature = "__internals"), allow(dead_code))]
    pub fn length(&self) -> usize {
        self.length
    }

    /// The hash in lowercase hex, with no zero padding.
    // upstream: deduplication.py::Simhash.to_hex
    pub fn to_hex(self) -> String {
        format!("{:x}", self.hash)
    }

    /// The fraction of this hash's bits that `other` shares, from 0.0 to 1.0, counted over this
    /// hash's length. A shorter hash against a longer one can go negative, as upstream's does.
    // upstream: deduplication.py::Simhash.similarity
    #[cfg_attr(not(feature = "__internals"), allow(dead_code))]
    pub fn similarity(&self, other: &Simhash) -> f64 {
        let differing = (self.hash ^ other.hash).count_ones();
        let agreeing = self.length as i64 - i64::from(differing);
        agreeing as f64 / self.length as f64
    }
}

/// The integer a string names as an existing hash: its decimal digits when there are 18 to 22 of
/// them, else the value of `int(text, 16)`. `None` where `int` raises or the value leaves `i128`.
// upstream: deduplication.py::Simhash.validate
#[cfg_attr(not(feature = "__internals"), allow(dead_code))]
fn parse_hash_str(text: &str) -> Option<i128> {
    if pystr::is_decimal(text) && (18..=22).contains(&text.chars().count()) {
        return pystr::int_str(text).ok()?.parse().ok();
    }
    pystr::int_hex(text)
}

/// Charikar simhash over `length` bits of the sampled tokens of `input`. Callers keep `length` in
/// 1..=64.
// upstream: deduplication.py::Simhash.create_hash
fn create_hash(input: &str, length: usize) -> u64 {
    let mut sums = vec![0i64; length];
    for token in sample_tokens(input, length) {
        let hash_bits = token_hash(token);
        for (bit, sum) in sums.iter_mut().enumerate() {
            let contribution = if hash_bits & (1u64 << bit) != 0 {
                1
            } else {
                -1
            };
            *sum = sum.saturating_add(contribution);
        }
    }
    // a bit is set when its sum is not negative, so no token at all sets every bit
    sums.iter()
        .enumerate()
        .filter(|(_, sum)| **sum >= 0)
        .fold(0u64, |hash, (bit, _)| hash | (1u64 << bit))
}

/// The blake2b hash of a token, whose bits give the token's `+1` or `-1` contribution to each
/// simhash bit.
// upstream: deduplication.py::_vector_to_add
fn token_hash(token: &str) -> u64 {
    u64::from_be_bytes(Blake2b::<U8>::digest(token.as_bytes()).into())
}

/// The blake2b digest of the sampled tokens of `input` joined by spaces, `length` bytes long. Fails
/// with `DigestSize` unless `length` is in 1..=64.
// upstream: deduplication.py::generate_bow_hash
#[cfg_attr(not(feature = "__internals"), allow(dead_code))]
pub fn generate_bow_hash(input: &str, length: usize) -> Result<Vec<u8>, DedupError> {
    let text = sample_tokens(input, HASH_BITS).join(" ");
    let text = text.as_bytes();
    // one arm per output size: blake2 takes the size as a type
    macro_rules! digest_of_size {
        ($($size:ident),+ $(,)?) => {
            $(
                if length == <consts::$size as Unsigned>::USIZE {
                    return Ok(Blake2b::<consts::$size>::digest(text).to_vec());
                }
            )+
        };
    }
    digest_of_size!(
        U1, U2, U3, U4, U5, U6, U7, U8, U9, U10, U11, U12, U13, U14, U15, U16, U17, U18, U19, U20,
        U21, U22, U23, U24, U25, U26, U27, U28, U29, U30, U31, U32, U33, U34, U35, U36, U37, U38,
        U39, U40, U41, U42, U43, U44, U45, U46, U47, U48, U49, U50, U51, U52, U53, U54, U55, U56,
        U57, U58, U59, U60, U61, U62, U63, U64,
    );
    Err(DedupError::DigestSize)
}

#[cfg(test)]
mod cases;
#[cfg(test)]
mod tests;
