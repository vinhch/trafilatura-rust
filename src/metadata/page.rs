//! The page-element readers of upstream `metadata.py`: title, author, URL, sitename, category, tags and license.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::Regex;

use crate::baseline::block_text;
use crate::cancel::Call;
use crate::courlan::{normalize_url, validate_url};
use crate::dom::{LocalName, NodeId, Ns, Tree};
use crate::error::ExtractError;
use crate::html_processing::{prune_unwanted_nodes, tick};
use crate::json_metadata::normalize_authors;
use crate::pystr::{self, PY_SPACE_CLASS, ignore_case_i};
use crate::utils::{line_processing, safe_base_url, trim};
use crate::xpaths::{
    AUTHOR_DISCARD_XPATHS, AUTHOR_XPATHS, CATEGORIES_XPATHS, Selector, TAGS_XPATHS, TITLE_XPATHS,
};

use super::{collect_nodes, compile};

// The separators are those of upstream's class, in its order. `\s` is Python's, `$` is `\n?\z` and
// `re.match` starts at `\A`; the pattern is a literal that compiles (`tree_field_patterns_compile`).
// upstream: metadata.py::HTMLTITLE_REGEX
pub static HTMLTITLE_REGEX: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile(&format!(
        r"\A(.+)?[{PY_SPACE_CLASS}]+[–•·—|⁄*⋆~‹«<›»>:-][{PY_SPACE_CLASS}]+(.+)\n?\z"
    ))
});

// upstream: metadata.py::LICENSE_REGEX
pub static LICENSE_REGEX: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r"/(by-nc-nd|by-nc-sa|by-nc|by-nd|by-sa|by|zero)/([1-9]\.[0-9])"));

// `re.IGNORECASE`: the `i` of `creative` goes through `ignore_case_i`.
// upstream: metadata.py::TEXT_LICENSE_REGEX
pub static TEXT_LICENSE_REGEX: LazyLock<Option<Regex>> = LazyLock::new(|| {
    compile(&format!(
        r"(?i)(cc|{}) (by-nc-nd|by-nc-sa|by-nc|by-nd|by-sa|by|zero) ?([1-9]\.[0-9])?",
        ignore_case_i("creative commons")
    ))
});

/// The scheme and host of a URL, as `metadata.py` reads them; matched at the start.
// upstream: metadata.py::META_URL
pub static META_URL: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r"\Ahttps?://(?:www\.|w[0-9]+\.)?([^/]+)"));

// upstream: metadata.py::extract_catstags (`"/" + "categor" + "(?:y|ies|s)?/"`)
static CATEGORY_HREF: LazyLock<Option<Regex>> = LazyLock::new(|| compile("/categor(?:y|ies|s)?/"));

// upstream: metadata.py::extract_catstags (`"/" + "tag" + "(?:y|ies|s)?/"`)
static TAG_HREF: LazyLock<Option<Regex>> = LazyLock::new(|| compile("/tag(?:y|ies|s)?/"));

// upstream: metadata.py::URL_SELECTORS (`.//head//link[@rel="canonical"]`)
fn is_canonical_link(tree: &Tree, node: NodeId) -> bool {
    tree.tag(node) == Some("link") && tree.get(node, "rel") == Some("canonical")
}

// upstream: metadata.py::URL_SELECTORS (`.//head//base`)
fn is_base(tree: &Tree, node: NodeId) -> bool {
    tree.tag(node) == Some("base")
}

// upstream: metadata.py::URL_SELECTORS (`.//head//link[@rel="alternate"][@hreflang="x-default"]`)
fn is_default_alternate_link(tree: &Tree, node: NodeId) -> bool {
    tree.tag(node) == Some("link")
        && tree.get(node, "rel") == Some("alternate")
        && tree.get(node, "hreflang") == Some("x-default")
}

