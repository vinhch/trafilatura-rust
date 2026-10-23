//! The URL helpers upstream takes from the courlan package (`courlan/urlutils.py`).

use crate::pyurl::{UrlError, unescape, urljoin, urlsplit, urlunsplit};

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
