//! The element examiners and their attribute tables of htmldate's `core.py`.

use crate::cancel::Call;
use crate::dom::{NodeId, Tree};
use crate::error::{ExtractError, Rejection};
use crate::html_processing::tick;
use crate::pydatetime::DateTime;
use crate::pystr;

use crate::htmldate::extractors::{FAST_TAGS, extract_url_date, try_date_expr_opts};
use crate::htmldate::utils::Extractor;
use crate::htmldate::validators::{
    Format, check_extracted_reference, int_text, is_valid_date_str, update_reference,
};

use super::{compare_reference, examine_text, has_plausible_candidates};

/// The meta keys that hold a publication date, by `name`, `property` or `http-equiv`; lowercase.
// upstream: core.py::DATE_ATTRIBUTES
pub(crate) const DATE_ATTRIBUTES: &[&str] = &[
    "analyticsattributes.articledate",
    "article.created",
    "article_date_original",
    "article:post_date",
    "article.published",
    "article:published",
    "article:published_date",
    "article:published_time",
    "article:publicationdate",
    "bt:pubdate",
    "citation_date",
    "citation_publication_date",
    "content_create_date",
    "created",
    "cxenseparse:recs:publishtime",
    "date",
    "date_created",
    "date_published",
    "datecreated",
    "dateposted",
    "datepublished",
    // Dublin Core: https://wiki.whatwg.org/wiki/MetaExtensions
    "dc.date",
    "dc.created",
    "dc.date.created",
    "dc.date.issued",
    "dc.date.publication",
    "dcsext.articlefirstpublished",
    "dcterms.created",
    "dcterms.date",
    "dcterms.issued",
    "dc:created",
    "dc:date",
    "displaydate",
    "doc_date",
    "field-name-post-date",
    "gentime",
    "mediator_published_time",
    "meta", // too loose?
    // Open Graph: https://opengraphprotocol.org/
    "og:article:published",
    "og:article:published_time",
    "og:datepublished",
    "og:pubdate",
    "og:publish_date",
    "og:published_time",
    "og:question:published_time",
    "og:regdate",
    "originalpublicationdate",
    "parsely-pub-date",
    "pdate",
    "ptime",
    "pubdate",
    "publishdate",
    "publish_date",
    "publish_time",
    "publish-date",
    "published-date",
    "published_date",
    "published_time",
    "publisheddate",
    "publication_date",
    "rbpubdate",
    "release_date",
    "rnews:datepublished",
    "sailthru.date",
    "shareaholic:article_published_time",
    "timestamp",
    "twt-published-at",
    "video:release_date",
    "vr:published_time",
];

/// The `name` values of meta keys that hold a modified date.
// upstream: core.py::NAME_MODIFIED
pub(crate) const NAME_MODIFIED: &[&str] = &[
    "lastdate",
    "lastmod",
    "lastmodified",
    "last-modified",
    "modified",
    "utime",
];

/// The `property` values of meta keys that hold a modified date.
// upstream: core.py::PROPERTY_MODIFIED
pub(crate) const PROPERTY_MODIFIED: &[&str] = &[
    "article:modified",
    "article:modified_date",
    "article:modified_time",
    "article:post_modified",
    "bt:moddate",
    "datemodified",
    "dc.modified",
    "dcterms.modified",
    "lastmodified",
    "modified_time",
    "modificationdate",
    "og:article:modified_time",
    "og:modified_time",
    "og:updated_time",
    "release_date",
    "revision_date",
    "updated_time",
];

/// The `itemprop` values of original (publication) dates.
// upstream: core.py::ITEMPROP_ATTRS_ORIGINAL
pub(crate) const ITEMPROP_ATTRS_ORIGINAL: &[&str] = &["datecreated", "datepublished", "pubyear"];

/// The `itemprop` values of modified dates.
// upstream: core.py::ITEMPROP_ATTRS_MODIFIED
pub(crate) const ITEMPROP_ATTRS_MODIFIED: &[&str] = &["datemodified", "dateupdate"];

/// The `class` values of `abbr` and `time` elements that mark a publication date.
// upstream: core.py::CLASS_ATTRS
pub(crate) const CLASS_ATTRS: &[&str] = &["date-published", "published", "time published"];