/// The elements below `root` that satisfy `keep` and sit inside a `head` that is itself below
/// `root`, in document order: `.//head//*[keep]`.
fn head_descendants(
    tree: &Tree,
    root: NodeId,
    call: &Call<'_>,
    keep: impl Fn(NodeId) -> bool,
) -> Result<Vec<NodeId>, ExtractError> {
    collect_nodes(tree, root, call, |node| {
        keep(node)
            && tree
                .iterancestors(node, &[])
                .take_while(|&up| up != root)
                .any(|up| tree.tag(up) == Some("head"))
    })
}

/// The first text of the elements `expressions` select below `root` that is longer than 2 and shorter
/// than `len_limit` code points, trimmed; `None` when no element qualifies.
// upstream: metadata.py::extract_metainfo
pub fn extract_metainfo(
    tree: &Tree,
    root: NodeId,
    expressions: &[Selector],
    len_limit: usize,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    for expression in expressions {
        // examine all results
        for elem in expression(tree, root, call)? {
            // Checked per match: each join reads a whole subtree, and matches nest.
            call.check()?;
            // `" ".join` keeps the empty pieces
            let pieces: Vec<&str> = tree.itertext_lxml(elem).collect();
            let content = trim(&pieces.join(" "));
            let length = content.chars().count();
            if 2 < length && length < len_limit {
                return Ok(Some(content));
            }
        }
    }
    Ok(None)
}

/// The text of the `title` element in the `head`, trimmed, with the parts before and after its
/// separator when it has one. Fails with `Cancelled` when `call` is cancelled.
// upstream: metadata.py::examine_title_element
pub fn examine_title_element(
    tree: &Tree,
    root: NodeId,
    call: &Call<'_>,
) -> Result<(String, Option<String>, Option<String>), ExtractError> {
    let titles = head_descendants(tree, root, call, |node| tree.tag(node) == Some("title"))?;
    let Some(&title_element) = titles.first() else {
        return Ok((String::new(), None, None));
    };
    let title = trim(&tree.text_content(title_element));
    let parts = HTMLTITLE_REGEX
        .as_ref()
        .and_then(|regex| regex.captures(&title))
        .map(|found| {
            let part = |index| found.get(index).map(|text| text.as_str().to_owned());
            (part(1), part(2))
        });
    Ok(match parts {
        Some((first, second)) => (title, first, second),
        None => (title, None, None),
    })
}

/// The title of the page: a lone `h1`, else the first selector hit, the title element, the first
/// non-empty `h1` or the first `h2`; `None` when all are empty.
// upstream: metadata.py::extract_title
pub fn extract_title(
    tree: &mut Tree,
    root: NodeId,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    // only one h1-element: take it
    let h1_results = collect_nodes(tree, root, call, |node| tree.tag(node) == Some("h1"))?;
    if let [only] = h1_results.as_slice() {
        let title = block_text(tree, *only, call)?;
        if !title.is_empty() {
            return Ok(Some(title));
        }
    }
    // extract using x-paths
    if let Some(title) = extract_metainfo(tree, root, TITLE_XPATHS, 200, call)? {
        return Ok(Some(title));
    }
    // extract using title tag
    let (mut title, first, second) = examine_title_element(tree, root, call)?;
    for part in [first.as_deref(), second.as_deref(), Some(title.as_str())]
        .into_iter()
        .flatten()
    {
        if !part.is_empty() && !part.contains('.') {
            return Ok(Some(part.to_owned()));
        }
    }
    // take first non-empty h1-title
    for h1_result in h1_results {
        title = block_text(tree, h1_result, call)?;
        if !title.is_empty() {
            return Ok(Some(title));
        }
    }
    // take first h2-title
    let h2_results = collect_nodes(tree, root, call, |node| tree.tag(node) == Some("h2"))?;
    if let Some(&h2_result) = h2_results.first() {
        title = block_text(tree, h2_result, call)?;
    }
    Ok((!title.is_empty()).then_some(title))
}

