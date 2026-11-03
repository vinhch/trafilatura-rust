//! The text helpers of upstream `utils.py`: trimming, line processing, image checks and Unicode normalization.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::Regex;

use crate::cancel::{CHECK_INTERVAL, Call};
use crate::context::Ctx;
use crate::courlan::{fix_relative_urls, get_base_url};
use crate::dom::{NodeId, Tree};
use crate::error::ExtractError;
use crate::pyjson::{Json, JsonRef, Kind};
use crate::pystr::{self, PY_SPACE_CLASS, PY_WORD_CLASS, ignore_case_i};
use crate::pyunicodedata::normalize_nfc;

use super::compile;

/// Tags whose text keeps its formatting spaces when it is sanitized.
// upstream: utils.py::FORMATTING_PROTECTED
pub const FORMATTING_PROTECTED: &[&str] =
    &["cell", "head", "hi", "item", "p", "quote", "ref", "td"];

/// Tags whose text keeps all its spacing when it is sanitized.
// upstream: utils.py::SPACING_PROTECTED
pub const SPACING_PROTECTED: &[&str] = &["code", "pre"];

/// Link text above this fraction of the total text makes a link farm.
// upstream: utils.py::LINK_FARM_RATIO
pub const LINK_FARM_RATIO: f64 = 0.9;

/// The meta attribute (name, value) pairs that state a page's language; upstream keeps them as the
/// XPath predicates `http-equiv="content-language"` and `property="og:locale"`.
/// <https://developer.mozilla.org/en-US/docs/Web/HTTP/Headers/Content-Language>
// upstream: utils.py::TARGET_LANG_ATTRS
pub const TARGET_LANG_ATTRS: [(&str, &str); 2] = [
    ("http-equiv", "content-language"),
    ("property", "og:locale"),
];

/// Splits a language attribute into its two-letter codes.
// upstream: utils.py::RE_HTML_LANG
pub(super) static RE_HTML_LANG: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"([a-z]{2})"));

/// Matches an image file name: a non-space run, a dot, an image extension, then a non-word
/// character or the end. `[^\s]` and `\b` take Python's `\s` and `\w`, which Rust's differ from.
// upstream: utils.py::IMAGE_EXTENSION
pub(super) static IMAGE_EXTENSION: LazyLock<Option<Regex>> = LazyLock::new(|| {
    let extension = ignore_case_i("avif|bmp|gif|hei[cf]|jpe?g|png|webp");
    // `(\b|$)` after an extension (it ends in a word character) is "not followed by a word character".
    compile(&format!(
        r"(?i)(?-i:[^{PY_SPACE_CLASS}])+\.(?:{extension})(?:(?-i:[^{PY_WORD_CLASS}])|\z)"
    ))
});

/// Matches a line that is only a social media or share label; applied with `re.match`, so anchored
/// at the start. `\W` is Python's, outside `(?i)` where Python's categories ignore case.
// upstream: utils.py::RE_FILTER
pub(super) static RE_FILTER: LazyLock<Option<Regex>> = LazyLock::new(|| {
    let words = ignore_case_i(
        "Drucken|E-?Mail|Facebook|Flipboard|Google|Instagram|\
         Linkedin|Mail|PDF|Pinterest|Pocket|Print|QQ|Reddit|Twitter|\
         WeChat|WeiBo|Whatsapp|Xing|Mehr zum Thema:?|More on this.{0,8}$",
    );
    compile(&format!(r"(?i)\A(?-i:[^{PY_WORD_CLASS}])*(?:{words})$"))
});

/// Characters before which a newline is kept by `LINES_TRIMMING`.
///
/// Upstream's class is `[p{P}>]`: Python `re` has no `\p{P}`, so it holds the literal characters
/// `p`, `{`, `P`, `}` and `>`, not punctuation. Ported as it behaves.
// upstream: utils.py::LINES_TRIMMING
const LINES_TRIMMING_KEEPS_NEWLINE_AFTER: [char; 5] = ['p', '{', 'P', '}', '>'];

