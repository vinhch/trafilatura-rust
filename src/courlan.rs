//! The URL helpers upstream takes from the courlan package: `urlutils.py` here, `filters.py` in
//! `filters` and `clean.py` in `clean`.

mod clean;
mod filters;
mod tld;
mod tld_tables;

// Re-exported for the metadata port; unused outside the tests until it lands.
#[allow(unused_imports)]
pub use clean::normalize_url;
#[allow(unused_imports)]
pub use filters::{is_valid_url, validate_url};

use std::sync::LazyLock;

use regex::Regex;

use crate::pystr::{decimal_value, digit_class};
use crate::pyurl::{UrlError, unescape, urljoin, urlsplit, urlunsplit};

// `$` is `\n?\z`: Python's also matches before a final newline. `\d` is Python's (`digit_class`). The
// regex is a literal that compiles (`domain_regex_compiles`); `None` would send every URL to the tld
// lookup instead of panicking.
// upstream: courlan/urlutils.py::DOMAIN_REGEX
static DOMAIN_REGEX: LazyLock<Option<Regex>> = LazyLock::new(|| {
    let d = digit_class();
    Regex::new(&format!(
        r"\A(?:(?:f|ht)tp)s?://(?:[^/?#]{{0,63}}\.)?([^/?#.]{{4,63}}\.[^/?#]{{2,63}}|[{d}]{{1,3}}\.[{d}]{{1,3}}\.[{d}]{{1,3}}\.[{d}]{{1,3}}|[0-9a-f:]{{16,}})(?:/|\n?\z)"
    ))
    .ok()
});

/// `text` without each `:` and the digits after it that follow a character other than a digit.
///
/// The test looks at the character before the colon in `text`, not in the output, as Python's
/// lookbehind does: in `a:1:2` the second colon follows the digit `1` and stays.
// upstream: courlan/urlutils.py::STRIP_PORT_REGEX (`(?<=\D):\d+` substituted by the empty string)
fn strip_port(text: &str) -> String {
    let mut stripped = String::with_capacity(text.len());
    let mut previous: Option<char> = None;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == ':'
            && previous.is_some_and(|before| decimal_value(before).is_none())
            && chars
                .peek()
                .is_some_and(|&next| decimal_value(next).is_some())
        {
            while let Some(digit) = chars.next_if(|&next| decimal_value(next).is_some()) {
                previous = Some(digit);
            }
            continue;
        }
        stripped.push(c);
        previous = Some(c);
    }
    stripped
}

/// `fld` without a leading `www`, digits and dot, such as `www2.`; one prefix at most.
// upstream: courlan/urlutils.py::CLEAN_FLD_REGEX (`^www[0-9]*\.` substituted by the empty string)
fn strip_www_prefix(fld: &str) -> &str {
    fld.strip_prefix("www")
        .map(|rest| rest.trim_start_matches(|c: char| c.is_ascii_digit()))
        .and_then(|rest| rest.strip_prefix('.'))
        .unwrap_or(fld)
}

/// The domain label and the full domain of `url`: from the regex when it matches, else from the suffix
/// list (`www` and digits dropped from the full domain); `None` for an empty URL or no match of either.
// upstream: courlan/urlutils.py::get_tldinfo (fast=True)
pub fn get_tldinfo(url: &str) -> Option<(String, String)> {
    if url.is_empty() {
        return None;
    }
    let matched = DOMAIN_REGEX
        .as_ref()
        .and_then(|regex| regex.captures(url))
        .and_then(|captures| captures.get(1));
    if let Some(domain) = matched {
        let host = domain.as_str();
        let full_domain = strip_port(host.rsplit('@').next().unwrap_or(host));
        let clean_match = full_domain.split('.').next().unwrap_or_default();
        if !clean_match.is_empty() {
            return Some((clean_match.to_owned(), full_domain));
        }
    }
    let found = tld::get_tld(url)?;
    Some((found.domain, strip_www_prefix(&found.fld).to_owned()))
}

/// The full domain of `url`, such as `example.co.uk`; `None` when there is none.
// upstream: courlan/urlutils.py::extract_domain (blacklist=None, fast=True)
pub fn extract_domain(url: &str) -> Option<String> {
    let (_, full_domain) = get_tldinfo(url)?;
    Some(full_domain).filter(|domain| !domain.is_empty())
}

/// Strips a URL to its scheme and host, such as `https://example.org`.
///
/// Fails where CPython raises `ValueError`: an overlong decimal charref or a rejected netloc.
// upstream: courlan/urlutils.py::get_base_url
pub fn get_base_url(url: &str) -> Result<String, UrlError> {
    // upstream: courlan/urlutils.py::_parse
    let parsed = urlsplit(&unescape(url)?, "")?;
    if parsed.scheme.is_empty() {
        return Ok(parsed.netloc);
    }
    Ok(format!("{}://{}", parsed.scheme, parsed.netloc))
}

/// Resolves a link against `baseurl`; a link to another host is kept, given `http` when it has no
/// scheme. Fails where CPython raises `ValueError`.
// upstream: courlan/urlutils.py::fix_relative_urls
pub fn fix_relative_urls(baseurl: &str, url: &str) -> Result<String, UrlError> {
    if url.starts_with('{') {
        return Ok(url.to_owned());
    }

    let base_netloc = urlsplit(baseurl, "")?.netloc;
    let mut split_url = urlsplit(url, "")?;

    if split_url.netloc != base_netloc && !split_url.netloc.is_empty() {
        if !split_url.scheme.is_empty() {
            return Ok(url.to_owned());
        }
        "http".clone_into(&mut split_url.scheme);
        return Ok(urlunsplit(&split_url));
    }

    urljoin(baseurl, url)
}

#[cfg(test)]
mod tests;
