use std::borrow::Cow;

use super::cases::{CHARACTER_CASES, HANGUL_CASES, PAIR_CASES};
use super::*;

fn shown(s: &str) -> String {
    s.chars()
        .map(|c| format!("U+{:04X} ", u32::from(c)))
        .collect()
}

#[test]
fn nfc_matches_python_table() {
    // oracle: unicodedata.normalize("NFC", s)
    for &(c, alone, wrapped) in CHARACTER_CASES {
        let input = c.to_string();
        assert_eq!(normalize_nfc(&input), alone, "alone {}", shown(&input));
        let input = format!("a{c}\u{301}");
        assert_eq!(normalize_nfc(&input), wrapped, "wrapped {}", shown(&input));
    }
    for &(first, second, composite) in PAIR_CASES {
        let input = format!("{first}{second}");
        assert_eq!(normalize_nfc(&input), composite, "pair {}", shown(&input));
    }
    for &(input, expected) in HANGUL_CASES {
        assert_eq!(normalize_nfc(input), expected, "hangul {}", shown(input));
    }
}

#[test]
fn nfc_keeps_unicode_16_pair() {
    // oracle: unicodedata.normalize("NFC", chr(0x105D2) + chr(0x307)) == chr(0x105D2) + chr(0x307)
    let todhri = "\u{105D2}\u{307}";
    assert_eq!(normalize_nfc(todhri), todhri);
}

#[test]
fn nfc_borrows_ascii_and_nfc_text() {
    assert!(matches!(normalize_nfc("plain ascii"), Cow::Borrowed(_)));
    // oracle: unicodedata.is_normalized("NFC", "\xe9t\xe9")
    assert!(matches!(normalize_nfc("\u{e9}t\u{e9}"), Cow::Borrowed(_)));
    assert!(matches!(normalize_nfc("e\u{301}"), Cow::Owned(_)));
}

#[test]
fn nfc_basic() {
    // oracle: unicodedata.normalize("NFC", "e" + chr(0x301) + " A" + chr(0x30a) + " " + chr(0x212b) + " " + chr(0xac00) + chr(0x11a8))
    assert_eq!(
        normalize_nfc("e\u{301} A\u{30a} \u{212b} \u{ac00}\u{11a8}"),
        "\u{e9} \u{c5} \u{c5} \u{ac01}"
    );
}

#[test]
fn nfc_orders_and_blocks_marks() {
    // oracle: unicodedata.normalize("NFC", s)
    for (input, expected) in [
        ("a\u{307}\u{323}", "\u{1ea1}\u{307}"),
        ("a\u{323}\u{307}", "\u{1ea1}\u{307}"),
        ("a\u{301}\u{301}", "\u{e1}\u{301}"),
        ("\u{301}a", "\u{301}a"),
        ("a\u{328}\u{301}", "\u{105}\u{301}"),
        ("o\u{302}\u{301}\u{323}", "\u{1ed9}\u{301}"),
        ("A\u{338}\u{301}", "\u{c1}\u{338}"),
        ("\u{1100}\u{1161}\u{11a8}\u{11a8}", "\u{ac01}\u{11a8}"),
        ("\u{d7a3}\u{11a8}", "\u{d7a3}\u{11a8}"),
        ("e\u{301}\u{345}", "\u{e9}\u{345}"),
    ] {
        assert_eq!(normalize_nfc(input), expected, "{}", shown(input));
    }
}