/// Longest image source, in characters, that is looked at.
const MAX_IMAGE_SRC_LEN: usize = 8192;

/// The character if it is printable or a space, else nothing.
// upstream: utils.py::return_printables_and_spaces
pub fn return_printables_and_spaces(c: char) -> Option<char> {
    (pystr::is_printable(c) || pystr::is_space(c)).then_some(c)
}

/// Drops non-printable and XML-invalid characters; borrows when there is none.
// upstream: utils.py::remove_control_characters
pub fn remove_control_characters(string: &str) -> Cow<'_, str> {
    // in case most strings are already clean
    if pystr::is_all_printable(string) {
        return Cow::Borrowed(string);
    }
    Cow::Owned(
        string
            .chars()
            .filter_map(return_printables_and_spaces)
            .collect(),
    )
}

/// `LINES_TRIMMING.sub(" ", text)`: each newline not preceded by a character of
/// `LINES_TRIMMING_KEEPS_NEWLINE_AFTER` becomes a space.
fn trim_lines(text: &str) -> String {
    let mut previous = None;
    text.chars()
        .map(|c| {
            let keep = c != '\n'
                || previous
                    .is_some_and(|before| LINES_TRIMMING_KEEPS_NEWLINE_AFTER.contains(&before));
            previous = Some(c);
            if keep { c } else { ' ' }
        })
        .collect()
}

/// Removes the unnecessary spaces within a text: every run of whitespace becomes one space.
// upstream: utils.py::trim
pub fn trim(string: &str) -> String {
    // The result is never longer than the input, so one allocation covers it.
    let mut trimmed = String::with_capacity(string.len());
    for (index, part) in pystr::split_whitespace(string).enumerate() {
        if index > 0 {
            trimmed.push(' ');
        }
        trimmed.push_str(part);
    }
    trimmed
}

/// Removes HTML space entities, then incompatible Unicode and invalid XML characters, on line level;
/// `None` for a line that is only space.
// upstream: utils.py::line_processing
pub fn line_processing(line: &str, preserve_space: bool, trailing_space: bool) -> Option<String> {
    // spacing HTML entities: https://www.w3.org/MarkUp/html-spec/html-spec_13.html
    // every needle starts with `&`, so a line without one is unchanged by the replacements
    let replaced = if line.contains('&') {
        Cow::Owned(
            line.replace("&#13;", "\r")
                .replace("&#10;", "\n")
                .replace("&nbsp;", "\u{a0}"),
        )
    } else {
        Cow::Borrowed(line)
    };
    let cleaned = remove_control_characters(&replaced);
    if preserve_space {
        return Some(cleaned.into_owned());
    }
    // remove newlines that are not related to punctuation or markup
    let new_line = trim(&trim_lines(&cleaned));
    // prune empty lines
    if new_line.chars().all(pystr::is_space) {
        return None;
    }
    if !trailing_space {
        return Some(new_line);
    }
    let space_before = if line.chars().next().is_some_and(pystr::is_space) {
        " "
    } else {
        ""
    };
    let space_after = if line.chars().next_back().is_some_and(pystr::is_space) {
        " "
    } else {
        ""
    };
    Some(format!("{space_before}{new_line}{space_after}"))
}

/// Converts text and discards incompatible and invalid characters; with `trailing_space` the text is
/// one line and `None` when it is only space.
// upstream: utils.py::sanitize
pub fn sanitize(text: &str, preserve_space: bool, trailing_space: bool) -> Option<String> {
    // consider all text as a single line
    if trailing_space {
        return line_processing(text, preserve_space, true);
    }
    // process line by line
    let mut sanitized = String::with_capacity(text.len());
    let kept_lines = pystr::splitlines(text)
        .filter_map(|line| line_processing(line, preserve_space, false))
        .filter(|line| !line.is_empty());
    for (index, line) in kept_lines.enumerate() {
        if index > 0 {
            sanitized.push('\n');
        }
        // removing U+2424 per line, after the empty-line filter, equals upstream's removal over the
        // joined text, because the separator is not U+2424
        sanitized.extend(line.chars().filter(|&c| c != '\u{2424}'));
    }
    Some(sanitized)
}

