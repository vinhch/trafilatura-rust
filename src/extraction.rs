//! The extraction entry points of upstream `core.py`: the output step, the extraction cascade and the
//! functions that run them on a page or a caller's tree.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::Regex;

use crate::baseline::{baseline_tree, html2txt_tree};
use crate::cancel::Call;
use crate::context::Ctx;
use crate::deduplication::{content_fingerprint, duplicate_test};
use crate::dom::{LocalName, NodeId, Ns, ParseLimits, Tree};
use crate::error::{ExtractError, Rejection, caught};
use crate::html_processing::{
    build_html_output, convert_tags, prune_unwanted_nodes, tick, tree_cleaning,
};
use crate::main_extractor::{elem_text, extract_comments, extract_content};
use crate::metadata::extract_metadata;
use crate::pyjson::dumps_str;
use crate::pystr::{self, PY_SPACE_CLASS, is_alpha};
use crate::settings::{Document, Extractor, Field, Focus, Format};
use crate::utils::{HtmlInput, check_html_lang, language_filter, load_html, normalize_unicode};
use crate::xml::{build_json_output, control_xml_output, meta_items, xml_to_csv, xml_to_text};
use crate::xpaths::{RAW_TREE_PRUNE_XPATH, REMOVE_COMMENTS_AND_LISTS_XPATH, REMOVE_COMMENTS_XPATH};

/// The output formats whose result is text.
// upstream: core.py::TXT_FORMATS
pub const TXT_FORMATS: &[Format] = &[Format::Markdown, Format::Txt];

/// The metadata fields of the text header, in output order.
// upstream: core.py::_YAML_FIELDS
pub const YAML_FIELDS: &[&str] = &[
    "title",
    "author",
    "url",
    "hostname",
    "description",
    "sitename",
    "date",
    "categories",
    "tags",
    "fingerprint",
    "id",
    "license",
];

/// The words a YAML reader takes for a non-string, compared in lower case.
// upstream: core.py::_YAML_RESERVED
pub const YAML_RESERVED: &[&str] = &[
    "true", "false", "yes", "no", "on", "off", "y", "n", "null", "none", "~",
];

/// `value` as a plain YAML scalar when that reads back as the same string, else as a double-quoted
/// one (a JSON string).
// upstream: core.py::_yaml_scalar
fn yaml_scalar(value: &str) -> String {
    let plain = value.chars().next().is_some_and(is_alpha)
        && value == pystr::strip(value)
        && !value.contains(": ")
        && !value.contains(" #")
        && !value.ends_with(':')
        // `lower()` of a non-ASCII character is never an ASCII letter a reserved word holds (U+212A
        // gives `k`, which none has), so an ASCII-insensitive comparison finds the same words
        && !YAML_RESERVED
            .iter()
            .any(|word| value.eq_ignore_ascii_case(word))
        && value.chars().all(|ch| ch >= ' ' && ch != '\x7f');
    if plain {
        value.to_owned()
    } else {
        dumps_str(value)
    }
}

/// The header of the text formats: the non-empty `YAML_FIELDS` of `doc` between two `---` lines,
/// a list as Python's `repr` writes it.
// upstream: core.py::determine_returnstring
fn metadata_header(doc: &Document) -> String {
    let mut header = String::from("---\n");
    for &name in YAML_FIELDS {
        match doc.field(name) {
            Some(Field::Text(text)) => header.push_str(&format!("{name}: {}\n", yaml_scalar(text))),
            Some(Field::List(items)) => {
                let items: Vec<String> = items.iter().map(|item| pystr::repr(item)).collect();
                header.push_str(&format!("{name}: [{}]\n", items.join(", ")));
            }
            None => {}
        }
    }
    header.push_str("---\n");
    header
}

