use super::cases::FINGERPRINT_CASES;
use super::*;

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