/// Trims spaces, removes control characters and drops attributes whose name has a colon, for `root`
/// and everything below it.
///
/// The walk collects the nodes first: it changes text, tail and attributes, never the structure.
// upstream: utils.py::sanitize_tree
pub fn sanitize_tree(tree: &mut Tree, root: NodeId, call: &Call<'_>) -> Result<(), ExtractError> {
    let nodes: Vec<NodeId> = tree.iter(root, &[]).collect();
    for (index, elem) in nodes.into_iter().enumerate() {
        if index.is_multiple_of(CHECK_INTERVAL) {
            call.check()?;
        }
        let node = tree.node(elem);
        let parent_tag = node
            .parent()
            .and_then(|parent| tree.tag(parent))
            .unwrap_or("");
        let tag = tree.tag(elem).unwrap_or("");
        // preserve space if the element or its parent is a specific tag
        let preserve_space =
            SPACING_PROTECTED.contains(&tag) || SPACING_PROTECTED.contains(&parent_tag);
        let trailing_space = FORMATTING_PROTECTED.contains(&tag)
            || FORMATTING_PROTECTED.contains(&parent_tag)
            || preserve_space;

        let node = tree.node_mut(elem);
        // A colon is reserved for namespaces in XML; the attribute stays only when its prefix is
        // declared and its value is not empty.
        // shortcut: the tree declares no namespaces (lxml's `nsmap` is empty for a parsed page), so
        // every attribute with a colon goes; revisit when XML-TEI trees with declarations are built.
        node.attrs.retain(|(name, _)| !name.contains(':'));
        let text = node
            .text
            .as_deref()
            .filter(|text| !text.is_empty())
            .map(|text| sanitize(text, preserve_space, trailing_space));
        if let Some(text) = text {
            node.text = text;
        }
        let tail = node
            .tail
            .as_deref()
            .filter(|tail| !tail.is_empty())
            .map(|tail| sanitize(tail, preserve_space, trailing_space));
        if let Some(tail) = tail {
            node.tail = tail;
        }
    }
    Ok(())
}

/// The source of an image element: `data-src`, `src`, or the first `data-src*` attribute that
/// points to an image file.
// upstream: utils.py::image_src
pub fn image_src(tree: &Tree, element: NodeId) -> Option<&str> {
    for attr in ["data-src", "src"] {
        let src = tree.get(element, attr).unwrap_or("");
        if is_image_file(Some(src)) {
            return Some(src);
        }
    }
    tree.node(element)
        .attrs
        .iter()
        .find(|(name, value)| name.starts_with("data-src") && is_image_file(Some(value)))
        .map(|(_, value)| &**value)
}

/// Whether the string ends in a valid image extension, up to a length threshold.
// upstream: utils.py::is_image_file
pub fn is_image_file(image_src: Option<&str>) -> bool {
    let Some(image_src) = image_src else {
        return false;
    };
    if image_src.chars().nth(MAX_IMAGE_SRC_LEN).is_some() {
        return false;
    }
    IMAGE_EXTENSION
        .as_ref()
        .is_some_and(|extension| extension.is_match(image_src))
}

/// Whether the document length is within the acceptable boundaries.
// upstream: utils.py::is_acceptable_length
pub fn is_acceptable_length(my_len: usize, ctx: &Ctx<'_>) -> bool {
    (ctx.options.min_file_size..=ctx.options.max_file_size).contains(&my_len)
}

/// Whether `target` is one of the pieces `re.split(r"([a-z]{2})", value.lower())` returns: the
/// two-letter codes and what lies between them.
fn lang_attr_matches(value: &str, target: &str) -> bool {
    let value = pystr::lower(value);
    let Some(codes) = RE_HTML_LANG.as_ref() else {
        return false;
    };
    let mut piece_start = 0;
    for code in codes.find_iter(&value) {
        if code.as_str() == target || value.get(piece_start..code.start()) == Some(target) {
            return true;
        }
        piece_start = code.end();
    }
    value.get(piece_start..) == Some(target)
}

