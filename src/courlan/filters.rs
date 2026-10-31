//! `courlan/filters.py`: the URL validity check.

use crate::pyurl::{SplitResult, urlsplit};

// upstream: courlan/filters.py::PROTOCOLS
const PROTOCOLS: [&str; 2] = ["http", "https"];

/// The parts of `url` when it parses, names an `http` or `https` scheme and has a netloc of at least
/// 5 characters (8 after a `www.` prefix); `None` where upstream returns `(False, None)`.
// upstream: courlan/filters.py::validate_url
pub fn validate_url(url: &str) -> Option<SplitResult> {
    let parsed = urlsplit(url, "").ok()?;
    if !PROTOCOLS.contains(&parsed.scheme.as_str()) {
        return None;
    }
    // code points, as Python's `len`
    let netloc_len = parsed.netloc.chars().count();
    if netloc_len < 5 || (parsed.netloc.starts_with("www.") && netloc_len < 8) {
        return None;
    }
    Some(parsed)
}

/// Whether [`validate_url`] accepts `url`.
// upstream: courlan/filters.py::is_valid_url
pub fn is_valid_url(url: &str) -> bool {
    validate_url(url).is_some()
}
