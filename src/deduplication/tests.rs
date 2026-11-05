use super::cache::key_of;
use super::cases::FINGERPRINT_CASES;
use super::*;
use crate::settings::LRU_SIZE;

#[test]
fn fingerprint_matches_python() {
    for &(input, tokens, fingerprint) in FINGERPRINT_CASES {
        assert_eq!(sample_tokens(input, HASH_BITS), tokens, "{input:?}");
        assert_eq!(content_fingerprint(input), fingerprint, "{input:?}");
    }
}

#[test]
fn content_without_tokens_sets_every_bit() {
    // every bit's sum is zero, which is not negative, so every bit is set
    assert_eq!(content_fingerprint(""), "ffffffffffffffff");
    assert_eq!(content_fingerprint(" \t\n"), "ffffffffffffffff");
    assert_eq!(content_fingerprint("!!! 。、！"), "ffffffffffffffff");
}

#[test]
fn cache_is_send_and_sync() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<DedupCache>();
}

#[test]
fn put_past_maxsize_keeps_maxsize_entries() {
    // maxsize 1 evicts on every new key; 3 evicts once the cache is full
    for maxsize in [1, 3] {
        let cache = DedupCache::new(maxsize).expect("maxsize is at least 1");
        for i in 0..10 {
            cache.put(&i.to_string(), i);
        }
        assert_eq!(cache.len(), maxsize);
        assert_eq!(cache.get("9"), Some(9), "newest entry survives");
        assert_eq!(cache.get("0"), None, "oldest entry is evicted");
    }
}

#[test]
fn apply_matches_repeated_increment() {
    let batched = DedupCache::new(8).expect("maxsize 8 is valid");
    let stepwise = DedupCache::new(8).expect("maxsize 8 is valid");
    let (k, j) = (key_of("k"), key_of("j"));

    batched.apply(&[k, k, j]);
    stepwise.increment("k");
    stepwise.increment("k");
    stepwise.increment("j");

    assert_eq!(batched.peek_key(&k), Some(2));
    assert_eq!(batched.peek_key(&j), Some(1));
    assert_eq!(batched.len(), stepwise.len());
    assert_eq!(batched.peek_key(&k), stepwise.peek_key(&k));
    assert_eq!(batched.peek_key(&j), stepwise.peek_key(&j));
}

#[test]
fn apply_on_no_keys_leaves_cache_unchanged() {
    let cache = DedupCache::new(8).expect("maxsize 8 is valid");
    cache.apply(&[]);
    assert!(cache.is_empty());
}

#[test]
fn increment_saturates_at_max_count() {
    let cache = DedupCache::new(8).expect("maxsize 8 is valid");
    cache.put("a", u64::MAX);
    assert_eq!(cache.increment("a"), Some(u64::MAX));
    assert_eq!(cache.get("a"), Some(u64::MAX));
}

#[test]
fn peek_key_does_not_refresh_recency() {
    let cache = DedupCache::new(2).expect("maxsize 2 is valid");
    cache.put("a", 1);
    cache.put("b", 2);
    assert_eq!(cache.peek_key(&key_of("a")), Some(1));
    // "a" is still the oldest, so the next new key evicts it
    cache.put("c", 3);
    assert_eq!(cache.get("a"), None);
}

#[test]
fn default_cache_uses_lru_size_and_is_empty() {
    let cache = DedupCache::default();
    assert_eq!(cache.maxsize(), LRU_SIZE);
    assert!(cache.is_empty());
}
