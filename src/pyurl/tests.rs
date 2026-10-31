#![allow(clippy::indexing_slicing)]

use super::cases::{
    HOSTNAME_CASES, NFKC_COMPOSITION_CASES, PARSE_QS_CASES, QUOTE_CASES, UNESCAPE_CASES,
    URLENCODE_CASES, URLJOIN_CASES, URLSPLIT_CASES, URLUNSPLIT_CASES,
};
use super::tables::NFKC_NETLOC_BREAKERS;
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

fn parts_of(split: &SplitResult) -> [&str; 5] {
    [
        &split.scheme,
        &split.netloc,
        &split.path,
        &split.query,
        &split.fragment,
    ]
}

#[test]
fn urlsplit_matches_python() {
    assert!(!URLSPLIT_CASES.is_empty());
    for &(url, scheme, expected, _) in URLSPLIT_CASES {
        let got = urlsplit(url, scheme);
        assert_eq!(
            got.as_ref().ok().map(parts_of),
            expected,
            "urlsplit({url:?}, {scheme:?})"
        );
    }
}

#[test]
fn urlunsplit_round_trips_python() {
    for &(url, scheme, _, expected) in URLSPLIT_CASES {
        let Ok(split) = urlsplit(url, scheme) else {
            assert_eq!(expected, None, "urlsplit({url:?}, {scheme:?}) raises");
            continue;
        };
        assert_eq!(
            Some(urlunsplit(&split)).as_deref(),
            expected,
            "urlunsplit of urlsplit({url:?}, {scheme:?})"
        );
    }
}

#[test]
fn urljoin_matches_python() {
    assert!(!URLJOIN_CASES.is_empty());
    for &(base, url, expected) in URLJOIN_CASES {
        assert_eq!(
            urljoin(base, url).ok().as_deref(),
            expected,
            "urljoin({base:?}, {url:?})"
        );
    }
}

#[test]
fn urlsplit_error_is_url_error() {
    assert_eq!(urlsplit("http://[::1", ""), Err(UrlError));
}

#[test]
fn urlunsplit_matches_python_on_given_parts() {
    assert!(!URLUNSPLIT_CASES.is_empty());
    for &([scheme, netloc, path, query, fragment], expected) in URLUNSPLIT_CASES {
        let parts = SplitResult {
            scheme: scheme.to_owned(),
            netloc: netloc.to_owned(),
            path: path.to_owned(),
            query: query.to_owned(),
            fragment: fragment.to_owned(),
        };
        assert_eq!(
            urlunsplit(&parts),
            expected,
            "urlunsplit({scheme:?}, {netloc:?}, {path:?}, {query:?}, {fragment:?})"
        );
    }
}

#[test]
fn hostname_and_port_match_python() {
    assert!(!HOSTNAME_CASES.is_empty());
    for &(url, hostname, port) in HOSTNAME_CASES {
        let split = urlsplit(url, "");
        assert!(split.is_ok(), "urlsplit({url:?}) raises");
        let Ok(split) = split else { continue };
        assert_eq!(
            split.hostname().as_deref(),
            hostname,
            "urlsplit({url:?}).hostname"
        );
        let got = match split.port() {
            Ok(None) => "none".to_owned(),
            Ok(Some(number)) => number.to_string(),
            Err(UrlError) => "err".to_owned(),
        };
        let shown: String = url.chars().take(40).collect();
        assert_eq!(got, port, "urlsplit({shown:?}..).port");
    }
}

#[test]
fn quote_matches_python() {
    assert!(!QUOTE_CASES.is_empty());
    for &(s, safe, quoted, quoted_plus) in QUOTE_CASES {
        assert_eq!(quote(s, safe), quoted, "quote({s:?}, {safe:?})");
        assert_eq!(
            quote_plus(s, safe),
            quoted_plus,
            "quote_plus({s:?}, {safe:?})"
        );
    }
}

#[test]
fn parse_qs_matches_python() {
    assert!(!PARSE_QS_CASES.is_empty());
    for &(qs, expected) in PARSE_QS_CASES {
        let expected: Vec<(String, Vec<String>)> = expected
            .iter()
            .map(|&(key, values)| {
                (
                    key.to_owned(),
                    values.iter().map(|&value| value.to_owned()).collect(),
                )
            })
            .collect();
        assert_eq!(parse_qs(qs), expected, "parse_qs({qs:?})");
    }
}

#[test]
fn urlencode_matches_python() {
    assert!(!URLENCODE_CASES.is_empty());
    for &(qs, expected) in URLENCODE_CASES {
        assert_eq!(
            urlencode_doseq(&parse_qs(qs)),
            expected,
            "urlencode(parse_qs({qs:?}), doseq=True)"
        );
    }
}
