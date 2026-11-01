//! Metadata extraction of upstream `metadata.py`: the meta tag and JSON-LD readers and `extract_metadata`,
//! the entry point `core.py` calls.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::baseline::block_text;
use crate::cancel::Call;
use crate::courlan::{extract_domain, is_valid_url, normalize_url, validate_url};
use crate::dom::{LocalName, NodeId, Ns, Tree};
use crate::error::ExtractError;
use crate::html_processing::{prune_unwanted_nodes, tick};
use crate::htmldate::find_date;
use crate::json_metadata::{
    JsonFailure, extract_json, extract_json_parse_error, normalize_authors, normalize_json,
};
use crate::pyhtml::unescape;
use crate::pyjson::{Invalid, try_loads};
use crate::pystr::{self, PY_SPACE_CLASS, ignore_case_i};
use crate::settings::{DateParams, Document, set_date_params};
use crate::utils::{HTML_STRIP_TAGS, line_processing, safe_base_url, trim};
use crate::xpaths::{
    AUTHOR_DISCARD_XPATHS, AUTHOR_XPATHS, CATEGORIES_XPATHS, Selector, TAGS_XPATHS, TITLE_XPATHS,
};

// The patterns are literals that compile (`patterns_compile`); `None` would make the step that uses
// one a no-op instead of a panic.
fn compile(pattern: &str) -> Option<Regex> {
    Regex::new(pattern).ok()
}

