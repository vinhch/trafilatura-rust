#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Port of upstream `tests/deduplication_tests.py`. Each test keeps its upstream name.
//!
//! Ported: `test_hashes`, `test_sample_tokens`, `test_lrucache_api`, `test_simhash`,
//! `test_simhash_validate_pins`, `test_generate_bow_hash`, `test_lrucache`, `test_dedup`,
//! `test_duplicate_test_boundaries`, `test_dedup_cache_option`.
//!
//! Not ported: `test_dedup_reset_caches` resets the process-global `LRU_TEST`, which has no
//! counterpart: the caller owns the cache (AGENTS.md, no process-global state).
//! `generate_hash_filename` is in `cli_utils.py`, outside the port.

#[allow(dead_code)]
mod trees;

use std::collections::HashSet;

use trafilatura_rust::__internals::{
    Config, Extractor, Format, HashInput, NodeId, Simhash, Tree, content_fingerprint,
    duplicate_test, generate_bow_hash, process_node, sample_tokens,
};
use trafilatura_rust::{Call, DedupCache, DedupError, ExtractError, Input, Options, Rejection};
use trees::{ctx_for, lxml_fromstring};

#[test]
fn test_hashes() {
    let content = "abcde ijk l, ".repeat(10);
    assert_eq!(content_fingerprint(&content), "528497a1d07b66d6");
    assert_eq!(
        content_fingerprint("Hello world! This is a test string with some numbers 123."),
        "5efdce9f2b554683"
    );
    assert_eq!(
        content_fingerprint("这是一个测试。我们在测试中文。"),
        "ff377edee6edfb78"
    );
    assert_eq!(
        content_fingerprint("Hello世界。This is混合文本!"),
        "24979dc6c8a26a5"
    );
}

#[test]
fn test_sample_tokens() {
    let tokens = sample_tokens(
        "Hello world! This is a test string with some numbers 123.",
        64,
    );
    assert!(tokens.contains(&"Hello"));
    assert!(tokens.contains(&"world"));
    assert!(tokens.contains(&"123"));
    // Chinese punctuation: fallback path
    assert_eq!(
        sample_tokens("这是一个测试。我们在测试中文。", 64),
        ["这是一个测试", "我们在测试中文"]
    );
    // mixed text: primary path yields one token, no fallback
    assert_eq!(
        sample_tokens("Hello世界。This is混合文本!", 64),
        ["is混合文本"]
    );
    assert!(sample_tokens("", 64).is_empty());
    assert!(sample_tokens("!!! ... ???", 64).is_empty());
    assert!(sample_tokens("。、！", 64).is_empty());
    // many short tokens force the length threshold down
    assert_eq!(sample_tokens(&"ab ".repeat(40), 64), vec!["ab"; 40]);
    // enough long tokens keep the strict threshold: short ones are dropped
    let input = (0..40)
        .map(|i| format!("token{i}"))
        .collect::<Vec<_>>()
        .join(" ")
        + " ab cd";
    let tokens = sample_tokens(&input, 64);
    assert!(!tokens.contains(&"ab"));
    assert_eq!(tokens.len(), 40);
}

#[test]
fn test_lrucache_api() {
    assert_eq!(DedupCache::new(0).err(), Some(DedupError::CacheSize));

    let original = DedupCache::new(8).expect("maxsize 8 is valid");
    original.increment("a");
    let cache = original.clone();
    assert_eq!(cache.maxsize(), 8);
    assert_eq!(cache.get("a"), Some(1)); // contents survive
    assert_eq!(cache.increment("a"), Some(1));
    assert_eq!(original.get("a"), Some(1)); // the clone is a deep copy

    let lru = DedupCache::new(2).expect("maxsize 2 is valid");
    assert_eq!(lru.get("a"), None);
    lru.put("a", 1);
    assert_eq!(lru.get("a"), Some(1));
    lru.put("b", 2);
    // touching "a" makes "b" the eviction candidate
    assert_eq!(lru.get("a"), Some(1));
    lru.put("c", 3);
    assert_eq!(lru.get("b"), None);
    assert_eq!(lru.get("a"), Some(1));
    assert_eq!(lru.get("c"), Some(3));
    // put on an existing key updates value and recency
    lru.put("a", 10);
    lru.put("d", 4);
    assert_eq!(lru.get("c"), None);
    assert_eq!(lru.get("a"), Some(10));
    lru.clear();
    assert!(lru.is_empty());

    // increment matches the get + put composition and returns the previous count
    let lru = DedupCache::new(2).expect("maxsize 2 is valid");
    assert_eq!(lru.increment("x"), None);
    assert_eq!(lru.increment("x"), Some(1));
    assert_eq!(lru.get("x"), Some(2));
    lru.increment("y");
    lru.increment("z");
    assert_eq!(lru.get("x"), None); // evicted as oldest
}

