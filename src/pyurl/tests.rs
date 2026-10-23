#![allow(clippy::indexing_slicing)]

use super::cases::{NFKC_COMPOSITION_CASES, UNESCAPE_CASES};
use super::tables::{HTML5_ENTITIES, NFKC_NETLOC_BREAKERS};
use super::*;

#[test]
fn unescape_matches_python() {
    for &(input, expected) in UNESCAPE_CASES {
        let shown: String = input.chars().take(40).collect();
        assert_eq!(
            unescape(input).ok().as_deref(),
            expected,
            "unescape({shown:?}..)"
        );
    }
}

#[test]
fn unescape_borrows_without_ampersand() {
    assert!(matches!(unescape("plain"), Ok(Cow::Borrowed("plain"))));
    assert!(matches!(unescape("&zzz;"), Ok(Cow::Owned(_))));
}

#[test]
fn unescape_raises_above_the_decimal_digit_limit() {
    let decimal = |digits: usize| format!("&#{}", "9".repeat(digits));
    assert!(unescape(&decimal(4300)).is_ok());
    assert_eq!(unescape(&decimal(4301)), Err(UrlError));
    assert_eq!(
        unescape(&format!("&#{}1;", "0".repeat(4300))),
        Err(UrlError)
    );
    assert!(unescape(&format!("&#x{}", "9".repeat(5000))).is_ok());
}

#[test]
fn charref_pattern_compiles() {
    assert!(CHARREF.is_some());
}

#[test]
fn entity_table_is_sorted() {
    assert!(HTML5_ENTITIES.windows(2).all(|pair| pair[0].0 < pair[1].0));
}

#[test]
fn netloc_breakers_hold_fullwidth_solidus() {
    assert!(NFKC_NETLOC_BREAKERS.contains(&'\u{FF0F}'));
}

#[test]
fn netloc_breakers_are_sorted_and_non_ascii() {
    assert!(
        NFKC_NETLOC_BREAKERS
            .windows(2)
            .all(|pair| pair[0] < pair[1])
    );
    assert!(NFKC_NETLOC_BREAKERS.iter().all(|c| !c.is_ascii()));
}

#[test]
fn netloc_breakers_survive_composition() {
    assert!(!NFKC_COMPOSITION_CASES.is_empty());
    for &(netloc, raises) in NFKC_COMPOSITION_CASES {
        let has_breaker = netloc
            .chars()
            .any(|c| NFKC_NETLOC_BREAKERS.binary_search(&c).is_ok());
        assert_eq!(has_breaker, raises, "{netloc:?}");
    }
}
