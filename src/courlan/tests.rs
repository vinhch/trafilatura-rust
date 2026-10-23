use crate::pyurl::cases::COURLAN_CASES;
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
