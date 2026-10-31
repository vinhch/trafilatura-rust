use super::tld::get_tld;
use super::{DOMAIN_REGEX, clean, extract_domain, is_valid_url, normalize_url, validate_url};
use crate::pyurl::cases::{
    CLEAN_QUERY_CASES, COURLAN_CASES, DECODE_PUNYCODE_CASES, EXTRACT_DOMAIN_CASES,
    NORMALIZE_URL_CASES, TLD_CASES, VALIDATE_URL_CASES,
};
use crate::pyurl::urlsplit;
use crate::utils::{safe_base_url, safe_relative_url};

#[test]
fn courlan_matches_upstream() {
    for &(baseurl, url, base_expected, relative_expected) in COURLAN_CASES {
        let shown: String = url.chars().take(40).collect();
        assert_eq!(
            safe_base_url(url),
            base_expected,
            "safe_base_url({shown:?}..)"
        );
        assert_eq!(
            safe_relative_url(baseurl, url),
            relative_expected,
            "safe_relative_url({baseurl:?}, {shown:?}..)"
        );
    }
}

fn shown(text: &str) -> String {
    text.chars().take(40).collect()
}

#[test]
fn validate_url_matches_upstream() {
    assert!(!VALIDATE_URL_CASES.is_empty());
    for &(url, expected) in VALIDATE_URL_CASES {
        let validated = validate_url(url);
        assert_eq!(
            validated.is_some(),
            expected,
            "validate_url({:?}..)",
            shown(url)
        );
        assert_eq!(
            is_valid_url(url),
            expected,
            "is_valid_url({:?}..)",
            shown(url)
        );
        if expected {
            assert_eq!(validated, urlsplit(url, "").ok(), "{:?}..", shown(url));
        }
    }
}

#[test]
fn normalize_url_matches_upstream() {
    assert!(!NORMALIZE_URL_CASES.is_empty());
    for &(url, expected) in NORMALIZE_URL_CASES {
        let Ok(parsed) = urlsplit(url, "") else {
            panic!("urlsplit({:?}..) raises", shown(url));
        };
        assert_eq!(
            normalize_url(&parsed),
            expected,
            "normalize_url(urlsplit({:?}..))",
            shown(url)
        );
    }
}

#[test]
fn clean_query_matches_upstream() {
    assert!(!CLEAN_QUERY_CASES.is_empty());
    for &(query, expected) in CLEAN_QUERY_CASES {
        assert_eq!(
            clean::clean_query(query),
            expected,
            "clean_query({:?}..)",
            shown(query)
        );
    }
}

#[test]
fn decode_punycode_matches_upstream() {
    assert!(!DECODE_PUNYCODE_CASES.is_empty());
    for &(host, expected) in DECODE_PUNYCODE_CASES {
        assert_eq!(
            clean::decode_punycode(host),
            expected,
            "decode_punycode({:?}..)",
            shown(host)
        );
    }
}

#[test]
fn tld_matches_upstream() {
    assert!(!TLD_CASES.is_empty());
    for &(url, expected) in TLD_CASES {
        let expected = expected.map(|(domain, fld)| (domain.to_owned(), fld.to_owned()));
        assert_eq!(
            get_tld(url).map(|found| (found.domain, found.fld)),
            expected,
            "get_tld({:?}..)",
            shown(url)
        );
    }
}

#[test]
fn extract_domain_matches_upstream() {
    assert!(!EXTRACT_DOMAIN_CASES.is_empty());
    for &(url, expected) in EXTRACT_DOMAIN_CASES {
        assert_eq!(
            extract_domain(url).as_deref(),
            expected,
            "extract_domain({:?}..)",
            shown(url)
        );
    }
}

#[test]
fn domain_regex_compiles() {
    assert!(DOMAIN_REGEX.is_some());
}
