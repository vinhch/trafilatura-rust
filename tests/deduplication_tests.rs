//! Port of upstream `tests/deduplication_tests.py` (trafilatura 2.3.1). Each test keeps its upstream name.
//! Not ported: `test_simhash`, `test_simhash_validate_pins` and `test_generate_bow_hash`. They test
//! `Simhash.similarity`, `Simhash.validate` and `generate_bow_hash`, which wait for phase 5 (the
//! last only has a caller in `cli_utils.py`, outside the port), and `generate_hash_filename` is
//! `cli_utils.py` as well.

use trafilatura_rust::__internals::{content_fingerprint, sample_tokens};

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
