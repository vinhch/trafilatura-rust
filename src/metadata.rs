//! Metadata extraction of upstream `metadata.py`: the meta tag and JSON-LD readers and `extract_metadata`,
//! the entry point `core.py` calls.

use std::collections::HashSet;
use std::sync::LazyLock;

use regex::{Captures, Regex};

use crate::cancel::Call;
use crate::courlan::{extract_domain, is_valid_url};
use crate::dom::{NodeId, Tree};
use crate::error::ExtractError;
use crate::html_processing::tick;
use crate::htmldate::find_date;
use crate::json_metadata::{
    JsonFailure, extract_json, extract_json_parse_error, normalize_authors, normalize_json,
};
use crate::pyhtml::unescape;
use crate::pyjson::{Invalid, try_loads};
use crate::pystr::{self, PY_SPACE_CLASS};
use crate::settings::{DateParams, Document, set_date_params, today_utc};
use crate::utils::{HTML_STRIP_TAGS, trim};

mod page;

use page::{META_URL, extract_author, extract_catstags, extract_license, extract_sitename};

#[cfg(test)]
use page::{HTMLTITLE_REGEX, LICENSE_REGEX, TEXT_LICENSE_REGEX, examine_title_element};

#[cfg(any(test, feature = "__internals"))]
pub use page::extract_metainfo;
pub use page::{extract_title, extract_url};

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
/// is the date parameters, or `None` for the defaults built from `extensive`; an absent `max_date`
/// is today's UTC date, and `filedate` is the `max_date` used. Fails with `InvalidValue` on a
/// character reference of more than 4300 digits and with the call's error when `call` stops.
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
    let mut date_config = date_config
        .cloned()
        .unwrap_or_else(|| set_date_params(extensive));
    // deviation: D-15 (upstream's default is the local date; the UTC date is read per call)
    if date_config.max_date.is_none() {
        date_config.max_date = Some(today_utc());
    }

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

#[cfg(test)]
mod tests;
