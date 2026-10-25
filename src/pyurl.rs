//! CPython's `html.unescape` (over `pyhtml`) and `urllib.parse.urlsplit`, `urlunsplit` and `urljoin`
//! (the functions the URL helpers call), ported with generated tables
//! (`tools/parity/parity/gen_url.py`).

mod tables;

use std::borrow::Cow;

use tables::NFKC_NETLOC_BREAKERS;

/// The `ValueError` of the `urllib.parse` and `html` helpers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UrlError;

/// Python `html.unescape`: replaces named and numeric character references with the characters they
/// stand for, by the HTML 5 rules (a name without `;` matches by its longest known prefix).
///
/// Errors exactly where CPython raises: a decimal reference of more than 4300 digits.
// upstream: cpython Lib/html/__init__.py::unescape
pub fn unescape(s: &str) -> Result<Cow<'_, str>, UrlError> {
    // the only error `pyhtml::unescape` has is the `ValueError` of `int`
    crate::pyhtml::unescape(s).map_err(|_| UrlError)
}

/// The five components of `urllib.parse.SplitResult`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitResult {
    /// Scheme, lowercased; the default scheme when the URL names none.
    pub scheme: String,
    /// Text after `//` up to the first `/`, `?` or `#`; empty without `//`.
    pub netloc: String,
    /// Path, without query and fragment.
    pub path: String,
    /// Query, without the `?`.
    pub query: String,
    /// Fragment, without the `#`.
    pub fragment: String,
}

// oracle: urllib.parse.uses_relative
// upstream: cpython Lib/urllib/parse.py::uses_relative
const USES_RELATIVE: &[&str] = &[
    "", "ftp", "http", "gopher", "nntp", "imap", "wais", "file", "https", "shttp", "mms",
    "prospero", "rtsp", "rtsps", "rtspu", "sftp", "svn", "svn+ssh", "ws", "wss",
];

// oracle: urllib.parse.uses_netloc
// upstream: cpython Lib/urllib/parse.py::uses_netloc
const USES_NETLOC: &[&str] = &[
    "",
    "ftp",
    "http",
    "gopher",
    "nntp",
    "telnet",
    "imap",
    "wais",
    "file",
    "mms",
    "https",
    "shttp",
    "snews",
    "prospero",
    "rtsp",
    "rtsps",
    "rtspu",
    "rsync",
    "svn",
    "svn+ssh",
    "sftp",
    "nfs",
    "git",
    "git+ssh",
    "ws",
    "wss",
    "itms-services",
];

// oracle: urllib.parse.uses_params
// upstream: cpython Lib/urllib/parse.py::uses_params
const USES_PARAMS: &[&str] = &[
    "", "ftp", "hdl", "prospero", "http", "imap", "https", "shttp", "rtsp", "rtsps", "rtspu",
    "sip", "sips", "mms", "sftp", "tel",
];

// Python's `scheme_chars`: ASCII letters, digits, `+`, `-`, `.`.
// upstream: cpython Lib/urllib/parse.py::scheme_chars
fn is_scheme_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')
}

// upstream: cpython Lib/urllib/parse.py::_WHATWG_C0_CONTROL_OR_SPACE
fn is_c0_control_or_space(c: char) -> bool {
    c <= ' '
}

// upstream: cpython Lib/urllib/parse.py::_UNSAFE_URL_BYTES_TO_REMOVE
const UNSAFE_URL_CHARS: [char; 3] = ['\t', '\r', '\n'];

// `rest` is the text after the leading `//`, so Python's `start` argument is gone.
// upstream: cpython Lib/urllib/parse.py::_splitnetloc
fn split_netloc(rest: &str) -> (&str, &str) {
    let delim = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    rest.split_at_checked(delim).unwrap_or((rest, ""))
}

// `NFKC_NETLOC_BREAKERS` holds the non-ASCII characters whose NFKC form has one of `/?#@:`; CPython
// strips `@:#?` from the netloc before normalizing, and a netloc has no `/`.
// upstream: cpython Lib/urllib/parse.py::_checknetloc
fn check_netloc(netloc: &str) -> Result<(), UrlError> {
    if netloc.is_ascii()
        || !netloc
            .chars()
            .any(|c| NFKC_NETLOC_BREAKERS.binary_search(&c).is_ok())
    {
        return Ok(());
    }
    Err(UrlError)
}

