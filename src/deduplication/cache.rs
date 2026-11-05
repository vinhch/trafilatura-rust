//! The duplicate cache: a least-recently-used map from a text's digest to how often it was seen.

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::sync::{Mutex, MutexGuard, PoisonError};

use blake2::digest::consts::U16;
use blake2::{Blake2b, Digest};

use crate::settings::LRU_SIZE;

/// The digest of a text, which the cache stores in place of the text.
pub(crate) type Key = [u8; 16];

/// Why a deduplication value was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DedupError {
    /// The cache size is below 1.
    CacheSize,
    /// The simhash length is outside 1..=64, the bits of the `u64` hash.
    SimhashLength,
    /// The digest size is outside 1..=64 bytes.
    DigestSize,
}

impl fmt::Display for DedupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::CacheSize => "cache size must be at least 1",
            Self::SimhashLength => "simhash length is out of range",
            Self::DigestSize => "digest size is out of range",
        })
    }
}

impl std::error::Error for DedupError {}

/// A least-recently-used cache from a text to how often it was seen, for duplicate detection.
///
/// Upstream's `LRUCache`. The cache keeps a digest of each text, not the text. Each method holds
/// the lock for its own duration. A lock poisoned by a panic on another thread is recovered, so no
/// method panics and the cache keeps the entries it had. `Clone` copies the contents: the clone and
/// the original count separately from then on.
// upstream: deduplication.py::LRUCache
#[derive(Debug)]
pub struct DedupCache {
    inner: Mutex<Lru>,
}

impl DedupCache {
    /// Creates an empty cache that keeps at most `maxsize` texts, evicting the least recently used.
    /// Fails with `DedupError::CacheSize` when `maxsize` is 0.
    pub fn new(maxsize: usize) -> Result<Self, DedupError> {
        if maxsize < 1 {
            return Err(DedupError::CacheSize);
        }
        Ok(Self::with_maxsize(maxsize))
    }

    /// The count stored for `text`, or `None` when it is absent. A hit makes `text` the most
    /// recently used.
    pub fn get(&self, text: &str) -> Option<u64> {
        self.lock().get(&key_of(text))
    }

    /// Stores `count` for `text`, which becomes the most recently used. Past `maxsize` entries the
    /// least recently used one is evicted.
    pub fn put(&self, text: &str, count: u64) {
        let key = key_of(text);
        self.lock().put(key, count);
    }

    /// Adds one to the count of `text`, which becomes the most recently used. Returns the count
    /// before the add, or `None` when `text` was absent (its count is then 1).
    pub fn increment(&self, text: &str) -> Option<u64> {
        self.increment_key(key_of(text))
    }

    /// Removes every entry. `maxsize` is unchanged.
    pub fn clear(&self) {
        self.lock().clear();
    }

    /// The number of entries held.
    pub fn len(&self) -> usize {
        self.lock().entries.len()
    }

    /// Whether the cache holds no entries.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// The most entries the cache keeps.
    pub fn maxsize(&self) -> usize {
        self.lock().maxsize
    }

    /// The count stored for `key`, without making it the most recently used.
    pub(crate) fn peek_key(&self, key: &Key) -> Option<u64> {
        self.lock().entries.get(key).map(|&(count, _)| count)
    }

    /// `increment` for a digest of the text, as `key_of` gives it.
    pub(crate) fn increment_key(&self, key: Key) -> Option<u64> {
        self.lock().increment(key)
    }

    /// Replays `increment` for each key in `ops`, in order, under one lock hold.
    pub(crate) fn apply(&self, ops: &[Key]) {
        let mut lru = self.lock();
        for key in ops {
            lru.increment(*key);
        }
    }

    fn with_maxsize(maxsize: usize) -> Self {
        Self {
            inner: Mutex::new(Lru::new(maxsize)),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Lru> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Default for DedupCache {
    fn default() -> Self {
        Self::with_maxsize(LRU_SIZE)
    }
}

impl Clone for DedupCache {
    fn clone(&self) -> Self {
        Self {
            inner: Mutex::new(self.lock().clone()),
        }
    }
}

/// The digest of `text` as given. Callers pass the text already trimmed, as upstream does.
pub(crate) fn key_of(text: &str) -> Key {
    Blake2b::<U16>::digest(text.as_bytes()).into()
}

/// The entries of a `DedupCache`, held under its lock.
#[derive(Clone, Debug)]
struct Lru {
    maxsize: usize,
    /// Key to its count and the tick of its last use.
    entries: HashMap<Key, (u64, u64)>,
    /// Tick to key, so the first entry is the least recently used. Holds one entry per key in
    /// `entries`.
    order: BTreeMap<u64, Key>,
    next_tick: u64,
}

impl Lru {
    fn new(maxsize: usize) -> Self {
        Self {
            maxsize,
            entries: HashMap::new(),
            order: BTreeMap::new(),
            next_tick: 0,
        }
    }

    fn get(&mut self, key: &Key) -> Option<u64> {
        let &(count, _) = self.entries.get(key)?;
        self.touch(key);
        Some(count)
    }

    fn put(&mut self, key: Key, count: u64) {
        let tick = self.tick();
        if let Some((_, last)) = self.entries.insert(key, (count, tick)) {
            self.order.remove(&last);
        }
        self.order.insert(tick, key);
        while self.entries.len() > self.maxsize {
            let Some((_, oldest)) = self.order.pop_first() else {
                break;
            };
            self.entries.remove(&oldest);
        }
    }

    fn increment(&mut self, key: Key) -> Option<u64> {
        let previous = self.entries.get(&key).map(|&(count, _)| count);
        self.put(key, previous.unwrap_or(0).saturating_add(1));
        previous
    }

    fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    /// Makes `key`, which is present, the most recently used.
    fn touch(&mut self, key: &Key) {
        let tick = self.tick();
        if let Some((_, last)) = self.entries.get_mut(key) {
            self.order.remove(&*last);
            *last = tick;
            self.order.insert(tick, *key);
        }
    }

    fn tick(&mut self) -> u64 {
        let tick = self.next_tick;
        // shortcut: ticks saturate after 2^64 uses, which one process does not reach; widen to u128 if that changes.
        self.next_tick = self.next_tick.saturating_add(1);
        tick
    }
}
