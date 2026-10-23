//! Input decoding, HTML loading and the text helpers, ported from upstream `utils.py`.

use std::borrow::Cow;
use std::sync::LazyLock;

use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{Encoding, UTF_8};
use regex::Regex;

use crate::cancel::{CHECK_INTERVAL, Call};
use crate::context::Ctx;
use crate::courlan::{fix_relative_urls, get_base_url};
use crate::dom::{NodeId, ParseLimits, Tree, parse_document};
use crate::error::{ExtractError, Rejection};
use crate::pystr::{self, PY_SPACE_CLASS, PY_WORD_CLASS, ignore_case_i};
use crate::settings::MAX_FILE_SIZE;

/// Input accepted by `load_html`.
// upstream: utils.py::HtmlInput
#[derive(Clone, Copy, Debug)]
pub enum HtmlInput<'a> {
    /// Raw bytes, possibly compressed, in an unknown encoding.
    Bytes(&'a [u8]),
    /// An already decoded string.
    Str(&'a str),
}

// The patterns are literals that compile (`patterns_compile` forces each one); `None` would make
// the step that uses it a no-op instead of a panic.
fn compile(pattern: &str) -> Option<Regex> {
    Regex::new(pattern).ok()
}

/// Matches a doctype that contains a slash, which libxml2 mishandles.
// upstream: utils.py::DOCTYPE_TAG
pub static DOCTYPE_TAG: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r"(?i)^< ?! ?DOCTYPE[^>]*/[^<>]*>"));

/// Matches a self-closing `<html .../>` tag.
// upstream: utils.py::FAULTY_HTML
pub static FAULTY_HTML: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"(?i)(<html.*?)\s*/>"));

/// Matches control characters that XML cannot hold.
// upstream: utils.py::INVALID_XML_CHARS
pub static INVALID_XML_CHARS: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r"[\x00-\x08\x0b\x0c\x0e-\x1f\x{fffe}\x{ffff}]"));

// lxml's `fromstring` keeps the whole document when the input starts like one.
static LOOKS_LIKE_FULL_HTML: LazyLock<Option<Regex>> =
    LazyLock::new(|| compile(r"(?i)^\s*<(?:html|!doctype)"));

/// Most members of a concatenated gzip or zstd stream; bgzip output needs about 320 for 20 MB.
// upstream: utils.py::MAX_MEMBERS
#[cfg(feature = "decompress")]
const MAX_MEMBERS: usize = 1000;

/// Largest zstd window the decoder accepts: 128 MiB, the reference decoder's default limit.
#[cfg(feature = "decompress")]
const ZSTD_MAX_WINDOW: u64 = 1 << 27;

/// Decompresses `content` by magic number, or by trying brotli then zlib, capped at `max_size`;
/// returns the input unchanged when it is not compressed, corrupt, truncated or too large.
// upstream: utils.py::handle_compressed_file
#[cfg(feature = "decompress")]
pub fn handle_compressed_file(content: &[u8], max_size: usize) -> Cow<'_, [u8]> {
    let unpacked = if content.starts_with(&[0x1f, 0x8b, 0x08]) {
        bounded_gzip_members(content, max_size)
    } else if content.starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        bounded_zstd_frames(content, max_size)
    } else {
        bounded_unbrotli(content, max_size).or_else(|| bounded_inflate(content, max_size))
    };
    unpacked.map_or(Cow::Borrowed(content), Cow::Owned)
}

/// Appends `source` to `out` up to its end; `None` when it errors or `out` exceeds `max_size`.
// upstream: utils.py::_capped
#[cfg(feature = "decompress")]
fn capped(source: impl std::io::Read, max_size: usize, out: &mut Vec<u8>) -> Option<()> {
    use std::io::Read;
    let room = u64::try_from(max_size.saturating_add(1).saturating_sub(out.len())).ok()?;
    source.take(room).read_to_end(out).ok()?;
    (out.len() <= max_size).then_some(())
}