/// The author of the page, read from a copy of `root` without comment sections, dates and figures;
/// `tree` is left as it is. Fails with `InvalidValue` on a character reference of more than 4300
/// digits.
// upstream: metadata.py::extract_author
pub fn extract_author(
    tree: &Tree,
    root: NodeId,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    // the copy lives in a scratch tree, so it does not stay in the caller's arena
    let mut scratch = Tree::new(LocalName::from("html"), Ns::Html, Vec::new());
    let subtree = tree.copy_into(root, &mut scratch);
    prune_unwanted_nodes(&mut scratch, subtree, AUTHOR_DISCARD_XPATHS, false, call)?;
    let Some(author) = extract_metainfo(&scratch, subtree, AUTHOR_XPATHS, 120, call)? else {
        return Ok(None);
    };
    normalize_authors(None, &author, call)
}

/// The scheme and host of the first `head` meta tag with a `content` whose `name`, else `property`,
/// starts with `og:` or `twitter:`; `None` when no such tag has a host.
fn meta_base_url(
    tree: &Tree,
    root: NodeId,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let metas = head_descendants(tree, root, call, |node| {
        tree.tag(node) == Some("meta") && tree.get(node, "content").is_some()
    })?;
    let mut visited = 0;
    for element in metas {
        tick(&mut visited, call)?;
        let attrtype = [tree.get(element, "name"), tree.get(element, "property")]
            .into_iter()
            .flatten()
            .find(|value| !value.is_empty())
            .unwrap_or_default();
        if attrtype.starts_with("og:") || attrtype.starts_with("twitter:") {
            let base_url = safe_base_url(tree.get(element, "content").unwrap_or_default());
            if !base_url.is_empty() {
                return Ok(Some(base_url));
            }
        }
    }
    Ok(None)
}

/// The normalized URL of the canonical link, else of the base or default alternate link; an
/// invalid one gives `default_url`, returned as it is.
// upstream: metadata.py::extract_url
pub fn extract_url(
    tree: &Tree,
    root: NodeId,
    default_url: Option<&str>,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let mut url = None;
    for selector in [is_canonical_link, is_base, is_default_alternate_link] {
        let elements = head_descendants(tree, root, call, |node| selector(tree, node))?;
        url = elements
            .first()
            .and_then(|&element| tree.get(element, "href"))
            .filter(|href| !href.is_empty())
            .map(str::to_owned);
        if url.is_some() {
            break;
        }
    }

    url = match url {
        // protocol-relative URL: only the scheme is missing
        Some(href) if href.starts_with("//") => Some(format!("https:{href}")),
        // fix relative URLs
        Some(href) if href.starts_with('/') => Some(match meta_base_url(tree, root, call)? {
            Some(base_url) => base_url + &href,
            None => href,
        }),
        other => other,
    };

    // do not return invalid URLs
    let url = url
        .and_then(|candidate| validate_url(&candidate))
        .map(|parsed| normalize_url(&parsed))
        .filter(|normalized| !normalized.is_empty());
    Ok(url.or_else(|| default_url.map(str::to_owned)))
}

/// The part of the title element, before or after its separator, that holds a dot; `None` when
/// neither does. Fails with `Cancelled` when `call` is cancelled.
// upstream: metadata.py::extract_sitename
pub fn extract_sitename(
    tree: &Tree,
    root: NodeId,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let (_, first, second) = examine_title_element(tree, root, call)?;
    Ok([first, second]
        .into_iter()
        .flatten()
        .find(|part| part.contains('.')))
}

