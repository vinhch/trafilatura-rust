//! `courlan/clean.py`: URL normalization (query and fragment trackers, default ports, punycode, path).

use std::borrow::Cow;

use crate::pyidna::decode_label;
use crate::pystr::{is_word, lower};
use crate::pyurl::{SplitResult, parse_qs, quote, urlencode_doseq, urlunsplit};

// The anchored alternatives of `TRACKERS_RE`, written out: `(?:dc|fbc|gc|twc|yc|ysc)lid`,
// `(?:click|gbra|msclk|igsh|partner|wbra)id` and `(?:ads?|mc|ga|gs|itm|mc|mkt|ml|mtm|oly|pk|utm|vero)_`.
const TRACKER_PREFIXES: &[&str] = &[
    "dclid",
    "fbclid",
    "gclid",
    "twclid",
    "yclid",
    "ysclid",
    "clickid",
    "gbraid",
    "msclkid",
    "igshid",
    "partnerid",
    "wbraid",
    "ad_",
    "ads_",
    "mc_",
    "ga_",
    "gs_",
    "itm_",
    "mkt_",
    "ml_",
    "mtm_",
    "oly_",
    "pk_",
    "utm_",
    "vero_",
];

// The keywords of the last alternative of `TRACKERS_RE`. `affi`, `affiliate`, `referer` and `referrer`
// are left out: `aff` and `ref` match wherever they would, and nothing follows the group.
const TRACKER_KEYWORDS: &[&str] = &[
    "aff", "campaign", "cid", "clid", "eid", "ga", "gl", "kwd", "keyword", "medium", "ref",
    "session", "source", "uid", "xtor",
];

/// Python's `TRACKERS_RE.search(text)`. The `(?:\b|_)` before a keyword is a Python word boundary
/// (the keyword starts with a word character, so the character before it is no word character, or
/// the text starts there) or an underscore.
// upstream: courlan/clean.py::TRACKERS_RE
fn is_tracker(text: &str) -> bool {
    if TRACKER_PREFIXES
        .iter()
        .any(|prefix| text.starts_with(prefix))
    {
        return true;
    }
    let mut previous: Option<char> = None;
    text.char_indices().any(|(index, c)| {
        let is_boundary = previous.is_none_or(|before| before == '_' || !is_word(before));
        previous = Some(c);
        is_boundary
            && text.get(index..).is_some_and(|tail| {
                TRACKER_KEYWORDS
                    .iter()
                    .any(|keyword| tail.starts_with(keyword))
            })
    })
}

/// The query without tracker parameters, keys sorted, re-encoded; empty for an empty query.
// upstream: courlan/clean.py::clean_query (strict=False, language=None)
pub(super) fn clean_query(querystring: &str) -> String {
    if querystring.is_empty() {
        return String::new();
    }
    let mut qdict = parse_qs(querystring);
    // `sorted` of `str` orders by code point, which is the byte order of UTF-8
    qdict.sort_unstable_by(|(a, _), (b, _)| a.cmp(b));
    qdict.retain(|(key, _)| !is_tracker(&lower(key)));
    urlencode_doseq(&qdict)
}

/// Decodes each `xn--` label of a host; a label that does not decode stays as it is.
// upstream: courlan/clean.py::decode_punycode
pub(super) fn decode_punycode(host: &str) -> Cow<'_, str> {
    if !host.contains("xn--") {
        return Cow::Borrowed(host);
    }
    let parts: Vec<Cow<str>> = host
        .split('.')
        .map(|part| {
            if lower(part).starts_with("xn--") {
                decode_label(part).map_or(Cow::Borrowed(part), Cow::Owned)
            } else {
                Cow::Borrowed(part)
            }
        })
        .collect();
    Cow::Owned(parts.join("."))
}

// upstream: courlan/clean.py::normalize_part
fn normalize_part(part: &str) -> String {
    quote(part, "/%!=:,-")
}

// upstream: courlan/clean.py::normalize_fragment (language=None)
fn normalize_fragment(fragment: &str) -> String {
    let fragment = if !fragment.contains('=') {
        Cow::Borrowed(fragment)
    } else if fragment.contains('&') {
        Cow::Owned(clean_query(fragment))
    } else if is_tracker(fragment) {
        Cow::Borrowed("")
    } else {
        Cow::Borrowed(fragment)
    };
    normalize_part(&fragment)
}

const DEFAULT_PORTS: [&str; 2] = [":80", ":443"];

/// Removes every `:80` and `:443` that follows a Python word character, in any part of the netloc
/// (userinfo too), and also the front of a longer number such as `:8080`.
// Python's `(?<=\w):(?:80|443)` has no lookbehind in Rust `regex`; the lookbehind reads the
// original text, so the end of a removed match counts as a word character.
// upstream: courlan/clean.py::NETLOC_RE
fn remove_default_ports(netloc: &str) -> String {
    let mut kept = String::with_capacity(netloc.len());
    let mut rest = netloc;
    let mut previous: Option<char> = None;
    while let Some(c) = rest.chars().next() {
        let default_port = if c == ':' && previous.is_some_and(is_word) {
            DEFAULT_PORTS
                .iter()
                .find_map(|port| rest.strip_prefix(port).map(|after| (port, after)))
        } else {
            None
        };
        if let Some((port, after)) = default_port {
            rest = after;
            previous = port.chars().last();
        } else {
            kept.push(c);
            rest = rest.strip_prefix(c).unwrap_or_default();
            previous = Some(c);
        }
    }
    kept
}

// upstream: courlan/clean.py::PATH1
fn collapse_slashes(path: &str) -> String {
    let mut collapsed = String::with_capacity(path.len());
    for c in path.chars() {
        if c != '/' || !collapsed.ends_with('/') {
            collapsed.push(c);
        }
    }
    collapsed
}

/// Drops leading `/..` segments, each ending the text or followed by `/`.
// Python's `^(?:/\.\.(?![^/]))+` has a lookahead; it is the next character here.
// upstream: courlan/clean.py::PATH2
fn strip_leading_parent_segments(path: &str) -> &str {
    let mut rest = path;
    while let Some(after) = rest.strip_prefix("/..") {
        if !after.is_empty() && !after.starts_with('/') {
            break;
        }
        rest = after;
    }
    rest
}

/// The normalized URL of `parsed`: lowercase scheme and host, decoded punycode, no default port of the
/// scheme, a clean path, query and fragment.
// upstream: courlan/clean.py::normalize_url (strict=False, language=None, trailing_slash=True)
pub fn normalize_url(parsed: &SplitResult) -> String {
    let scheme = lower(&parsed.scheme);
    let netloc = lower(&parsed.netloc);
    let netloc = decode_punycode(&netloc);
    // a port that does not parse counts as no port
    let port = parsed.port().ok().flatten();
    let netloc = if matches!(
        (scheme.as_str(), port),
        ("http", Some(80)) | ("https", Some(443))
    ) {
        Cow::Owned(remove_default_ports(&netloc))
    } else {
        netloc
    };
    let path = normalize_part(strip_leading_parent_segments(&collapse_slashes(
        &parsed.path,
    )));
    let query = clean_query(&parsed.query);
    let path = if !query.is_empty() && path.is_empty() {
        "/".to_owned()
    } else {
        path
    };
    urlunsplit(&SplitResult {
        scheme,
        netloc: netloc.into_owned(),
        path,
        query,
        fragment: normalize_fragment(&parsed.fragment),
    })
}