/// A string, or whitespace outside one: the replacement keeps the string. `\s` is Python's.
// upstream: metadata.py::JSON_MINIFY
pub static JSON_MINIFY: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(&format!(r#"("(?:\\.|[^"\\])*")|[{PY_SPACE_CLASS}]"#)));

/// A quote character of a tag.
// upstream: metadata.py::CLEAN_META_TAGS
pub static CLEAN_META_TAGS: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r#"["']"#));

// upstream: metadata.py::METANAME_AUTHOR
const METANAME_AUTHOR: &[&str] = &[
    "article:author",
    "atc-metaauthor",
    "author",
    "authors",
    "byl",
    "citation_author",
    "creator",
    "dc.creator",
    "dc.creator.aut",
    "dc:creator",
    "dcterms.creator",
    "dcterms.creator.aut",
    "dcsext.author",
    "parsely-author",
    "rbauthors",
    "sailthru.author",
    "shareaholic:article_author_name",
];
// upstream: metadata.py::METANAME_DESCRIPTION
const METANAME_DESCRIPTION: &[&str] = &[
    "dc.description",
    "dc:description",
    "dcterms.abstract",
    "dcterms.description",
    "description",
    "sailthru.description",
    "twitter:description",
];
// upstream: metadata.py::METANAME_PUBLISHER
const METANAME_PUBLISHER: &[&str] = &[
    "article:publisher",
    "citation_journal_title",
    "copyright",
    "dc.publisher",
    "dc:publisher",
    "dcterms.publisher",
    "publisher",
    "sailthru.publisher",
    "rbpubname",
    "twitter:site",
];
// upstream: metadata.py::METANAME_TAG
const METANAME_TAG: &[&str] = &[
    "citation_keywords",
    "dcterms.subject",
    "keywords",
    "parsely-tags",
    "shareaholic:keywords",
    "tags",
];
// upstream: metadata.py::METANAME_TITLE
const METANAME_TITLE: &[&str] = &[
    "citation_title",
    "dc.title",
    "dcterms.title",
    "fb_title",
    "headline",
    "parsely-title",
    "sailthru.title",
    "shareaholic:title",
    "rbtitle",
    "title",
    "twitter:title",
];
// upstream: metadata.py::METANAME_IMAGE
const METANAME_IMAGE: &[&str] = &[
    "image",
    "og:image",
    "og:image:url",
    "og:image:secure_url",
    "twitter:image",
    "twitter:image:src",
];
// upstream: metadata.py::PROPERTY_AUTHOR
const PROPERTY_AUTHOR: &[&str] = &["author", "article:author"];
// upstream: metadata.py::TWITTER_ATTRS
const TWITTER_ATTRS: &[&str] = &["twitter:site", "application-name"];
// upstream: metadata.py::EXTRA_META. Only the unknown-attribute debug line reads it, which is not ported.
#[allow(dead_code)]
const EXTRA_META: &[&str] = &["charset", "http-equiv", "property"];
// upstream: metadata.py::OG_PROPERTIES
const OG_PROPERTIES: &[(&str, &str)] = &[
    ("og:title", "title"),
    ("og:description", "description"),
    ("og:site_name", "sitename"),
    ("og:image", "image"),
    ("og:image:url", "image"),
    ("og:image:secure_url", "image"),
    ("og:type", "pagetype"),
];
// upstream: metadata.py::OG_AUTHOR
const OG_AUTHOR: &[&str] = &["og:author", "og:article:author"];

/// The metadata of the page at `root`, as a [`Document`] with empty body and comments. `date_config`
/// is the date parameters, or `None` for the defaults built from `extensive`; `filedate` is its
/// `max_date`. Fails with `InvalidValue` on a character reference of more than 4300 digits and with
/// the call's error when `call` stops.
// upstream: metadata.py::extract_metadata (`filecontent` is always a tree here, so `load_html` is not ported)
pub fn extract_metadata(
    tree: &mut Tree,
    root: NodeId,
    default_url: Option<&str>,
    date_config: Option<&DateParams>,
    extensive: bool,
    author_blacklist: &HashSet<String>,
    call: &Call<'_>,
) -> Result<Document, ExtractError> {
    let date_config = date_config
        .cloned()
        .unwrap_or_else(|| set_date_params(extensive));

    // initialize dict and try to strip meta tags
    let mut metadata = examine_meta(tree, root, call)?;

    // to check: remove it and replace with author_blacklist in test case
    if metadata
        .author
        .as_deref()
        .is_some_and(|author| !author.is_empty() && !author.contains(' '))
    {
        metadata.author = None;
    }

    // fix: try json-ld metadata and override
    match extract_meta_json(tree, root, &mut metadata, call) {
        Ok(()) | Err(JsonFailure::Raised) => {}
        Err(JsonFailure::Stop(error)) => return Err(error),
    }

    // title
    if is_unset(&metadata.title) {
        metadata.title = extract_title(tree, root, call)?;
    }

    // check author in blacklist
    if !is_unset(&metadata.author) && !author_blacklist.is_empty() {
        metadata.author = metadata
            .author
            .as_deref()
            .and_then(|author| check_authors(author, author_blacklist));
    }
    // author
    if is_unset(&metadata.author) {
        metadata.author = extract_author(tree, root, call)?;
    }
    // recheck author in blacklist
    if !is_unset(&metadata.author) && !author_blacklist.is_empty() {
        metadata.author = metadata
            .author
            .as_deref()
            .and_then(|author| check_authors(author, author_blacklist));
    }

    // url
    if is_unset(&metadata.url) {
        metadata.url = extract_url(tree, root, default_url, call)?;
    }

    // hostname
    if let Some(url) = metadata.url.as_deref().filter(|url| !url.is_empty()) {
        metadata.hostname = extract_domain(url);
    }

    // extract date with external module htmldate
    metadata.date = find_date(tree, root, &date_config, metadata.url.as_deref(), call)?;

    // sitename
    if is_unset(&metadata.sitename) {
        metadata.sitename = extract_sitename(tree, root, call)?;
    }
    if let Some(sitename) = metadata.sitename.as_deref().filter(|name| !name.is_empty()) {
        // scrap Twitter ID
        let sitename = sitename.trim_start_matches('@');
        // capitalize
        let lowercase_start = sitename
            .chars()
            .next()
            .is_some_and(|first| !pystr::is_upper(first));
        metadata.sitename = Some(if lowercase_start && !sitename.contains('.') {
            pystr::title(sitename)
        } else {
            sitename.to_owned()
        });
    } else if let Some(url) = metadata.url.as_deref().filter(|url| !url.is_empty()) {
        // use URL
        metadata.sitename = META_URL
            .as_ref()
            .and_then(|regex| regex.captures(url))
            .and_then(|found| found.get(1))
            .map(|host| host.as_str().to_owned());
    }

    // categories
    if metadata
        .categories
        .as_deref()
        .is_none_or(<[String]>::is_empty)
    {
        metadata.categories = Some(extract_catstags("category", tree, root, call)?);
    }

    // tags
    if metadata.tags.as_deref().is_none_or(<[String]>::is_empty) {
        metadata.tags = Some(extract_catstags("tag", tree, root, call)?);
    }

    // license
    metadata.license = extract_license(tree, root, call)?;

    // safety checks
    metadata.filedate = date_config.max_date;
    metadata.clean_and_trim()?;

    Ok(metadata)
}

/// Python `not value` of an optional string.
fn is_unset(value: &Option<String>) -> bool {
    value.as_deref().is_none_or(str::is_empty)
}

/// `tags` unescaped, trimmed, without quote characters and without empty items; items are separated
/// by `, `. Fails with `InvalidValue` on a character reference of more than 4300 digits.
// upstream: metadata.py::normalize_tags
pub fn normalize_tags(tags: &str) -> Result<String, ExtractError> {
    let trimmed = trim(&unescape(tags)?);
    if trimmed.is_empty() {
        return Ok(String::new());
    }
    let unquoted = match CLEAN_META_TAGS.as_ref() {
        Some(regex) => regex.replace_all(&trimmed, ""),
        None => trimmed.as_str().into(),
    };
    Ok(unquoted
        .split(", ")
        .filter(|tag| !tag.is_empty())
        .collect::<Vec<_>>()
        .join(", "))
}

/// The authors of `authors` (separated by `;`) that are not in `author_blacklist`, compared by
/// lowercase, joined by `; `; `None` when none is left.
// upstream: metadata.py::check_authors
pub fn check_authors(authors: &str, author_blacklist: &HashSet<String>) -> Option<String> {
    let blacklist: HashSet<String> = author_blacklist
        .iter()
        .map(|author| pystr::lower(author))
        .collect();
    let kept: Vec<&str> = authors
        .split(';')
        .filter(|author| !blacklist.contains(&pystr::lower(pystr::strip(author))))
        .map(pystr::strip)
        .collect();
    (!kept.is_empty()).then(|| kept.join("; ").trim_matches([';', ' ']).to_owned())
}

/// The elements below `root` that satisfy `keep`, in document order; checks `call` about every
/// `CHECK_INTERVAL` nodes.
fn collect_nodes(
    tree: &Tree,
    root: NodeId,
    call: &Call<'_>,
    keep: impl Fn(NodeId) -> bool,
) -> Result<Vec<NodeId>, ExtractError> {
    let mut visited = 0;
    let mut found = Vec::new();
    for node in tree.iterdescendants(root, &[]) {
        tick(&mut visited, call)?;
        if keep(node) {
            found.push(node);
        }
    }
    Ok(found)
}

/// `.//head/meta` below `root`, with `keep` applied to the `meta` element.
fn head_meta(
    tree: &Tree,
    root: NodeId,
    call: &Call<'_>,
    keep: impl Fn(NodeId) -> bool,
) -> Result<Vec<NodeId>, ExtractError> {
    collect_nodes(tree, root, call, |node| {
        tree.tag(node) == Some("meta")
            && tree
                .node(node)
                .parent()
                .is_some_and(|parent| parent != root && tree.tag(parent) == Some("head"))
            && keep(node)
    })
}

/// `field` where it is truthy, else `value`: Python's `field or value`.
fn or_value(field: &mut Option<String>, value: Option<&str>) {
    if field.as_deref().is_none_or(str::is_empty) {
        *field = value.map(str::to_owned);
    }
}

/// Parses and extracts the metadata of the JSON-LD scripts into `metadata`. A script that raises in
/// upstream ends the loop with [`JsonFailure::Raised`]; what earlier scripts and the raising one set
/// before it stays in `metadata`.
// upstream: metadata.py::extract_meta_json
pub fn extract_meta_json(
    tree: &Tree,
    root: NodeId,
    metadata: &mut Document,
    call: &Call<'_>,
) -> Result<(), JsonFailure> {
    let scripts = collect_nodes(tree, root, call, |node| {
        tree.tag(node) == Some("script")
            && matches!(
                tree.get(node, "type"),
                Some("application/ld+json" | "application/settings+json")
            )
    })?;
    let mut visited = 0;
    for script in scripts {
        tick(&mut visited, call)?;
        let Some(text) = tree
            .node(script)
            .text
            .as_deref()
            .filter(|text| !text.is_empty())
        else {
            continue;
        };
        let minified = match JSON_MINIFY.as_ref() {
            Some(regex) => regex.replace_all(text, |found: &Captures<'_>| {
                found.get(1).map_or("", |kept| kept.as_str()).to_owned()
            }),
            None => text.into(),
        };
        let element_text = normalize_json(&minified)?;
        // strict=False: `trim` handles \n\r\t, but strict JSON rejects the whole 0x00-0x1F range
        match try_loads(&element_text, false, call)? {
            Ok(json) => extract_json(&json, json.root(), metadata, call)?,
            Err(Invalid::Decode) => extract_json_parse_error(&element_text, metadata, call)?,
            Err(Invalid::Raised) => return Err(JsonFailure::Raised),
        }
    }
    Ok(())
}

/// The OpenGraph properties of the `head`, as a [`Document`] with the seven keys of upstream's dict.
/// Fails with `InvalidValue` on an author with a character reference of more than 4300 digits.
// upstream: metadata.py::extract_opengraph
pub fn extract_opengraph(
    tree: &Tree,
    root: NodeId,
    call: &Call<'_>,
) -> Result<Document, ExtractError> {
    let mut result = Document::new();
    let metas = head_meta(tree, root, call, |node| {
        tree.get(node, "property")
            .is_some_and(|property| property.starts_with("og:"))
    })?;
    for elem in metas {
        let (Some(property_name), Some(content)) =
            (tree.get(elem, "property"), tree.get(elem, "content"))
        else {
            continue;
        };
        // safeguard
        if content.is_empty() || pystr::is_all_space(content) {
            continue;
        }
        if let Some((_, key)) = OG_PROPERTIES
            .iter()
            .find(|(name, _)| *name == property_name)
        {
            let field = match *key {
                "title" => &mut result.title,
                "description" => &mut result.description,
                "sitename" => &mut result.sitename,
                "image" => &mut result.image,
                _ => &mut result.pagetype,
            };
            *field = Some(content.to_owned());
        } else if property_name == "og:url" && is_valid_url(content) {
            result.url = Some(content.to_owned());
        } else if OG_AUTHOR.contains(&property_name) {
            result.author = normalize_authors(None, content, call)?;
        }
    }
    Ok(result)
}

/// The metadata of the `head` meta tags: OpenGraph first, then `property`, `name` and `itemprop`
/// attributes, each only where nothing is set. Fails with `InvalidValue` on a character reference of
/// more than 4300 digits in an author or tag.
// upstream: metadata.py::examine_meta
pub fn examine_meta(tree: &Tree, root: NodeId, call: &Call<'_>) -> Result<Document, ExtractError> {
    // bootstrap from potential OpenGraph tags
    let mut metadata = extract_opengraph(tree, root, call)?;
    let mut tags: Vec<String> = Vec::new();
    let mut backup_sitename: Option<String> = None;

    for elem in head_meta(tree, root, call, |node| tree.get(node, "content").is_some())? {
        let raw = tree.get(elem, "content").unwrap_or_default();
        let stripped = match HTML_STRIP_TAGS.as_ref() {
            Some(regex) => regex.replace_all(raw, ""),
            None => raw.into(),
        };
        let content_attr = pystr::strip(&stripped);
        if content_attr.is_empty() {
            continue;
        }
        if let Some(property) = tree.get(elem, "property") {
            let property_attr = pystr::lower(property);
            // no opengraph a second time
            if property_attr.starts_with("og:") {
                continue;
            }
            if property_attr == "article:tag" {
                tags.push(normalize_tags(content_attr)?);
            } else if PROPERTY_AUTHOR.contains(&property_attr.as_str()) {
                metadata.author =
                    normalize_authors(metadata.author.as_deref(), content_attr, call)?;
            } else if property_attr == "article:publisher" {
                or_value(&mut metadata.sitename, Some(content_attr));
            } else if METANAME_IMAGE.contains(&property_attr.as_str()) {
                or_value(&mut metadata.image, Some(content_attr));
            }
        } else if let Some(name) = tree.get(elem, "name") {
            let name_attr = pystr::lower(name);
            let name_attr = name_attr.as_str();
            if METANAME_AUTHOR.contains(&name_attr) {
                metadata.author =
                    normalize_authors(metadata.author.as_deref(), content_attr, call)?;
            } else if METANAME_TITLE.contains(&name_attr) {
                or_value(&mut metadata.title, Some(content_attr));
            } else if METANAME_DESCRIPTION.contains(&name_attr) {
                or_value(&mut metadata.description, Some(content_attr));
            } else if METANAME_PUBLISHER.contains(&name_attr) {
                or_value(&mut metadata.sitename, Some(content_attr));
            } else if METANAME_IMAGE.contains(&name_attr) {
                or_value(&mut metadata.image, Some(content_attr));
            } else if TWITTER_ATTRS.contains(&name_attr) || name_attr.contains("twitter:app:name") {
                backup_sitename = Some(content_attr.to_owned());
            } else if name_attr == "twitter:url"
                && metadata.url.as_deref().is_none_or(str::is_empty)
                && is_valid_url(content_attr)
            {
                metadata.url = Some(content_attr.to_owned());
            } else if METANAME_TAG.contains(&name_attr) {
                tags.push(normalize_tags(content_attr)?);
            }
        } else if let Some(itemprop) = tree.get(elem, "itemprop") {
            match pystr::lower(itemprop).as_str() {
                "author" => {
                    metadata.author =
                        normalize_authors(metadata.author.as_deref(), content_attr, call)?;
                }
                "description" => or_value(&mut metadata.description, Some(content_attr)),
                "headline" => or_value(&mut metadata.title, Some(content_attr)),
                _ => {}
            }
        }
        // a meta with none of these attributes: upstream logs it at debug level, which is not ported
    }

    // backups
    or_value(&mut metadata.sitename, backup_sitename.as_deref());
    metadata.tags = Some(tags);
    Ok(metadata)
}

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

#[cfg(test)]
mod tests;