/// The hash `existing` gives for the empty text: a valid `existing` is kept, anything else is re-hashed.
fn hash_of_empty(existing: HashInput<'_>) -> u64 {
    Simhash::from_parts("", 64, Some(existing))
        .expect("64 is in range")
        .hash()
}

#[test]
fn test_simhash() {
    // https://en.wiktionary.org/wiki/put_lipstick_on_a_pig
    let hashes = [
        Simhash::new("This is like putting lipstick on a pig."),
        Simhash::new("This is just like putting lipstick on a pig."),
        Simhash::new("Putting lipstick on a pig is what this is about."),
        Simhash::new("The words are completely different but let's see."),
    ];
    let sims: Vec<f64> = hashes
        .iter()
        .map(|other| hashes[0].similarity(other))
        .collect();
    assert_eq!(sims[0], 1.0);
    let minimum = sims.iter().copied().fold(f64::INFINITY, f64::min);
    assert_eq!(minimum, sims[3]);

    // sanity checks; upstream's `int(hex(hash), 0) == hash` is a Python-only check
    let hex = hashes[0].to_hex();
    let round_trip =
        Simhash::from_parts("", 64, Some(HashInput::Str(&hex))).expect("64 is in range");
    assert_eq!(round_trip.hash(), hashes[0].hash());

    // re-hashed: the empty text sets every bit
    assert_eq!(hash_of_empty(HashInput::Str("aghj")), u64::MAX);
    assert_eq!(
        hash_of_empty(HashInput::Str("18446744073709551615")),
        u64::MAX
    );
    assert_eq!(
        hash_of_empty(HashInput::Str("18446744073709551616")),
        u64::MAX
    );
    assert_eq!(hash_of_empty(HashInput::Int(123)), 123);
    assert_eq!(
        hash_of_empty(HashInput::Int(18446744073709551615)),
        u64::MAX
    );
    assert_eq!(hash_of_empty(HashInput::Int(1 << 64)), u64::MAX);
    assert_eq!(hash_of_empty(HashInput::Int(-1)), u64::MAX);
    assert_eq!(
        Simhash::from_parts("", 64, None)
            .expect("64 is in range")
            .hash(),
        u64::MAX
    );

    // similarity
    let abcde = Simhash::new("abcde");
    assert_eq!(abcde.similarity(&Simhash::new("abcde")), 1.0);
    let short = Simhash::from_parts("abcde", 2, None).expect("2 is in range");
    assert_ne!(abcde.similarity(&short), 1.0);
    // a shorter self counts the bits it has, so the value goes negative (oracle: upstream -12.5 and 0.578125)
    assert_eq!(short.similarity(&abcde), -12.5);
    assert_eq!(abcde.similarity(&short), 0.578125);
    assert!(abcde.similarity(&Simhash::new("fghij")) < 0.6);
    assert_eq!(Simhash::new(&"abcde ".repeat(100)).similarity(&abcde), 1.0);
}

/// The hash of `text` with `existing` as the existing hash.
fn hash_of(text: &str, existing: HashInput<'_>) -> u64 {
    Simhash::from_parts(text, 64, Some(existing))
        .expect("64 is in range")
        .hash()
}

