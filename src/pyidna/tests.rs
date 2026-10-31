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

// deviation: D-17: upstream raises OverflowError for the first, third and fourth labels; the port
// keeps them encoded. `1w..` is 2**64 + 256 as a punycode number, which wraps to a valid position,
// so the round-trip check is what rejects it if the checked arithmetic ever wraps. The second
// label is incomplete and the fifth is 2**62 (both UnicodeError upstream); U+10FFFF decodes.
#[test]
fn idna_overflowing_position_is_none() {
    let huge = format!("xn--{}a", "9".repeat(40));
    assert_eq!(decode_label(&huge), None);
    assert_eq!(decode_label(&format!("xn--{}", "9".repeat(40))), None);
    assert_eq!(decode_label("xn--1w124498107776961m"), None);
    assert_eq!(decode_label("xn--t6767205604493046e"), None);
    assert_eq!(decode_label("xn--e4049608857257578a"), None);
    assert_eq!(decode_label("xn--dn32g").as_deref(), Some("\u{10FFFF}"));
}
