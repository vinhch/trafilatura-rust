use super::*;
use crate::pyurl::cases::{IDNA_CASES, IDNA_NAMEPREP_CASES};

fn shown(label: &str) -> String {
    label.chars().take(40).collect()
}

#[test]
fn idna_decode_matches_python() {
    assert!(!IDNA_CASES.is_empty());
    for &(label, expected) in IDNA_CASES {
        assert_eq!(
            decode_label(label).as_deref(),
            expected,
            "decode_label({:?}..)",
            shown(label)
        );
    }
}

// deviation: D-16: the port runs no nameprep, so these labels, which `decode("idna")` rejects, decode
// to their punycode text.
#[test]
fn idna_skips_nameprep() {
    assert!(!IDNA_NAMEPREP_CASES.is_empty());
    for &(label, upstream, ported) in IDNA_NAMEPREP_CASES {
        assert_eq!(upstream, None, "upstream decodes {label:?}");
        assert_eq!(
            decode_label(label).as_deref(),
            Some(ported),
            "decode_label({label:?})"
        );
    }
}