#[test]
fn test_simhash_validate_pins() {
    let reference = Simhash::new("abcde").hash();
    // negative integers are rejected and trigger re-hashing
    assert_eq!(
        hash_of("abcde", HashInput::Int(-1_000_000_000_000_000_000)),
        reference
    );
    // upstream's booleans-are-rejected line has no counterpart: HashInput has no bool variant
    // existing_hash="0" is deliberately re-hashed (kept for backward compatibility)
    assert_eq!(hash_of_empty(HashInput::Str("0")), u64::MAX);
    // non-decimal Unicode digits fall through to re-hashing instead of crashing int()
    assert_eq!(
        hash_of("abcde", HashInput::Str(&"\u{b9}".repeat(18))),
        reference
    );
    // hex strings outside the 64-bit hash range are rejected and re-hashed
    assert_eq!(hash_of("abcde", HashInput::Str("-ff")), reference);
    assert_eq!(hash_of("abcde", HashInput::Str(&"f".repeat(17))), reference);
    // small hex values as produced by to_hex() still round-trip
    assert_eq!(hash_of_empty(HashInput::Str("ff")), 255);
}

#[test]
fn test_generate_bow_hash() {
    // digest size parameter and degenerate inputs
    assert_eq!(
        generate_bow_hash("Hello world this is a test", 12)
            .expect("12 is in range")
            .len(),
        12
    );
    assert_eq!(generate_bow_hash("", 24).expect("24 is in range").len(), 24);
    assert_eq!(generate_bow_hash("", 24), generate_bow_hash("   ", 24));
    assert_eq!(generate_bow_hash("", 0), Err(DedupError::DigestSize));
}

/// The options `duplicate_test` reads: upstream's `Extractor()` defaults.
fn default_options() -> Extractor {
    Extractor::new(
        Config::default(),
        Format::Txt,
        false,
        false,
        None,
        false,
        false,
        HashSet::new(),
        HashSet::new(),
    )
}

/// Upstream's `_make_pair`: a paragraph and the body that holds it, in one tree.
fn make_pair(text: &str) -> (Tree, NodeId, NodeId) {
    let (tree, paragraph) = lxml_fromstring(&format!("<p>{text}</p>"));
    let body = tree
        .node(paragraph)
        .parent()
        .expect("a paragraph from lxml_fromstring sits in a body");
    (tree, body, paragraph)
}

#[test]
fn test_lrucache() {
    let cache = DedupCache::new(2).expect("maxsize 2 is valid");
    let call = Call::default().dedup(&cache);
    let options = default_options();
    let ctx = ctx_for(&options, &call);

    let (my_tree, my_body, my_element) = make_pair(&"AAAA BBBB ".repeat(13));
    let seq = [my_element, my_element, my_body, my_element];
    let flags: Vec<bool> = seq
        .iter()
        .map(|&node| duplicate_test(&my_tree, node, &ctx))
        .collect();
    assert_eq!(flags, [false, false, false, true]);

    let (other_tree, other_body, other_element) = make_pair(&"CCCC DDDD ".repeat(11));
    let seq = [other_body, other_element, other_body, other_element];
    let flags: Vec<bool> = seq
        .iter()
        .map(|&node| duplicate_test(&other_tree, node, &ctx))
        .collect();
    assert_eq!(flags, [false, false, false, true]);

    let (yet_tree, yet_body, yet_element) = make_pair(&"EEEE FFFF ".repeat(13));
    let flags: Vec<bool> = (0..3)
        .map(|_| duplicate_test(&yet_tree, yet_body, &ctx))
        .collect();
    assert_eq!(flags, [false, false, false]);
    // 2 elements in cache, original element has been cleared?
    assert!(duplicate_test(&other_tree, other_element, &ctx));
    assert!(duplicate_test(&yet_tree, yet_element, &ctx));
    assert!(!duplicate_test(&my_tree, my_element, &ctx));
    // clear the cache
    cache.clear();
    assert!(!duplicate_test(&other_tree, other_element, &ctx));
}

#[test]
fn test_duplicate_test_boundaries() {
    let cache = DedupCache::new(1024).expect("maxsize 1024 is valid");
    let call = Call::default().dedup(&cache);
    let options = default_options();
    let ctx = ctx_for(&options, &call);
    let limit = Config::default().min_duplcheck_size;

    // the length bound is strict: at the limit nothing is flagged
    let (at_tree, at_limit) = lxml_fromstring(&format!("<p>{}</p>", "x".repeat(limit)));
    let flags: Vec<bool> = (0..6)
        .map(|_| duplicate_test(&at_tree, at_limit, &ctx))
        .collect();
    assert_eq!(flags, [false; 6]);

    // one past the limit: flagged once the previous count exceeds max_repetitions (2)
    let (above_tree, above_limit) = lxml_fromstring(&format!("<p>{}</p>", "x".repeat(limit + 1)));
    let flags: Vec<bool> = (0..6)
        .map(|_| duplicate_test(&above_tree, above_limit, &ctx))
        .collect();
    assert_eq!(flags, [false, false, false, true, true, true]);
}