/// What follows the NUL padding that gzip allows between members.
#[cfg(feature = "decompress")]
fn skip_nul_padding(rest: &[u8]) -> &[u8] {
    let padding = rest.iter().take_while(|&&byte| byte == 0).count();
    rest.get(padding..).unwrap_or_default()
}

// upstream: utils.py::_bounded_members (gzip)
#[cfg(feature = "decompress")]
fn bounded_gzip_members(content: &[u8], max_size: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut rest = content;
    for _ in 0..MAX_MEMBERS {
        let mut member = flate2::bufread::GzDecoder::new(rest);
        capped(&mut member, max_size, &mut out)?;
        rest = skip_nul_padding(member.into_inner());
        if rest.is_empty() {
            return Some(out);
        }
    }
    None
}

// upstream: utils.py::_bounded_members (zstd)
#[cfg(feature = "decompress")]
fn bounded_zstd_frames(content: &[u8], max_size: usize) -> Option<Vec<u8>> {
    use ruzstd::decoding::StreamingDecoder;
    let mut out = Vec::new();
    let mut rest = content;
    for _ in 0..MAX_MEMBERS {
        let mut frame = StreamingDecoder::new_with_max_window_size(rest, ZSTD_MAX_WINDOW).ok()?;
        capped(&mut frame, max_size, &mut out)?;
        // A frame cut short, or cut off by the cap, is not a whole document.
        frame.decoder.is_finished().then_some(())?;
        rest = skip_nul_padding(frame.into_inner());
        if rest.is_empty() {
            return Some(out);
        }
    }
    None
}

// upstream: utils.py::_bounded_inflate
#[cfg(feature = "decompress")]
fn bounded_inflate(content: &[u8], max_size: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    capped(
        flate2::bufread::ZlibDecoder::new(content),
        max_size,
        &mut out,
    )?;
    Some(out)
}

// upstream: utils.py::_bounded_unbrotli
// Trailing bytes after a valid brotli stream are read ahead and not noticed; upstream keeps the input.
#[cfg(feature = "decompress")]
fn bounded_unbrotli(content: &[u8], max_size: usize) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    capped(
        brotli_decompressor::Decompressor::new(content, 4096),
        max_size,
        &mut out,
    )?;
    // Input that is not brotli can decode to nothing without an error.
    (!out.is_empty()).then_some(out)
}

/// Candidate encodings for bytes that are not valid UTF-8, best first; none for mostly-UTF-8 text.
///
/// Inputs of 10 000 bytes or more are judged on their first and last 5000 bytes.
// upstream: utils.py::detect_encoding
pub fn detect_encoding(content: &[u8]) -> Vec<&'static Encoding> {
    if std::str::from_utf8(content).is_ok() {
        return vec![UTF_8];
    }
    if is_mostly_utf8(content) {
        return Vec::new();
    }
    let mut detector = EncodingDetector::new(Iso2022JpDetection::Deny);
    let head = content.get(..5000);
    let tail = content
        .len()
        .checked_sub(5000)
        .and_then(|from| content.get(from..));
    match (content.len() < 10_000, head, tail) {
        (false, Some(head), Some(tail)) => {
            detector.feed(head, false);
            detector.feed(tail, true);
        }
        _ => {
            detector.feed(content, true);
        }
    }
    // deviation: D-3 (one guess instead of every charset_normalizer match, and no retry on the whole
    // input when the sample matched nothing)
    vec![detector.guess(None, Utf8Detection::Deny)]
}