/// Converts the body of `doc` to the output format of `options`, as a string in Unicode normal
/// form NFC. The XML, CSV and JSON formats are stubs until phase 6. Fails with
/// `Rejection::InvalidValue` where `xml_to_text` and `build_html_output` do.
// upstream: core.py::determine_returnstring
pub fn determine_returnstring(
    doc: &mut Document,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<String, ExtractError> {
    let returnstring = match options.format {
        Format::Xml | Format::XmlTei => {
            // todo(phase 6): xml (the last cleaning of empty elements, then the output tree)
            control_xml_output(doc, options)
        }
        Format::Csv => xml_to_csv(doc, options.formatting),
        Format::Json => build_json_output(doc, options.with_metadata),
        Format::Html => {
            let items = options.with_metadata.then(|| meta_items(doc));
            let pairs: Option<Vec<(&str, &str)>> = items.as_ref().map(|items| {
                items
                    .iter()
                    .map(|(name, value)| (*name, value.as_str()))
                    .collect()
            });
            build_html_output(&mut doc.tree, doc.body, pairs.as_deref(), call)?
        }
        // Markdown and TXT, which is also upstream's `else` for the Python format
        Format::Markdown | Format::Txt | Format::Python => {
            let header = if options.with_metadata {
                metadata_header(doc)
            } else {
                String::new()
            };
            let text = xml_to_text(&mut doc.tree, Some(doc.body), options.formatting, call)?;
            // the comments body is never missing (`Document` always has one), so the join and the
            // strip always run
            let comments = xml_to_text(
                &mut doc.tree,
                Some(doc.comments_body),
                options.formatting,
                call,
            )?;
            pystr::strip(&format!("{header}{text}\n{comments}")).to_owned()
        }
    };
    Ok(normalize_unicode(&returnstring).into_owned())
}

/// Only an extraction shorter than this many characters is considered for the recall escalation.
// upstream: core.py::ESCALATION_MAX_LENGTH
pub const ESCALATION_MAX_LENGTH: usize = 3000;

/// The escalation also needs the extraction to cover less than this share of the page text.
// upstream: core.py::ESCALATION_PAGE_SHARE
pub const ESCALATION_PAGE_SHARE: f64 = 0.2;

/// The recall retry replaces the first pass when it is longer by more than this factor.
// upstream: core.py::ESCALATION_ACCEPT_RATIO
pub const ESCALATION_ACCEPT_RATIO: f64 = 1.5;

// `\s` as Python's `re` reads it, which includes `\x1c`-`\x1f` (`regex`'s does not). A pattern is a
// literal that compiles (`forum_pattern_compiles` forces it); `None` would make every page no
// forum instead of panicking.
// upstream: core.py::_DISCUSSION_FORUM_POSTING_RE
static DISCUSSION_FORUM_POSTING_RE: LazyLock<Option<Regex>> = LazyLock::new(|| {
    let s = format!("[{PY_SPACE_CLASS}]");
    Regex::new(&format!(
        r#""@type"{s}*:{s}*"DiscussionForumPosting"|"@type"{s}*:{s}*\[[^\]]*"DiscussionForumPosting""#
    ))
    .ok()
});

/// Whether a `ld+json` script below `root` declares a `DiscussionForumPosting`: a thread page whose
/// posts sit in the containers the comment prune would remove.
// upstream: core.py::_forum_thread_page
fn forum_thread_page(tree: &Tree, root: NodeId, call: &Call<'_>) -> Result<bool, ExtractError> {
    let Some(pattern) = DISCUSSION_FORUM_POSTING_RE.as_ref() else {
        return Ok(false);
    };
    let mut visited = 0usize;
    for script in tree.iterdescendants(root, &["script"]) {
        tick(&mut visited, call)?;
        if tree.get(script, "type") == Some("application/ld+json")
            && tree
                .node(script)
                .text
                .as_deref()
                .is_some_and(|text| pattern.is_match(text))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Cleans a copy of the raw tree below `raw_root` and converts its tags; returns the converted tree
/// and a copy of the cleaned tree from before the conversion.
// upstream: core.py::_prepare_tree
fn prepare_tree(
    tree: &mut Tree,
    raw_root: NodeId,
    ctx: &Ctx<'_>,
    url: Option<&str>,
) -> Result<(NodeId, NodeId), ExtractError> {
    let copy = tree.copy(raw_root);
    let cleaned = tree_cleaning(tree, copy, ctx)?;
    let backup = tree.copy(cleaned);
    convert_tags(tree, cleaned, ctx, url)?;
    Ok((cleaned, backup))
}

/// Cascade stages 1 and 2: the main extractor, then the comparison with the external extractors.
// upstream: core.py::_extract_and_compare
fn extract_and_compare(
    tree: &mut Tree,
    cleaned: NodeId,
    ctx: &Ctx<'_>,
) -> Result<(NodeId, String), ExtractError> {
    // deviation: D-1 (`if not options.fast`: compare_extraction, readability and jusText)
    extract_content(tree, cleaned, ctx)
}

/// The stage 4 retry: stages 1 and 2 again on `esc_root`, with no comment capture, no baseline and
/// no escalation.
// upstream: core.py::_recall_retry
fn recall_retry(
    tree: &mut Tree,
    esc_root: NodeId,
    ctx: &Ctx<'_>,
    url: Option<&str>,
) -> Result<(NodeId, String), ExtractError> {
    let (cleaned, _backup) = prepare_tree(tree, esc_root, ctx, url)?;
    extract_and_compare(tree, cleaned, ctx)
}

/// Stage 4 from the escalation input to the acceptance rule: runs `retry` with the focus set to
/// recall on the raw tree (a thread forum, or comments off) or on a copy pruned of the comment
/// containers, and returns its result when it is at least `min_extracted_size` characters and more
/// than `ESCALATION_ACCEPT_RATIO` times the current text. A rejection inside `retry` leaves the
/// first pass standing (`None`); a cancellation propagates.
// upstream: core.py::trafilatura_sequence (stage 4)
#[allow(clippy::too_many_arguments)] // the stage's inputs, as upstream's locals
fn escalate(
    tree: &mut Tree,
    raw_root: NodeId,
    ctx: &Ctx<'_>,
    url: Option<&str>,
    is_forum: bool,
    temp_comments: &str,
    (_, current_text): (NodeId, String),
    retry: impl FnOnce(
        &mut Tree,
        NodeId,
        &Ctx<'_>,
        Option<&str>,
    ) -> Result<(NodeId, String), ExtractError>,
) -> Result<Option<(NodeId, String)>, ExtractError> {
    let call = ctx.call;
    // a copy, so the caller's options keep their focus
    let mut r_options = (*ctx.options).clone();
    r_options.focus = Focus::Recall;
    let r_ctx = Ctx {
        options: Cow::Owned(r_options),
        call,
    };
    let esc_root = if is_forum || !ctx.options.comments {
        raw_root
    } else {
        let copy = tree.copy(raw_root);
        let comment_nodes = if temp_comments.is_empty() {
            REMOVE_COMMENTS_XPATH
        } else {
            REMOVE_COMMENTS_AND_LISTS_XPATH
        };
        prune_unwanted_nodes(tree, copy, comment_nodes, false, call)?;
        copy
    };
    // upstream's `except Exception` leaves `r_text` empty, which fails the floor
    let Some((r_body, r_text)) = caught(retry(tree, esc_root, &r_ctx, url))? else {
        return Ok(None);
    };
    let len_text = current_text.chars().count();
    let r_len = r_text.chars().count();
    // deviation: D-1 (the jusText candidate, `ESCALATION_JUSTEXT_RATIO`, is never tried)
    let accepted = r_len >= ctx.options.min_extracted_size
        && r_len as f64 > ESCALATION_ACCEPT_RATIO * len_text as f64;
    Ok(accepted.then_some((r_body, r_text)))
}

/// The thread-forum salvage: appends the captured posts whose text is not in the body to `postbody`
/// and returns the recomputed text, `None` when no post was appended. Checks `call` for every
/// child of the body and every post, as both scans are quadratic.
// upstream: core.py::trafilatura_sequence (forum salvage)
fn salvage(
    tree: &mut Tree,
    postbody: NodeId,
    forum_posts: NodeId,
    call: &Call<'_>,
) -> Result<Option<String>, ExtractError> {
    let mut texts = Vec::new();
    for child in tree.children(postbody).collect::<Vec<_>>() {
        call.check()?;
        let text = elem_text(tree, child);
        if !text.is_empty() {
            texts.push(text);
        }
    }
    let existing = texts.join("\n");
    let mut salvaged = Vec::new();
    for post in tree.children(forum_posts).collect::<Vec<_>>() {
        call.check()?;
        let text = elem_text(tree, post);
        if !text.is_empty() && !existing.contains(&text) {
            salvaged.push(post);
        }
    }
    if salvaged.is_empty() {
        return Ok(None);
    }
    for post in salvaged {
        tree.append(postbody, post);
    }
    let pieces: Vec<&str> = tree.itertext_lxml(postbody).collect();
    Ok(Some(pystr::strip(&pieces.join(" ")).to_owned()))
}

/// Prepares the raw tree below `root` (cleaning, tag conversion, comment handling), then runs the
/// cascade of extractors, each stage engaging only when the previous one under-delivered: the main
/// extractor, the baseline rescue on the raw tree, the recall escalation, and the forum-post
/// salvage. Returns the body and the comments body with their texts. `root` is pruned in place; the
/// stages work on copies.
// upstream: core.py::trafilatura_sequence
pub fn trafilatura_sequence(
    tree: &mut Tree,
    root: NodeId,
    ctx: &Ctx<'_>,
    url: Option<&str>,
    limits: &ParseLimits,
) -> Result<(NodeId, String, NodeId, String), ExtractError> {
    let call = ctx.call;
    let options = &ctx.options;
    call.check()?;
    let is_forum = forum_thread_page(tree, root, call)?;
    // the raw-tree prune, so the baseline, html2txt and the retry inherit it
    prune_unwanted_nodes(tree, root, RAW_TREE_PRUNE_XPATH, false, call)?;
    if !options.comments && (options.focus == Focus::Precision || !is_forum) {
        prune_unwanted_nodes(tree, root, REMOVE_COMMENTS_AND_LISTS_XPATH, false, call)?;
    }
    call.check()?;
    let (mut cleaned, backup) = prepare_tree(tree, root, ctx, url)?;
    call.check()?;

    let (mut comments_body, mut temp_comments) = if options.comments {
        extract_comments(tree, cleaned, ctx)?
    } else {
        (tree.new_element("body"), String::new())
    };
    let mut forum_posts = None;
    if !temp_comments.is_empty() && is_forum {
        // thread forum: the comments are the posts, so they go to the body (the backup predates the
        // capture) and the capture waits for the salvage
        forum_posts = Some(comments_body);
        comments_body = tree.new_element("body");
        temp_comments = String::new();
        cleaned = tree.copy(backup);
        convert_tags(tree, cleaned, ctx, url)?;
    }
    if options.focus == Focus::Precision && !is_forum {
        // after the conversion the lists are `list`, which the raw-tree prune could not match
        prune_unwanted_nodes(tree, cleaned, REMOVE_COMMENTS_XPATH, false, call)?;
    }
    call.check()?;

    let (mut postbody, mut temp_text) = extract_and_compare(tree, cleaned, ctx)?;
    call.check()?;

    // 3. baseline rescue on the raw tree, accepted only when it adds text
    if temp_text.chars().count() < options.min_extracted_size && options.focus != Focus::Precision {
        let (b_body, b_text, b_len) = baseline_tree(tree, root, limits, call)?;
        if b_len > temp_text.chars().count() {
            (postbody, temp_text) = (b_body, b_text);
            // the dump saw the whole page: the missing posts are boilerplate, not lost
            forum_posts = None;
        }
        call.check()?;
    }

    // 4. recall escalation: a short extraction covering little of the page is retried in recall mode
    let len_text = temp_text.chars().count();
    if options.focus == Focus::Balanced && 0 < len_text && len_text < ESCALATION_MAX_LENGTH {
        // the page measure is only taken when the two cheaper tests hold
        let page_len = html2txt_tree(tree, root, true, call)?.chars().count();
        if (len_text as f64) < ESCALATION_PAGE_SHARE * page_len as f64 {
            let current = (postbody, temp_text.clone());
            if let Some(retried) = escalate(
                tree,
                root,
                ctx,
                url,
                is_forum,
                &temp_comments,
                current,
                recall_retry,
            )? {
                (postbody, temp_text) = retried;
                // an accepted candidate saw the full page: its exclusions are deliberate
                forum_posts = None;
            }
        }
        call.check()?;
    }

    if let Some(posts) = forum_posts
        && let Some(text) = salvage(tree, postbody, posts, call)?
    {
        temp_text = text;
    }
    Ok((postbody, temp_text, comments_body, temp_comments))
}

/// What an extraction reads, upstream's `filecontent`.
// upstream: core.py::bare_extraction (filecontent)
#[derive(Clone, Copy, Debug)]
pub enum Source<'a> {
    /// An HTML page, as bytes or text.
    Input(HtmlInput<'a>),
    /// An element of a caller's tree. Its subtree is copied into the call's own arena, so the
    /// caller's tree stays as it is. The subtree must be at most `MAX_DEPTH` deep, as the parser
    /// guarantees, or the caller must provide the stack the recursive output walks need.
    Tree(&'a Tree, NodeId),
}

/// Upstream's tree size check on the extracted body: more than `max_tree_size` children are first
/// cut by dissolving the `hi` elements, then rejected when still too many. `None` and `Some(0)`
/// skip the check, as upstream's `if options.max_tree_size:` does.
// upstream: core.py::bare_extraction (tree size sanity check)
fn check_tree_size(
    tree: &mut Tree,
    postbody: NodeId,
    max_tree_size: Option<usize>,
) -> Result<(), ExtractError> {
    let Some(max) = max_tree_size.filter(|&max| max != 0) else {
        return Ok(());
    };
    if tree.child_count(postbody) <= max {
        return Ok(());
    }
    tree.strip_tags(postbody, &["hi"]);
    if tree.child_count(postbody) > max {
        return Err(ExtractError::Rejected(Rejection::TreeTooLarge));
    }
    Ok(())
}

/// Extracts the main text and the comments of `source` into a [`Document`], rejecting what upstream
/// discards: an empty tree, a wrong `html` language, a blacklisted URL or missing metadata, a tree
/// over `max_tree_size`, a text and comments both under the minimum sizes, a duplicate and a wrong
/// language. With the Python format the document carries `text` and `comments` as strings; with any
/// other it keeps only `raw_text`, and the comments body always.
// upstream: core.py::bare_extraction
pub fn bare_extraction(
    source: Source<'_>,
    ctx: &Ctx<'_>,
    limits: &ParseLimits,
) -> Result<Document, ExtractError> {
    let call = ctx.call;
    let options = &ctx.options;
    let (mut tree, root) = match source {
        Source::Input(input) => {
            let tree = load_html(input, options.max_file_size, limits, call)?;
            let root = tree.root();
            (tree, root)
        }
        Source::Tree(caller, element) => {
            // the cascade prunes and edits the tree it gets
            let mut tree = Tree::new(LocalName::from("html"), Ns::Html, Vec::new());
            let root = caller.copy_into(element, &mut tree);
            (tree, root)
        }
    };
    call.check()?;

    let lang = options.lang.as_deref().filter(|lang| !lang.is_empty());
    // deviation: D-1 (`options.fast or not LANGID_FLAG`: the check runs in fast mode)
    if let Some(lang) = lang
        && !check_html_lang(&tree, root, lang, false)
    {
        return Err(ExtractError::Rejected(Rejection::WrongHtmlLang));
    }

    let mut document = if options.with_metadata {
        // todo(phase 4): metadata
        // deviation: D-1 (upstream passes `options.fast` as `extensive`)
        let document = extract_metadata(
            &tree,
            root,
            options.url.as_deref(),
            &options.date_params,
            true,
            &options.author_blacklist,
        );
        if document
            .url
            .as_ref()
            .is_some_and(|url| options.url_blacklist.contains(url))
        {
            return Err(ExtractError::Rejected(Rejection::BlacklistedUrl));
        }
        if options.only_with_metadata
            && !["date", "title", "url"]
                .iter()
                .all(|&name| document.field(name).is_some())
        {
            return Err(ExtractError::Rejected(Rejection::MissingMetadata));
        }
        document
    } else {
        Document::new()
    };
    call.check()?;

    // `options.url or document.url`: an empty string is no URL
    let url = options
        .url
        .as_deref()
        .filter(|url| !url.is_empty())
        .or(document.url.as_deref().filter(|url| !url.is_empty()));
    let (postbody, temp_text, comments_body, temp_comments) =
        trafilatura_sequence(&mut tree, root, ctx, url, limits)?;
    call.check()?;

    check_tree_size(&mut tree, postbody, options.max_tree_size)?;
    if temp_text.chars().count() < options.min_output_size
        && temp_comments.chars().count() < options.min_output_comm_size
    {
        return Err(ExtractError::Rejected(Rejection::TooShort));
    }
    if options.dedup && duplicate_test(&tree, postbody, ctx) {
        return Err(ExtractError::Rejected(Rejection::Duplicate));
    }
    // todo(phase 4): language (the detected language goes into `document.language`)
    if let Some(lang) = lang
        && language_filter(&temp_text, &temp_comments, Some(lang))
    {
        return Err(ExtractError::Rejected(Rejection::WrongLanguage));
    }
    call.check()?;

    // outside upstream's `try`: an error here escapes
    let keeps_comments_body = options.format != Format::Python || options.comments;
    if options.format == Format::Python {
        let text = xml_to_text(&mut tree, Some(postbody), options.formatting, call)?;
        if options.comments {
            document.comments = Some(xml_to_text(
                &mut tree,
                Some(comments_body),
                options.formatting,
                call,
            )?);
        }
        document.raw_text = Some(text.clone());
        document.text = Some(text);
    } else {
        document.raw_text = Some(temp_text);
    }
    // the document owns just the two bodies, so the call's arena goes away with `tree`
    document.body = tree.copy_into(postbody, &mut document.tree);
    if keeps_comments_body {
        document.comments_body = tree.copy_into(comments_body, &mut document.tree);
    }
    Ok(document)
}

/// Extracts `source` and renders it in the format of `options` into `Document::text`; `record_id`
/// goes to `Document::id` and a content fingerprint to `Document::fingerprint` for the formats
/// that carry them. Fails with `Rejection::InvalidValue` after a successful extraction for the
/// Python format, which only `bare_extraction` serves.
// upstream: core.py::_internal_extraction
pub fn internal_extraction(
    source: Source<'_>,
    record_id: Option<&str>,
    ctx: &Ctx<'_>,
    limits: &ParseLimits,
) -> Result<Document, ExtractError> {
    let options = &ctx.options;
    let mut document = bare_extraction(source, ctx, limits)?;
    // the TXT formats show the ID and the fingerprint only in their metadata header
    if !TXT_FORMATS.contains(&options.format) || options.with_metadata {
        if options.format == Format::Python {
            return Err(ExtractError::Rejected(Rejection::InvalidValue));
        }
        document.id = record_id.map(str::to_owned);
        if let Some(raw_text) = &document.raw_text {
            let title = document.title.as_deref().unwrap_or("None");
            // todo(phase 5): deduplication
            document.fingerprint = Some(content_fingerprint(&format!("{title} {raw_text}")));
        }
    }
    document.text = Some(determine_returnstring(&mut document, options, ctx.call)?);
    Ok(document)
}

#[cfg(test)]
mod tests;