/// Tags whose elements are date candidates whatever their attributes.
// upstream: core.py::ALWAYS_TAGS
pub(crate) const ALWAYS_TAGS: &[&str] = &["footer", "small"];

/// Substrings of the first of `id` or `class` in source order that make an element a date candidate
/// (`[Mm]eta|time|publish|footer`).
// upstream: core.py::ID_CLASS_CUES
pub(crate) const ID_CLASS_CUES: &[&str] = &["Meta", "meta", "time", "publish", "footer"];

/// Substrings of a `class` that make an element a date candidate.
// upstream: core.py::CLASS_CUES
pub(crate) const CLASS_CUES: &[&str] = &[
    "info",
    "post_detail",
    "block-content",
    "byline",
    "subline",
    "posted",
    "submitted",
    "created-post",
    "publication",
    "author",
    "autor",
    "field-content",
    "fa-clock-o",
    "fa-calendar",
    "fecha",
    "parution",
];
/// Whether `elem` is a date candidate by its attributes: the first of `id` and `class` in source order is a
/// date cue, or `itemprop` is, or the `id` or `class` names a cue.
// upstream: core.py::is_date_candidate
pub(crate) fn is_date_candidate(tree: &Tree, elem: NodeId) -> bool {
    let mut first: Option<&str> = None;
    let mut itemprop: Option<&str> = None;
    for (key, value) in &tree.node(elem).attrs {
        if &**key == "itemprop" {
            itemprop = Some(&**value);
        } else if first.is_none() && matches!(&**key, "id" | "class") {
            first = Some(&**value);
        }
    }
    for cue in [first, itemprop].into_iter().flatten() {
        // `replace("D", "d")` folds only the capital D
        let folded = cue.replace('D', "d");
        if folded.contains("date") || folded.contains("datum") {
            return true;
        }
    }
    let Some(first) = first else {
        return false;
    };
    if ID_CLASS_CUES.iter().any(|cue| first.contains(*cue)) {
        return true;
    }
    if tree
        .get(elem, "class")
        .is_some_and(|class| CLASS_CUES.iter().any(|cue| class.contains(*cue)))
    {
        return true;
    }
    tree.get(elem, "id")
        .is_some_and(|id| id.contains("footer-info-lastmod"))
}

/// The HTML elements of `root`'s subtree named in `tags`, in document order; checks `call` at each node.
fn tagged_descendants(
    tree: &Tree,
    root: NodeId,
    tags: &[&str],
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    let mut found = Vec::new();
    let mut visited = 0;
    for id in tree.iterdescendants(root, &[]) {
        tick(&mut visited, call)?;
        if tree.tag(id).is_some_and(|name| tags.contains(&name)) {
            found.push(id);
        }
    }
    Ok(found)
}

/// The elements of `root`'s subtree that may carry a date, in document order. The always-tags are kept; the other
/// elements in scope (every element when `extensive_search`, else the fast tags) are kept when `is_date_candidate`
/// holds.
// upstream: core.py::date_candidates
pub(crate) fn date_candidates(
    tree: &Tree,
    root: NodeId,
    extensive_search: bool,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    let mut found = Vec::new();
    let mut visited = 0;
    for id in tree.iterdescendants(root, &[]) {
        tick(&mut visited, call)?;
        let tag = tree.tag(id);
        let always = tag.is_some_and(|name| ALWAYS_TAGS.contains(&name));
        let in_scope =
            extensive_search || always || tag.is_some_and(|name| FAST_TAGS.contains(&name));
        if in_scope && (always || is_date_candidate(tree, id)) {
            found.push(id);
        }
    }
    Ok(found)
}

/// The `title` and `h1` elements of `root`'s subtree, in document order: the union `.//title|.//h1`.
// upstream: core.py::find_date (`search_tree.xpath(".//title|.//h1")`)
pub(crate) fn title_h1_elements(
    tree: &Tree,
    root: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    tagged_descendants(tree, root, &["title", "h1"], call)
}