// upstream: cpython Lib/urllib/parse.py::_check_bracketed_netloc
fn check_bracketed_netloc(netloc: &str) -> Result<(), UrlError> {
    let hostname_and_port = netloc.rsplit_once('@').map_or(netloc, |(_, host)| host);
    let hostname = match hostname_and_port.split_once('[') {
        Some((before_bracket, bracketed)) => {
            if !before_bracket.is_empty() {
                return Err(UrlError);
            }
            let (hostname, port) = bracketed.split_once(']').unwrap_or((bracketed, ""));
            if !port.is_empty() && !port.starts_with(':') {
                return Err(UrlError);
            }
            hostname
        }
        None => hostname_and_port
            .split_once(':')
            .map_or(hostname_and_port, |(hostname, _)| hostname),
    };
    check_bracketed_host(hostname)
}

// `ipaddress.ip_address` succeeds on an IPv4 address too, and CPython then raises as well: only a
// valid IPv6 address passes.
// upstream: cpython Lib/urllib/parse.py::_check_bracketed_host
fn check_bracketed_host(hostname: &str) -> Result<(), UrlError> {
    if let Some(after_v) = hostname.strip_prefix(['v', 'V']) {
        // re.match(r"\A[vV][a-fA-F0-9]+\..+\Z", hostname)
        let hex_len = after_v.bytes().take_while(u8::is_ascii_hexdigit).count();
        let rest = after_v
            .get(hex_len..)
            .and_then(|rest| rest.strip_prefix('.'));
        return match rest {
            Some(rest) if hex_len > 0 && !rest.is_empty() && !rest.contains('\n') => Ok(()),
            _ => Err(UrlError),
        };
    }
    if is_python_ipv6(hostname) {
        Ok(())
    } else {
        Err(UrlError)
    }
}

// upstream: cpython Lib/ipaddress.py::_BaseV4._parse_octet
fn is_python_ipv4_octet(octet: &str) -> bool {
    !octet.is_empty()
        && octet.bytes().all(|b| b.is_ascii_digit())
        && octet.len() <= 3
        && (octet == "0" || !octet.starts_with('0'))
        && octet.parse::<u16>().is_ok_and(|value| value <= 255)
}

// upstream: cpython Lib/ipaddress.py::_BaseV4._ip_int_from_string
fn is_python_ipv4(address: &str) -> bool {
    let octets: Vec<&str> = address.split('.').collect();
    octets.len() == 4 && octets.iter().all(|octet| is_python_ipv4_octet(octet))
}