/// The categories (`metatype` `category`) or tags (any other) of the page: the texts of the links
/// whose `href` holds `/<metatype>/` in singular or plural, without empty texts and duplicates.
/// Categories fall back to the `article:section` and `subject` meta tags.
// upstream: metadata.py::extract_catstags
pub fn extract_catstags(
    metatype: &str,
    tree: &mut Tree,
    root: NodeId,
    call: &Call<'_>,
) -> Result<Vec<String>, ExtractError> {
    let is_category = metatype == "category";
    // upstream builds `"/" + metatype.rstrip("y") + "(?:y|ies|s)?/"` at run time; only these two
    // metatypes exist
    let (regexpr, xpath_expression) = if is_category {
        (&*CATEGORY_HREF, CATEGORIES_XPATHS)
    } else {
        (&*TAG_HREF, TAGS_XPATHS)
    };
    let mut results: Vec<String> = Vec::new();
    let mut visited = 0;
    // search using custom expressions
    for catexpr in xpath_expression {
        for elem in catexpr(tree, root, call)? {
            tick(&mut visited, call)?;
            let href = tree.get(elem, "href").unwrap_or_default();
            if regexpr.as_ref().is_some_and(|regex| regex.is_match(href)) {
                results.push(block_text(tree, elem, call)?);
            }
        }
        if !results.is_empty() {
            break;
        }
    }
    // category fallback
    if is_category && results.is_empty() {
        let metas = head_descendants(tree, root, call, |node| {
            tree.tag(node) == Some("meta")
                && (tree.get(node, "property") == Some("article:section")
                    || tree
                        .get(node, "name")
                        .is_some_and(|name| name.contains("subject")))
                && tree.get(node, "content").is_some()
        })?;
        results.extend(
            metas
                .into_iter()
                .filter_map(|element| tree.get(element, "content").map(str::to_owned)),
        );
    }
    // `dict.fromkeys` over `line_processing`: a line that is only space gives `None`, a key that is
    // dropped afterwards
    let mut seen: HashSet<Option<String>> = HashSet::new();
    let mut unique: Vec<String> = Vec::new();
    for line in results.iter().filter(|text| !text.is_empty()) {
        let processed = line_processing(line, false, false);
        if seen.insert(processed.clone()) {
            unique.extend(processed.filter(|text| !text.is_empty()));
        }
    }
    Ok(unique)
}

/// The license a link names: a Creative Commons one from its `href`, else its text, which in
/// `strict` mode counts only when it names a Creative Commons license.
// upstream: metadata.py::parse_license_element
pub fn parse_license_element(
    tree: &mut Tree,
    element: NodeId,
    strict: bool,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    // look for Creative Commons elements
    let href = tree.get(element, "href").unwrap_or_default();
    if let Some(found) = LICENSE_REGEX
        .as_ref()
        .and_then(|regex| regex.captures(href))
    {
        let kind = found.get(1).map_or("", |kind| kind.as_str());
        let version = found.get(2).map_or("", |version| version.as_str());
        return Ok(Some(format!("CC {} {version}", kind.to_ascii_uppercase())));
    }
    let text = block_text(tree, element, call)?;
    if text.is_empty() {
        return Ok(None);
    }
    // check if it could be a CC license
    if strict {
        return Ok(TEXT_LICENSE_REGEX
            .as_ref()
            .and_then(|regex| regex.find(&text))
            .map(|found| found.as_str().to_owned()));
    }
    Ok(Some(text))
}

/// The license of the page: the first `rel="license"` link that names one, else the first link in
/// a footer that names a Creative Commons one.
// upstream: metadata.py::extract_license
pub fn extract_license(
    tree: &mut Tree,
    root: NodeId,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    // look for links labeled as license
    let links = collect_nodes(tree, root, call, |node| {
        tree.tag(node) == Some("a")
            && tree.get(node, "href").is_some()
            && tree.get(node, "rel").is_some_and(|rel| {
                pystr::split_whitespace(&pystr::lower(rel)).any(|token| token == "license")
            })
    })?;
    let mut visited = 0;
    for element in links {
        tick(&mut visited, call)?;
        if let Some(result) = parse_license_element(tree, element, false, call)? {
            return Ok(Some(result));
        }
    }
    // probe footer elements for CC links
    let is_footer = |node: NodeId| {
        tree.tag(node) == Some("footer")
            || (tree.tag(node) == Some("div")
                && ["class", "id"].iter().any(|name| {
                    tree.get(node, name)
                        .is_some_and(|value| value.contains("footer"))
                }))
    };
    let links = collect_nodes(tree, root, call, |node| {
        tree.tag(node) == Some("a")
            && tree.get(node, "href").is_some()
            && tree
                .iterancestors(node, &[])
                .take_while(|&up| up != root)
                .any(is_footer)
    })?;
    for element in links {
        tick(&mut visited, call)?;
        if let Some(result) = parse_license_element(tree, element, true, call)? {
            return Ok(Some(result));
        }
    }
    Ok(None)
}