/// `examine_text` of the text and of the `title` attribute of each element, the first date found; nothing when
/// there are no candidates or more than `MAX_POSSIBLE_CANDIDATES`.
// upstream: core.py::examine_elements
pub(crate) fn examine_elements(
    tree: &Tree,
    elements: &[NodeId],
    options: &Extractor,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    if !has_plausible_candidates(elements.len()) {
        return Ok(None);
    }
    let mut visited = 0;
    for &elem in elements {
        tick(&mut visited, call)?;
        // the element text, then the link title (Blogspot)
        let content = tree.text_content(elem);
        let title = tree.get(elem, "title").unwrap_or("");
        for text in [content.as_str(), title] {
            if let Some(attempt) = examine_text(text, options, call)? {
                return Ok(Some(attempt));
            }
        }
    }
    Ok(None)
}

/// `YEAR-01-01` for a copyright year `content`, when that date is in the extractor's range.
// upstream: core.py::examine_header (the copyrightyear branch)
fn copyright_year_date(content: &str, options: &Extractor) -> Result<Option<String>, ExtractError> {
    let attempt = format!("{content}-01-01");
    if !is_valid_date_str(&attempt, &options.min, &options.max) {
        return Ok(None);
    }
    // unreachable: a string that validates reads its first four code points as `int` does
    let year = int_text(&attempt.chars().take(4).collect::<String>())
        .ok_or(ExtractError::Rejected(Rejection::InvalidValue))?;
    // unreachable: a validated year is in range, so the date exists
    let date = DateTime::new(year, 1, 1).ok_or(ExtractError::Rejected(Rejection::InvalidValue))?;
    Ok(Some(Format::Ymd.render(&date)))
}

/// The date of the header metas of `root`'s subtree: a date by name, property, itemprop, pubdate or http-equiv
/// wins; a URL date, a modified or copyright-year date is kept as a reserve.
// upstream: core.py::examine_header
pub(crate) fn examine_header(
    tree: &Tree,
    root: NodeId,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let mut headerdate: Option<String> = None;
    let mut reserve: Option<String> = None;
    let mut visited = 0;
    for elem in tree.iterdescendants(root, &[]) {
        tick(&mut visited, call)?;
        if tree.tag(elem) != Some("meta") {
            continue;
        }
        if tree.get(elem, "content").is_none() && tree.get(elem, "datetime").is_none() {
            continue;
        }
        let content = tree.get(elem, "content");
        if let Some(name) = tree.get(elem, "name") {
            let attribute = pystr::lower(name);
            if attribute == "og:url" {
                reserve = extract_url_date(content, options).or(reserve);
            } else if DATE_ATTRIBUTES.contains(&attribute.as_str()) {
                headerdate = try_date_expr_opts(content, options, call)?;
            } else if NAME_MODIFIED.contains(&attribute.as_str()) {
                if options.original {
                    reserve = try_date_expr_opts(content, options, call)?.or(reserve);
                } else {
                    headerdate = try_date_expr_opts(content, options, call)?;
                }
            }
        } else if let Some(property) = tree.get(elem, "property") {
            let attribute = pystr::lower(property);
            if (DATE_ATTRIBUTES.contains(&attribute.as_str())
                || PROPERTY_MODIFIED.contains(&attribute.as_str()))
                && let Some(attempt) = try_date_expr_opts(content, options, call)?
            {
                let stored = if options.original {
                    DATE_ATTRIBUTES
                } else {
                    PROPERTY_MODIFIED
                };
                // the other kind holds a date that hurts precision: a reserve
                if stored.contains(&attribute.as_str()) {
                    headerdate = Some(attempt);
                } else {
                    reserve = Some(attempt);
                }
            }
        } else if let Some(itemprop) = tree.get(elem, "itemprop") {
            let attribute = pystr::lower(itemprop);
            if ITEMPROP_ATTRS_ORIGINAL.contains(&attribute.as_str())
                || ITEMPROP_ATTRS_MODIFIED.contains(&attribute.as_str())
            {
                let value = tree
                    .get(elem, "datetime")
                    .filter(|value| !value.is_empty())
                    .or(content);
                if let Some(attempt) = try_date_expr_opts(value, options, call)? {
                    let stored = if options.original {
                        ITEMPROP_ATTRS_ORIGINAL
                    } else {
                        ITEMPROP_ATTRS_MODIFIED
                    };
                    if stored.contains(&attribute.as_str()) {
                        headerdate = Some(attempt);
                    }
                }
            } else if attribute == "copyrightyear"
                && let Some(year) = content
                && let Some(date) = copyright_year_date(year, options)?
            {
                reserve = Some(date);
            }
        } else if let Some(pubdate) = tree.get(elem, "pubdate") {
            if pystr::lower(pubdate) == "pubdate" {
                headerdate = try_date_expr_opts(content, options, call)?;
            }
        } else if let Some(http_equiv) = tree.get(elem, "http-equiv") {
            let attribute = pystr::lower(http_equiv);
            if attribute == "date" || attribute == "last-modified" {
                let attempt = try_date_expr_opts(content, options, call)?;
                // "date" is original, "last-modified" is updated
                if (attribute == "date") == options.original {
                    headerdate = attempt;
                } else {
                    reserve = attempt.or(reserve);
                }
            }
        }
        if headerdate.is_some() {
            break;
        }
    }
    Ok(headerdate.or(reserve))
}