// upstream: cpython Lib/ipaddress.py::_BaseV6._parse_hextet
fn is_python_hextet(hextet: &str) -> bool {
    !hextet.is_empty() && hextet.len() <= 4 && hextet.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Whether `ipaddress.IPv6Address(address)` accepts `address`, scope id included.
// upstream: cpython Lib/ipaddress.py::IPv6Address.__init__
fn is_python_ipv6(address: &str) -> bool {
    if address.contains('/') {
        return false;
    }
    // upstream: cpython Lib/ipaddress.py::_BaseV6._split_scope_id
    let (address, scope_id) = match address.split_once('%') {
        Some((address, scope_id)) => (address, Some(scope_id)),
        None => (address, None),
    };
    if scope_id.is_some_and(|scope_id| scope_id.is_empty() || scope_id.contains('%')) {
        return false;
    }
    is_python_ipv6_digits(address)
}

// upstream: cpython Lib/ipaddress.py::_BaseV6._ip_int_from_string
fn is_python_ipv6_digits(address: &str) -> bool {
    const HEXTET_COUNT: usize = 8;
    const MAX_PARTS: usize = HEXTET_COUNT + 1;
    if address.is_empty() || address.chars().count() > 45 {
        return false;
    }
    // At most MAX_PARTS + 1 parts, the last keeping any further colons, as `split(':', MAX_PARTS)`.
    let mut parts: Vec<&str> = address.splitn(MAX_PARTS + 1, ':').collect();
    if parts.len() < 3 {
        return false;
    }
    if parts.last().is_some_and(|last| last.contains('.')) {
        if !parts.pop().is_some_and(is_python_ipv4) {
            return false;
        }
        // Two hextets stand for the IPv4 address; only their count matters here.
        parts.extend(["0", "0"]);
    }
    if parts.len() > MAX_PARTS {
        return false;
    }
    let Some(last) = parts.len().checked_sub(1) else {
        return false;
    };
    // Disregarding the endpoints, the one empty part is the `::`.
    let mut skip_index = None;
    for (i, part) in parts.iter().enumerate().take(last).skip(1) {
        if part.is_empty() {
            if skip_index.is_some() {
                return false;
            }
            skip_index = Some(i);
        }
    }
    let first_empty = parts.first().is_some_and(|part| part.is_empty());
    let last_empty = parts.last().is_some_and(|part| part.is_empty());
    let (parts_hi, parts_lo) = match skip_index {
        Some(skip) => {
            // Parts before and after the `::`; an empty endpoint belongs to the `::` itself.
            // `skip` is an inner index, so `skip >= 1` and `last - skip >= 1`.
            let mut hi = skip;
            let Some(mut lo) = last.checked_sub(skip) else {
                return false;
            };
            if first_empty {
                hi = hi.saturating_sub(1);
                if hi != 0 {
                    return false;
                }
            }
            if last_empty {
                lo = lo.saturating_sub(1);
                if lo != 0 {
                    return false;
                }
            }
            if HEXTET_COUNT.saturating_sub(hi + lo) < 1 {
                return false;
            }
            (hi, lo)
        }
        None => {
            if parts.len() != HEXTET_COUNT || first_empty || last_empty {
                return false;
            }
            (parts.len(), 0)
        }
    };
    let (Some(head), Some(tail)) = (
        parts.get(..parts_hi),
        parts
            .len()
            .checked_sub(parts_lo)
            .and_then(|tail_start| parts.get(tail_start..)),
    ) else {
        return false;
    };
    head.iter()
        .chain(tail)
        .all(|hextet| is_python_hextet(hextet))
}

/// Python `urllib.parse.urlsplit(url, scheme)` with `allow_fragments=True`: the five components of
/// `url`, `scheme` standing in when the URL names none. `%` escapes are not expanded.
///
/// Errors exactly where CPython raises `ValueError`: unbalanced or invalid bracketed hosts and a
/// netloc that holds a delimiter after NFKC normalization.
// upstream: cpython Lib/urllib/parse.py::urlsplit
pub fn urlsplit(url: &str, scheme: &str) -> Result<SplitResult, UrlError> {
    // Only the front of `url` is stripped: trailing spaces are kept.
    let url = url
        .trim_start_matches(is_c0_control_or_space)
        .replace(UNSAFE_URL_CHARS, "");
    let mut scheme = scheme
        .trim_matches(is_c0_control_or_space)
        .replace(UNSAFE_URL_CHARS, "");
    let mut rest = url.as_str();
    if let Some((candidate, after)) = rest.split_once(':')
        && candidate.starts_with(|c: char| c.is_ascii_alphabetic())
        && candidate.chars().all(is_scheme_char)
    {
        scheme = candidate.to_ascii_lowercase();
        rest = after;
    }
    let mut netloc = "";
    if let Some(after_slashes) = rest.strip_prefix("//") {
        (netloc, rest) = split_netloc(after_slashes);
        let has_open = netloc.contains('[');
        if has_open != netloc.contains(']') {
            return Err(UrlError);
        }
        if has_open {
            check_bracketed_netloc(netloc)?;
        }
    }
    let (rest, fragment) = rest.split_once('#').unwrap_or((rest, ""));
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    check_netloc(netloc)?;
    Ok(SplitResult {
        scheme,
        netloc: netloc.to_owned(),
        path: path.to_owned(),
        query: query.to_owned(),
        fragment: fragment.to_owned(),
    })
}

// upstream: cpython Lib/urllib/parse.py::_splitparams
fn split_params(url: &str) -> (&str, &str) {
    let found = match url.rfind('/') {
        Some(slash) => url
            .get(slash..)
            .and_then(|tail| tail.find(';'))
            .and_then(|i| i.checked_add(slash)),
        None => url.find(';'),
    };
    match found.and_then(|i| url.split_at_checked(i)) {
        Some((path, params)) => (path, params.get(1..).unwrap_or("")),
        None => (url, ""),
    }
}

// The path of the returned `SplitResult` has the `;params` cut off; they come back beside it.
// upstream: cpython Lib/urllib/parse.py::urlparse
fn urlparse(url: &str, scheme: &str) -> Result<(SplitResult, String), UrlError> {
    let mut split = urlsplit(url, scheme)?;
    let mut params = String::new();
    if USES_PARAMS.contains(&split.scheme.as_str()) && split.path.contains(';') {
        let (path, split_off) = split_params(&split.path);
        params = split_off.to_owned();
        split.path = path.to_owned();
    }
    Ok((split, params))
}

// upstream: cpython Lib/urllib/parse.py::urlunsplit
fn unsplit(scheme: &str, netloc: &str, path: &str, query: &str, fragment: &str) -> String {
    let mut url = if !netloc.is_empty() {
        if path.is_empty() || path.starts_with('/') {
            format!("//{netloc}{path}")
        } else {
            format!("//{netloc}/{path}")
        }
    } else if path.starts_with("//")
        || (!scheme.is_empty()
            && USES_NETLOC.contains(&scheme)
            && (path.is_empty() || path.starts_with('/')))
    {
        // A leading `//` of the path must not read as a netloc; a netloc-scheme URL keeps its `//`.
        format!("//{path}")
    } else {
        path.to_owned()
    };
    if !scheme.is_empty() {
        url = format!("{scheme}:{url}");
    }
    if !query.is_empty() {
        url = format!("{url}?{query}");
    }
    if !fragment.is_empty() {
        url = format!("{url}#{fragment}");
    }
    url
}

/// Python `urllib.parse.urlunsplit`: the URL the components make up, which may differ from the one
/// they were split from by delimiters with nothing after them.
// upstream: cpython Lib/urllib/parse.py::urlunsplit
pub fn urlunsplit(parts: &SplitResult) -> String {
    unsplit(
        &parts.scheme,
        &parts.netloc,
        &parts.path,
        &parts.query,
        &parts.fragment,
    )
}

// upstream: cpython Lib/urllib/parse.py::urlunparse
fn urlunparse(parsed: &SplitResult, params: &str) -> String {
    if params.is_empty() {
        return urlunsplit(parsed);
    }
    let path = format!("{};{params}", parsed.path);
    unsplit(
        &parsed.scheme,
        &parsed.netloc,
        &path,
        &parsed.query,
        &parsed.fragment,
    )
}

/// Python `urllib.parse.urljoin(base, url)` with `allow_fragments=True`: `url` resolved against
/// `base` by the rules of CPython 3.13, not WHATWG.
///
/// Errors where either side raises in `urlsplit`; an empty `base` or `url` returns the other
/// without parsing.
// upstream: cpython Lib/urllib/parse.py::urljoin
pub fn urljoin(base: &str, url: &str) -> Result<String, UrlError> {
    if base.is_empty() {
        return Ok(url.to_owned());
    }
    if url.is_empty() {
        return Ok(base.to_owned());
    }
    let (base_parts, base_params) = urlparse(base, "")?;
    let (mut parsed, mut params) = urlparse(url, &base_parts.scheme)?;

    if parsed.scheme != base_parts.scheme || !USES_RELATIVE.contains(&parsed.scheme.as_str()) {
        return Ok(url.to_owned());
    }
    if USES_NETLOC.contains(&parsed.scheme.as_str()) {
        if !parsed.netloc.is_empty() {
            return Ok(urlunparse(&parsed, &params));
        }
        parsed.netloc = base_parts.netloc.clone();
    }

    if parsed.path.is_empty() && params.is_empty() {
        parsed.path = base_parts.path.clone();
        params = base_params;
        if parsed.query.is_empty() {
            parsed.query = base_parts.query.clone();
        }
        return Ok(urlunparse(&parsed, &params));
    }

    let mut base_segments: Vec<&str> = base_parts.path.split('/').collect();
    if base_segments.last() != Some(&"") {
        // The last item is not a directory, so it is not taken into account.
        base_segments.pop();
    }
    // RFC 3986: a path with a leading `/` ignores the base path.
    let segments: Vec<&str> = if parsed.path.starts_with('/') {
        parsed.path.split('/').collect()
    } else {
        base_segments.extend(parsed.path.split('/'));
        // Drop empty inner segments, which would make redundant slashes on re-joining.
        let last = base_segments.len().saturating_sub(1);
        base_segments
            .iter()
            .enumerate()
            .filter(|&(i, segment)| i == 0 || i == last || !segment.is_empty())
            .map(|(_, &segment)| segment)
            .collect()
    };

    let mut resolved: Vec<&str> = Vec::new();
    for &segment in &segments {
        match segment {
            // A `..` with nothing to pop is ignored (RFC 3986).
            ".." => {
                resolved.pop();
            }
            "." => {}
            _ => resolved.push(segment),
        }
    }
    if matches!(segments.last(), Some(&"." | &"..")) {
        // The last segment was a relative directory: the path ends in a `/`.
        resolved.push("");
    }
    let joined = resolved.join("/");
    parsed.path = if joined.is_empty() {
        "/".to_owned()
    } else {
        joined
    };
    Ok(urlunparse(&parsed, &params))
}

#[cfg(test)]
pub(crate) mod cases;
#[cfg(test)]
mod tests;