#[test]
fn test_dedup() {
    let page = format!("<html><body>{}</body></html>", "<p>abc</p>".repeat(50));
    let input = Input::text(&page);
    let extractor = Options::new().build();

    let cache = DedupCache::new(2).expect("maxsize 2 is valid");
    let call = Call::default().dedup(&cache);
    let results: Vec<Result<String, ExtractError>> =
        (0..4).map(|_| extractor.extract(&input, &call)).collect();
    assert!(results[..3].iter().all(Result::is_ok));
    assert_eq!(
        results[3],
        Err(ExtractError::Rejected(Rejection::Duplicate))
    );

    // paragraph level
    let cache = DedupCache::new(2).expect("maxsize 2 is valid");
    let call = Call::default().dedup(&cache);
    let options = default_options();
    let ctx = ctx_for(&options, &call);
    let (mut tree, paragraph) = lxml_fromstring(&format!("<p>{}</p>", "abc".repeat(50)));
    let flags: Vec<bool> = (0..4)
        .map(|_| process_node(&mut tree, paragraph, &ctx).is_some())
        .collect();
    assert_eq!(flags, [true, true, true, false]);
}

#[test]
fn test_dedup_cache_option() {
    let page = format!("<html><body>{}</body></html>", "<p>abc</p>".repeat(50));
    let input = Input::text(&page);
    let extractor = Options::new().build();

    // a fresh cache per call: deterministic, repeated extractions all succeed
    let fresh: Vec<Result<String, ExtractError>> = (0..6)
        .map(|_| {
            let cache = DedupCache::new(1024).expect("maxsize 1024 is valid");
            extractor.extract(&input, &Call::default().dedup(&cache))
        })
        .collect();
    assert!(fresh.iter().all(Result::is_ok));
    assert!(fresh.windows(2).all(|pair| pair[0] == pair[1]));

    // one shared cache: accumulation triggers deduplication
    let shared = DedupCache::new(1024).expect("maxsize 1024 is valid");
    let call = Call::default().dedup(&shared);
    let results: Vec<Result<String, ExtractError>> =
        (0..6).map(|_| extractor.extract(&input, &call)).collect();
    assert!(results[0].is_ok());
    assert_eq!(
        results[5],
        Err(ExtractError::Rejected(Rejection::Duplicate))
    );

    // a cache no call received stays untouched
    let untouched = DedupCache::new(1024).expect("maxsize 1024 is valid");
    assert!(untouched.is_empty());
    assert!(
        extractor
            .extract(&input, &Call::default().dedup(&untouched))
            .is_ok()
    );
}

#[test]
fn empty_cache_enables_dedup() {
    let cache = DedupCache::new(2).expect("maxsize 2 is valid");
    assert!(cache.is_empty());
    let call = Call::default().dedup(&cache);
    let options = default_options();
    let ctx = ctx_for(&options, &call);
    let (tree, paragraph) = lxml_fromstring(&format!("<p>{}</p>", "abc ".repeat(40)));
    let flags: Vec<bool> = (0..4)
        .map(|_| duplicate_test(&tree, paragraph, &ctx))
        .collect();
    assert_eq!(flags, [false, false, false, true]);
}

#[test]
fn whitespace_only_element_is_counted_and_never_flagged() {
    let cache = DedupCache::new(2).expect("maxsize 2 is valid");
    let call = Call::default().dedup(&cache);
    let options = default_options();
    let ctx = ctx_for(&options, &call);
    let (tree, paragraph) = lxml_fromstring("<p>   </p>");
    let flags: Vec<bool> = (0..6)
        .map(|_| duplicate_test(&tree, paragraph, &ctx))
        .collect();
    assert_eq!(flags, [false; 6]);
    // the trimmed text is empty, so the count sits under the empty key
    assert_eq!(cache.get(""), Some(6));
}