/// The date of an `abbr` `data-utime` in UTC, `None` where Python's `fromtimestamp` refuses the value.
// upstream: core.py::examine_abbr_elements (the `data-utime` conversion)
fn utime_date(text: &str) -> Option<DateTime> {
    DateTime::from_timestamp_utc(int_text(text)?)
}

/// The date of the `abbr` elements of `root`'s subtree: a `data-utime`, a published `title`, or a long text; the
/// element text is the fallback.
// upstream: core.py::examine_abbr_elements
pub(crate) fn examine_abbr_elements(
    tree: &Tree,
    root: NodeId,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let elements = tagged_descendants(tree, root, &["abbr"], call)?;
    if !has_plausible_candidates(elements.len()) {
        return Ok(None);
    }
    let mut reference: Option<DateTime> = None;
    let mut visited = 0;
    for &elem in &elements {
        tick(&mut visited, call)?;
        if let Some(utime) = tree.get(elem, "data-utime") {
            // untrusted: a value outside the platform timestamp range is skipped
            let Some(candidate) = utime_date(utime) else {
                continue;
            };
            reference = Some(update_reference(reference, candidate, options.original));
        } else if tree
            .get(elem, "class")
            .is_some_and(|class| CLASS_ATTRS.contains(&class))
        {
            if let Some(trytext) = tree.get(elem, "title") {
                if options.original {
                    if let Some(attempt) = try_date_expr_opts(Some(trytext), options, call)? {
                        return Ok(Some(attempt));
                    }
                } else {
                    reference = compare_reference(reference, trytext, options, call)?;
                    // faster execution
                    if reference.is_some() {
                        break;
                    }
                }
            } else if let Some(text) = tree
                .node(elem)
                .text
                .as_deref()
                .filter(|text| text.chars().count() > 10)
            {
                reference = compare_reference(reference, text, options, call)?;
            }
        }
    }
    if let Some(found) = check_extracted_reference(reference.as_ref(), options) {
        return Ok(Some(found));
    }
    examine_elements(tree, &elements, options, call)
}

/// The newest (or, when `original`, the oldest) date of the `time` elements of `root`'s subtree. The shortcut
/// attributes return their date at once.
// upstream: core.py::examine_time_elements
pub(crate) fn examine_time_elements(
    tree: &Tree,
    root: NodeId,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let elements = tagged_descendants(tree, root, &["time"], call)?;
    if !has_plausible_candidates(elements.len()) {
        return Ok(None);
    }
    let mut reference: Option<DateTime> = None;
    let mut visited = 0;
    for &elem in &elements {
        tick(&mut visited, call)?;
        let datetime_attr = tree.get(elem, "datetime").unwrap_or("");
        if datetime_attr.chars().count() > 6 {
            let class_attr = tree.get(elem, "class").unwrap_or("");
            let shortcut_flag = if options.original {
                tree.get(elem, "pubdate") == Some("pubdate")
                    || class_attr.starts_with("entry-date")
                    || class_attr.starts_with("entry-time")
            } else {
                class_attr == "updated"
            };
            if shortcut_flag {
                if let Some(attempt) = try_date_expr_opts(Some(datetime_attr), options, call)? {
                    return Ok(Some(attempt));
                }
            } else {
                reference = compare_reference(reference, datetime_attr, options, call)?;
            }
        } else if let Some(text) = tree
            .node(elem)
            .text
            .as_deref()
            .filter(|text| text.chars().count() > 6)
        {
            reference = compare_reference(reference, text, options, call)?;
        }
    }
    Ok(check_extracted_reference(reference.as_ref(), options))
}