/// Whether the bytes are UTF-8 text with a few stray invalid sequences: more than ten valid
/// non-ASCII characters per invalid sequence.
// deviation: D-3 (a heuristic standing in for charset_normalizer's mojibake check, which often leaves
// such text without a candidate so upstream decodes UTF-8 with replacement; chardetng always guesses
// one, so at a ratio of ten or less the crate takes that single-byte guess)
fn is_mostly_utf8(content: &[u8]) -> bool {
    let (mut valid_non_ascii, mut invalid) = (0_usize, 0_usize);
    for chunk in content.utf8_chunks() {
        valid_non_ascii =
            valid_non_ascii.saturating_add(chunk.valid().chars().filter(|c| !c.is_ascii()).count());
        invalid = invalid.saturating_add(usize::from(!chunk.invalid().is_empty()));
    }
    valid_non_ascii > invalid.saturating_mul(10)
}

/// Decompresses `content` if needed and decodes it to text: strict UTF-8 first, then the guessed
/// encodings, then UTF-8 with U+FFFD for what is invalid. Borrows when no copy is needed.
// upstream: utils.py::decode_file
pub fn decode_file(content: &[u8], max_size: usize) -> Cow<'_, str> {
    // deviation: D-4 (without the `decompress` feature the bytes are used as they are)
    #[cfg(feature = "decompress")]
    let content = handle_compressed_file(content, max_size);
    #[cfg(not(feature = "decompress"))]
    let content = {
        let _ = max_size;
        Cow::Borrowed(content)
    };
    // Fast path: valid UTF-8, a BOM kept as U+FEFF.
    let content = match content {
        Cow::Borrowed(bytes) => match std::str::from_utf8(bytes) {
            Ok(text) => return Cow::Borrowed(text),
            Err(_) => Cow::Borrowed(bytes),
        },
        Cow::Owned(bytes) => match String::from_utf8(bytes) {
            Ok(text) => return Cow::Owned(text),
            Err(err) => Cow::Owned(err.into_bytes()),
        },
    };
    decode_with(&content, &detect_encoding(&content))
}

/// Decodes `content` with the first of `encodings` that does it without loss, else as UTF-8 with
/// U+FFFD for what is invalid.
fn decode_with(content: &[u8], encodings: &[&'static Encoding]) -> Cow<'static, str> {
    for encoding in encodings {
        if let Some(text) = encoding.decode_without_bom_handling_and_without_replacement(content) {
            return Cow::Owned(text.into_owned());
        }
    }
    // Destructive fallback if nothing else succeeded.
    Cow::Owned(String::from_utf8_lossy(content).into_owned())
}

/// Whether the start of the page, lowercased, has no `html` in it.
// upstream: utils.py::is_dubious_html
pub fn is_dubious_html(beginning: &str) -> bool {
    !beginning.contains("html")
}

/// Strips XML-illegal characters, a doctype libxml2 mishandles and a self-closing `<html/>`.
// upstream: utils.py::repair_faulty_html
pub fn repair_faulty_html<'a>(html: &'a str, beginning: &str) -> Cow<'a, str> {
    let mut html = match INVALID_XML_CHARS.as_ref() {
        Some(invalid) => invalid.replace_all(html, ""),
        None => Cow::Borrowed(html),
    };
    // libxml2/LXML issue: https://bugs.launchpad.net/lxml/+bug/1955915
    if beginning.contains("doctype") {
        let (first_line, rest) = html.split_once('\n').unwrap_or((&html, ""));
        let first_line = match DOCTYPE_TAG.as_ref() {
            Some(doctype) => doctype.replace(first_line, ""),
            None => Cow::Borrowed(first_line),
        };
        html = Cow::Owned(format!("{first_line}\n{rest}"));
    }
    // Self-closing <html/> in the first lines.
    let self_closing = leading_lines(&html, 4096, 4)
        .iter()
        .any(|line| line.contains("<html") && line.ends_with("/>"));
    if self_closing && let Some(faulty) = FAULTY_HTML.as_ref() {
        html = Cow::Owned(faulty.replacen(&html, 1, "${1}>").into_owned());
    }
    html
}

