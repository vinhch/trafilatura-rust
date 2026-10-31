//! `tld/utils.py`: the suffix lookup of `get_tld`, over the `psl` crate instead of tld's own list.
//!
//! tld walks a trie of the Public Suffix List. `psl` answers the Public Suffix List algorithm, which
//! differs from the trie in two ways, and `suffix_length` shapes its answer to tld's:
//!
//! - tld's `Trie.add` marks the parent of every `!exception` rule as a suffix (`ck` for `!www.ck`), so
//!   the parents of `tld_tables` count as suffixes.
//! - tld holds the punycode form of a rule only at the top level (the `// xn--` lines of its list);
//!   `psl` holds both forms at every level. A label below the top that starts with `xn--` matches no
//!   rule but a wildcard, so it is masked before the lookup.

use psl::{List, Psl};

use super::tld_tables::TLD_EXCEPTION_PARENTS;
use crate::pystr::lower;
use crate::pyurl::urlsplit;

/// A label no rule holds, standing in for the `xn--` labels below the top level.
const MASK_LABEL: &str = "-";

/// The two fields of upstream's `tld.result.Result` that courlan reads.
pub(crate) struct TldResult {
    /// Upstream's `domain`: the label left of the suffix, or the suffix itself when the host is one.
    pub domain: String,
    /// Upstream's `fld`: `domain` and the suffix, or the host when it is a suffix.
    pub fld: String,
}

/// The number of labels of the longest suffix tld finds in `labels`, or `None` when it finds none.
// upstream: tld/utils.py::process_url (the trie walk)
fn suffix_length(labels: &[&str]) -> Option<usize> {
    let last = labels.len().checked_sub(1)?;
    let masked: Vec<&str> = labels
        .iter()
        .enumerate()
        .map(|(index, &label)| {
            if index < last && label.starts_with("xn--") {
                MASK_LABEL
            } else {
                label
            }
        })
        .collect();
    let known_length = List
        .suffix(masked.join(".").as_bytes())
        .filter(|suffix| suffix.is_known())
        .map_or(0, |suffix| {
            suffix
                .as_bytes()
                .iter()
                .filter(|&&byte| byte == b'.')
                .count()
                + 1
        });
    let host = labels.join(".");
    let parent_length = TLD_EXCEPTION_PARENTS
        .iter()
        .filter(|parent| {
            host.strip_suffix(**parent)
                .is_some_and(|rest| rest.is_empty() || rest.ends_with('.'))
        })
        .map(|parent| parent.split('.').count())
        .max()
        .unwrap_or(0);
    Some(known_length.max(parent_length)).filter(|&length| length > 0)
}

/// The domain and full domain of the host of `url` by tld's suffix list; `None` where upstream's
/// `get_tld(url, as_object=True, fail_silently=True)` returns `None`: the URL does not split, has no
/// host, or its suffix is not listed.
// upstream: tld/utils.py::get_tld (as_object=True, fail_silently=True, both public and private suffixes)
pub(crate) fn get_tld(url: &str) -> Option<TldResult> {
    // upstream: tld/utils.py::process_url
    let hostname = urlsplit(url, "").ok()?.hostname()?;
    let lowered = lower(&hostname);
    let labels: Vec<&str> = lowered.trim_end_matches('.').split('.').collect();
    let tld_length = suffix_length(&labels)?;

    // upstream: tld/result.py::Result.__init__
    let Some(non_zero_i) = labels.len().checked_sub(tld_length).filter(|&n| n > 0) else {
        // the host is a suffix; `Result` keeps its trailing dots
        return Some(TldResult {
            domain: hostname.clone(),
            fld: hostname,
        });
    };
    let suffix = labels.get(non_zero_i..)?.join(".");
    let domain = *labels.get(non_zero_i.checked_sub(1)?)?;
    if domain.is_empty() {
        return Some(TldResult {
            domain: suffix.clone(),
            fld: suffix,
        });
    }
    Some(TldResult {
        domain: domain.to_owned(),
        fld: format!("{domain}.{suffix}"),
    })
}
