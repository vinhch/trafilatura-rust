//! Input decoding and HTML loading, ported from upstream `utils.py`.

use std::borrow::Cow;
use std::sync::LazyLock;

use chardetng::{EncodingDetector, Iso2022JpDetection, Utf8Detection};
use encoding_rs::{Encoding, UTF_8};
use regex::Regex;

use crate::cancel::Call;
use crate::dom::{ParseLimits, Tree, parse_document};
use crate::error::{ExtractError, Rejection};
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

/// Whether the bytes are UTF-8 text with a few stray invalid sequences: at least ten times as many
/// valid non-ASCII characters as invalid sequences.
// deviation: D-3 (charset_normalizer rejects every single-byte decoding of such text as mojibake,
// so upstream has no candidate and decodes UTF-8 with replacement; chardetng always guesses one)
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
            .find(|&child| &*tree.node(child).name == name)
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
            if !blank(&tree.node(body).text) && is_block_tag(&tree.node(first).name) =>
        {
            tree.children(body).count().saturating_add(1)
        }
        _ => tree.children(body).count(),
    }
}

/// Whether `name` is in `lxml.html.defs.block_tags`.
// upstream: lxml/html/defs.py::block_tags
fn is_block_tag(name: &str) -> bool {
    matches!(
        name,
        "address"
            | "blockquote"
            | "center"
            | "del"
            | "div"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "hr"
            | "ins"
            | "isindex"
            | "noscript"
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

#[cfg(test)]
mod tests;