/// The first `max_lines` lines of the first `max_chars` characters of `text`, split as Python's
/// `str.splitlines` does (the other separators it knows are control characters, stripped above).
fn leading_lines(text: &str, max_chars: usize, max_lines: usize) -> Vec<&str> {
    let end = text
        .char_indices()
        .nth(max_chars)
        .map_or(text.len(), |(at, _)| at);
    let prefix = text.get(..end).unwrap_or(text);
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = prefix.char_indices().peekable();
    while let Some((at, ch)) = chars.next() {
        if !matches!(ch, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}') {
            continue;
        }
        lines.push(prefix.get(start..at).unwrap_or_default());
        start = at.saturating_add(ch.len_utf8());
        if ch == '\r' && chars.next_if(|&(_, next)| next == '\n').is_some() {
            start = start.saturating_add(1);
        }
        if lines.len() == max_lines {
            return lines;
        }
    }
    if start < prefix.len() {
        lines.push(prefix.get(start..).unwrap_or_default());
    }
    lines.truncate(max_lines);
    lines
}

/// Decodes, repairs and parses `input`; rejects with `Rejection::EmptyTree` what upstream
/// returns `None` for.
///
/// Expects a full document: a single-block fragment without `html` near its start is rejected,
/// as upstream does.
// upstream: utils.py::load_html
pub fn load_html(
    input: HtmlInput<'_>,
    limits: &ParseLimits,
    call: &Call<'_>,
) -> Result<Tree, ExtractError> {
    let html = match input {
        HtmlInput::Bytes(bytes) => decode_file(bytes, MAX_FILE_SIZE),
        HtmlInput::Str(text) => Cow::Borrowed(text),
    };
    let beginning = html.chars().take(50).collect::<String>().to_lowercase();
    let check_flag = is_dubious_html(&beginning);
    let html = repair_faulty_html(&html, &beginning);
    // lxml cannot parse an empty document and upstream keeps `None`.
    if html.trim().is_empty() {
        return Err(ExtractError::Rejected(Rejection::EmptyTree));
    }
    // One parse of the decoded string covers upstream's string pass and its bytes retry
    // (`fromstring_bytes`): the parser has no encoding declaration to refuse, so both give this tree.
    let tree = parse_document(&html, limits, call)?;
    let is_full_html = LOOKS_LIKE_FULL_HTML
        .as_ref()
        .is_some_and(|full| full.is_match(&html));
    if check_flag && lxml_len(&tree, is_full_html) < 2 {
        return Err(ExtractError::Rejected(Rejection::EmptyTree));
    }
    Ok(tree)
}

/// `len(tree)` of the element lxml's `fromstring` returns for this parse.
///
/// A full document is returned whole. For a fragment lxml returns the lone element of `body`
/// when there is no `head`, else the document. html5ever always adds an empty `head` while
/// libxml2 (`default_doctype=False`) makes one only for head content, so an empty `head` is not
/// counted. libxml2 also wraps leading body text in an implicit `p` that takes in the inline
/// elements after it, so text followed by a block element counts as one more child.
///
/// Known mismatches that come from the parser (D-2), not fixed here: `<head></head><div><p>x</p></div>`
/// (libxml2 keeps the explicit empty head and accepts; this rejects) and `<td>a</td><td>b</td>`
/// (html5ever drops the stray `td`).
// upstream: lxml/html/__init__.py::fromstring (the part `load_html` observes)
fn lxml_len(tree: &Tree, is_full_html: bool) -> usize {
    let root = tree.root();
    let child_named = |parent, name: &str| {
        tree.children(parent)
            .find(|&child| tree.tag(child) == Some(name))
    };
    let has_head =
        child_named(root, "head").is_some_and(|head| tree.children(head).next().is_some());
    let root_len = || {
        let empty_head = usize::from(!has_head && child_named(root, "head").is_some());
        tree.children(root).count().saturating_sub(empty_head)
    };
    if is_full_html || has_head {
        return root_len();
    }
    let Some(body) = child_named(root, "body") else {
        return root_len();
    };
    let blank = |text: &Option<String>| text.as_deref().is_none_or(|t| t.trim().is_empty());
    let mut children = tree.children(body);
    match (children.next(), children.next()) {
        (Some(only), None) if blank(&tree.node(body).text) && blank(&tree.node(only).tail) => {
            tree.children(only).count()
        }
        (Some(first), _)
            if !blank(&tree.node(body).text) && tree.tag(first).is_some_and(is_block_tag) =>
        {
            tree.children(body).count().saturating_add(1)
        }
        _ => tree.children(body).count(),
    }
}