/// Checks the meta elements for language information, and the `html` element's `lang` when
/// `strict`; the pages that state nothing pass.
// upstream: utils.py::check_html_lang
pub fn check_html_lang(tree: &Tree, root: NodeId, target_language: &str, strict: bool) -> bool {
    for (attr, value) in TARGET_LANG_ATTRS {
        // `.//meta[@attr="value"][@content]`
        let contents: Vec<&str> = tree
            .iterdescendants(root, &["meta"])
            .filter(|&meta| tree.get(meta, attr) == Some(value))
            .filter_map(|meta| tree.get(meta, "content"))
            .collect();
        if !contents.is_empty() {
            return contents
                .iter()
                .any(|content| lang_attr_matches(content, target_language));
        }
    }
    // HTML lang attribute: sometimes a wrong indication
    if strict {
        // `//html[@lang]` starts at the document root
        let top = tree.iterancestors(root, &[]).last().unwrap_or(root);
        let langs: Vec<&str> = tree
            .iter(top, &["html"])
            .filter_map(|html| tree.get(html, "lang"))
            .collect();
        if !langs.is_empty() {
            return langs
                .iter()
                .any(|lang| lang_attr_matches(lang, target_language));
        }
    }
    true
}

/// Runs the language classifier on the text or the comments, whichever is longer.
// upstream: utils.py::language_classifier
// todo(OI-4): language
pub fn language_classifier(_temp_text: &str, _temp_comments: &str) -> Option<String> {
    None
}

/// Whether the text is in another language than the target; the detected language is not written
/// to the document until OI-4 is decided.
// upstream: utils.py::language_filter
// todo(OI-4): language
pub fn language_filter(
    _temp_text: &str,
    _temp_comments: &str,
    _target_language: Option<&str>,
) -> bool {
    false
}

/// Whether the element's text, or its tail when it has no text, is empty, space only or a social
/// media label that is not content.
// upstream: utils.py::textfilter
pub fn textfilter(tree: &Tree, element: NodeId) -> bool {
    let node = tree.node(element);
    let testtext = if node.text.is_none() {
        node.tail.as_deref()
    } else {
        node.text.as_deref()
    };
    let Some(testtext) = testtext else {
        return true;
    };
    testtext.is_empty()
        || pystr::is_all_space(testtext)
        || pystr::splitlines(testtext).any(|line| {
            RE_FILTER
                .as_ref()
                .is_some_and(|filter| filter.is_match(line))
        })
}

/// Whether the string is not empty and not only spaces and control characters.
// upstream: utils.py::text_chars_test
pub fn text_chars_test(string: Option<&str>) -> bool {
    string.is_some_and(|string| !string.is_empty() && !pystr::is_all_space(string))
}

/// The scheme and host of `url`, an empty string where the URL is malformed.
// upstream: utils.py::safe_base_url
pub fn safe_base_url(url: &str) -> String {
    get_base_url(url).unwrap_or_default()
}

/// `url` resolved against `baseurl`, an empty string where either is malformed.
// upstream: utils.py::safe_relative_url
pub fn safe_relative_url(baseurl: &str, url: &str) -> String {
    fix_relative_urls(baseurl, url).unwrap_or_default()
}

/// The elements of a JSON value as a list: none for an absent value or `null`, a list's own items,
/// otherwise the value alone.
// upstream: utils.py::as_list
pub fn as_list(json: &Json, value: Option<JsonRef>) -> Vec<JsonRef> {
    match value {
        Some(r) if json.kind(r) == Kind::List => json.items(r).collect(),
        Some(r) if json.kind(r) != Kind::Null => vec![r],
        _ => Vec::new(),
    }
}

/// `s` in Unicode normal form NFC, the only form upstream passes; borrowed when already normalized.
// upstream: utils.py::normalize_unicode
pub fn normalize_unicode(s: &str) -> Cow<'_, str> {
    normalize_nfc(s)
}