/// Whether `name` makes libxml2 end the implied `p` that wraps leading body text: the entries of
/// `lxml.html.defs.block_tags` except `ins`, `del` and `noscript`, which libxml2 treats as inline
/// there, so `t <ins>a</ins>` keeps one child.
// upstream: lxml/html/defs.py::block_tags
fn is_block_tag(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "blockquote"
            | "center"
            | "div"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "hr"
            | "isindex"
            | "p"
            | "pre"
            | "dir"
            | "dl"
            | "dt"
            | "dd"
            | "li"
            | "menu"
            | "ol"
            | "ul"
            | "table"
            | "caption"
            | "colgroup"
            | "col"
            | "thead"
            | "tfoot"
            | "tbody"
            | "tr"
            | "td"
            | "th"
            | "fieldset"
            | "form"
            | "legend"
            | "optgroup"
            | "option"
    )
}

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
static RE_HTML_LANG: LazyLock<Option<Regex>> = LazyLock::new(|| compile(r"([a-z]{2})"));

/// Matches an image file name: a non-space run, a dot, an image extension, then a non-word
/// character or the end. `[^\s]` and `\b` take Python's `\s` and `\w`, which Rust's differ from.
// upstream: utils.py::IMAGE_EXTENSION
static IMAGE_EXTENSION: LazyLock<Option<Regex>> = LazyLock::new(|| {
    let extension = ignore_case_i("avif|bmp|gif|hei[cf]|jpe?g|png|webp");
    // `(\b|$)` after an extension (it ends in a word character) is "not followed by a word character".
    compile(&format!(
        r"(?i)(?-i:[^{PY_SPACE_CLASS}])+\.(?:{extension})(?:(?-i:[^{PY_WORD_CLASS}])|\z)"
    ))
});

/// Matches a line that is only a social media or share label; applied with `re.match`, so anchored
/// at the start. `\W` is Python's, outside `(?i)` where Python's categories ignore case.
// upstream: utils.py::RE_FILTER
static RE_FILTER: LazyLock<Option<Regex>> = LazyLock::new(|| {
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
    pystr::split_whitespace(string)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Removes HTML space entities, then incompatible Unicode and invalid XML characters, on line level;
/// `None` for a line that is only space.
// upstream: utils.py::line_processing
pub fn line_processing(line: &str, preserve_space: bool, trailing_space: bool) -> Option<String> {
    // spacing HTML entities: https://www.w3.org/MarkUp/html-spec/html-spec_13.html
    let replaced = line
        .replace("&#13;", "\r")
        .replace("&#10;", "\n")
        .replace("&nbsp;", "\u{a0}");
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
    let lines: Vec<String> = pystr::splitlines(text)
        .filter_map(|line| line_processing(line, preserve_space, false))
        .filter(|line| !line.is_empty())
        .collect();
    Some(lines.join("\n").replace('\u{2424}', ""))
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
    let value = value.to_lowercase();
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
// todo(phase 4): language
pub fn language_classifier(_temp_text: &str, _temp_comments: &str) -> Option<String> {
    None
}

/// Whether the text is in another language than the target; the detected language is not stored
/// until `Document` exists.
// upstream: utils.py::language_filter
// todo(phase 4): language
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

#[cfg(test)]
mod tests;
