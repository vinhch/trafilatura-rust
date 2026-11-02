//! Markdown text helpers of upstream `xml.py`: code spans and fences, wrapping, the escaping of
//! inline and block syntax, the math delimiters and links.
//!
//! The patterns of upstream that use a lookbehind or a backreference, or that rely on Python's `$`,
//! `\d` and `\S`, are hand-written scanners over code points. Each makes its lookbehind test at the
//! candidate position, as the regex engine does, and keeps a linear total cost.

use std::borrow::Cow;
use std::collections::HashMap;

use crate::pystr::{decimal_value, is_space, strip};

/// The characters `escape_inline_specials` escapes, in upstream's pass order.
// upstream: xml.py::_INLINE_SPECIAL_CHARS
pub(crate) const INLINE_SPECIAL_CHARS: [char; 7] = ['*', '_', '`', '~', '[', ']', '<'];

/// The shortest backtick string of at least `min_len` that does not appear as a run in `text`.
// upstream: xml.py::_code_fence
pub(crate) fn code_fence(text: &str, min_len: usize) -> String {
    let mut fence_len = min_len;
    let mut run = 0usize;
    for ch in text.chars() {
        if ch == '`' {
            run = run.saturating_add(1);
            if run >= fence_len {
                fence_len = run.saturating_add(1);
            }
        } else {
            run = 0;
        }
    }
    "`".repeat(fence_len)
}

/// An inline code span: a fence wider than any backtick run inside, with a space padding an edge
/// that abuts a backtick.
// upstream: xml.py::_code_span
pub(crate) fn code_span(text: &str) -> String {
    let fence = code_fence(text, 1);
    if text.starts_with('`') || text.ends_with('`') {
        // CommonMark: a space stops the edge backtick from merging with the fence
        return format!("{fence} {text} {fence}");
    }
    format!("{fence}{text}{fence}")
}

/// Wraps `text` in a Markdown marker, leaving the flanking whitespace outside it; `closing`
/// defaults to `opening`. Text of whitespace only is returned unchanged.
// upstream: xml.py::_md_wrap
pub(crate) fn md_wrap(text: &str, opening: &str, closing: Option<&str>) -> String {
    let stripped = strip(text);
    if stripped.is_empty() {
        return text.to_string();
    }
    let closing = closing.unwrap_or(opening);
    text.replacen(stripped, &format!("{opening}{stripped}{closing}"), 1)
}

/// Wraps `text` in an inline code span, leaving the flanking whitespace outside it.
// upstream: xml.py::_md_code
pub(crate) fn md_code(text: &str) -> String {
    let stripped = strip(text);
    if stripped.is_empty() {
        return text.to_string();
    }
    text.replacen(stripped, &code_span(stripped), 1)
}

/// The text between code points `start` and `end` of `chars`, empty when the range is invalid.
fn chars_between(chars: &[char], start: usize, end: usize) -> &[char] {
    chars.get(start..end).unwrap_or_default()
}

fn collect(chars: &[char]) -> String {
    chars.iter().collect()
}

/// The next index at or after a query position where `at` holds, for queries whose positions never
/// decrease: a scan result is reused until the position passes it, so the total work stays linear.
struct Forward {
    at: fn(&[char], usize) -> bool,
    /// The position of the last scan and what it found.
    memo: Option<(usize, Option<usize>)>,
}

impl Forward {
    fn new(at: fn(&[char], usize) -> bool) -> Self {
        Self { at, memo: None }
    }

    fn next(&mut self, chars: &[char], from: usize) -> Option<usize> {
        if let Some((asked, found)) = self.memo
            && from >= asked
        {
            match found {
                None => return None,
                Some(position) if position >= from => return Some(position),
                Some(_) => {}
            }
        }
        let found = (from..chars.len()).find(|&index| (self.at)(chars, index));
        self.memo = Some((from, found));
        found
    }
}

fn pair_at(chars: &[char], index: usize, first: char, second: char) -> bool {
    chars.get(index) == Some(&first) && chars.get(index.saturating_add(1)) == Some(&second)
}

fn is_close_bracket(chars: &[char], index: usize) -> bool {
    pair_at(chars, index, '\\', ']')
}

fn is_close_paren(chars: &[char], index: usize) -> bool {
    pair_at(chars, index, '\\', ')')
}

fn is_newline(chars: &[char], index: usize) -> bool {
    chars.get(index) == Some(&'\n')
}

/// Matches the math patterns at one position of a text.
struct MathScanner<'a> {
    chars: &'a [char],
    close_bracket: Forward,
    close_paren: Forward,
    newline: Forward,
}

impl<'a> MathScanner<'a> {
    fn new(chars: &'a [char]) -> Self {
        Self {
            chars,
            close_bracket: Forward::new(is_close_bracket),
            close_paren: Forward::new(is_close_paren),
            newline: Forward::new(is_newline),
        }
    }

    /// The end of a match of `(?<!\S)\\\[.+?\\\]` (DOTALL) at `index`.
    // upstream: xml.py::_MATH_BLOCK_RE
    fn block_end(&mut self, index: usize) -> Option<usize> {
        if !pair_at(self.chars, index, '\\', '[') {
            return None;
        }
        let after_space_or_start = match index.checked_sub(1) {
            None => true,
            Some(before) => self.chars.get(before).is_some_and(|&c| is_space(c)),
        };
        if !after_space_or_start {
            return None;
        }
        let close = self
            .close_bracket
            .next(self.chars, index.saturating_add(3))?;
        Some(close.saturating_add(2))
    }

    /// The end of a match of `\\\(.+?\\\)` (no DOTALL) at `index`.
    // upstream: xml.py::_MATH_INLINE_RE
    fn inline_end(&mut self, index: usize) -> Option<usize> {
        if !pair_at(self.chars, index, '\\', '(') {
            return None;
        }
        let close = self.close_paren.next(self.chars, index.saturating_add(3))?;
        // `.` stops at a newline, so one inside the content ends the attempt
        let newline = self.newline.next(self.chars, index.saturating_add(2));
        if newline.is_some_and(|newline| newline < close) {
            return None;
        }
        Some(close.saturating_add(2))
    }
}

/// `re.sub`: replaces each match of `span_end` with `rewrite` of the text between the two
/// delimiters of two code points each.
fn substitute(
    text: &str,
    span_end: impl Fn(&mut MathScanner<'_>, usize) -> Option<usize>,
    rewrite: fn(&str) -> String,
) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut scanner = MathScanner::new(&chars);
    let mut out = String::with_capacity(text.len());
    let (mut index, mut copied) = (0usize, 0usize);
    while index < chars.len() {
        let Some(end) = span_end(&mut scanner, index) else {
            index = index.saturating_add(1);
            continue;
        };
        out.push_str(&collect(chars_between(&chars, copied, index)));
        let inner = chars_between(&chars, index.saturating_add(2), end.saturating_sub(2));
        out.push_str(&rewrite(&collect(inner)));
        index = end;
        copied = end;
    }
    out.push_str(&collect(chars_between(&chars, copied, chars.len())));
    out
}

/// Converts LaTeX math delimiters to CommonMark `$` notation: block math first, then inline math
/// on the result of the first.
// upstream: xml.py::_convert_math
pub(crate) fn convert_math(text: &str) -> String {
    let blocks = substitute(
        text,
        |scan, at| scan.block_end(at),
        |inner| format!("\n$$\n{}\n$$\n", strip(inner)),
    );
    substitute(
        &blocks,
        |scan, at| scan.inline_end(at),
        |inner| format!("${inner}$"),
    )
}

/// Escapes literal CommonMark inline-syntax characters so they render as themselves.
///
/// Backslashes already in front of a special character are doubled before the escaping backslash
/// is added. One pass over the text gives the result of upstream's one substitution per character:
/// each of those inserts backslashes only before its own character, so no pass changes a run
/// another pass reads.
// upstream: xml.py::_escape_inline_specials
// upstream: xml.py::_INLINE_SPECIAL_RUN_RES
pub(crate) fn escape_inline_specials(text: &str) -> Cow<'_, str> {
    if !text.contains(INLINE_SPECIAL_CHARS) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len().saturating_add(text.len() / 4));
    let mut backslashes = 0usize;
    for ch in text.chars() {
        if ch == '\\' {
            backslashes = backslashes.saturating_add(1);
            continue;
        }
        let kept = if INLINE_SPECIAL_CHARS.contains(&ch) {
            backslashes.saturating_mul(2).saturating_add(1)
        } else {
            backslashes
        };
        out.extend(std::iter::repeat_n('\\', kept));
        out.push(ch);
        backslashes = 0;
    }
    out.extend(std::iter::repeat_n('\\', backslashes));
    Cow::Owned(out)
}

/// Escapes the prose and rewrites the formulas in one pass: the text is split on the math spans
/// (block or inline, the block pattern tried first at each position), the prose is escaped and the
/// spans are converted. Text without a `\[` or `\(` opener, the only two the scanners accept, has no
/// span and skips the scan.
// upstream: xml.py::_escape_and_convert_math
// upstream: xml.py::_MATH_SPAN_RE
pub(crate) fn escape_and_convert_math(text: &str) -> Cow<'_, str> {
    if !text.contains("\\[") && !text.contains("\\(") {
        return escape_inline_specials(text);
    }
    let chars: Vec<char> = text.chars().collect();
    let mut scanner = MathScanner::new(&chars);
    let mut out = String::with_capacity(text.len());
    let (mut index, mut prose_start) = (0usize, 0usize);
    while index < chars.len() {
        let end = scanner
            .block_end(index)
            .or_else(|| scanner.inline_end(index));
        let Some(end) = end else {
            index = index.saturating_add(1);
            continue;
        };
        let prose = collect(chars_between(&chars, prose_start, index));
        out.push_str(&escape_inline_specials(&prose));
        out.push_str(&convert_math(&collect(chars_between(&chars, index, end))));
        index = end;
        prose_start = end;
    }
    let prose = collect(chars_between(&chars, prose_start, chars.len()));
    out.push_str(&escape_inline_specials(&prose));
    Cow::Owned(out)
}

/// Whether the rest of the line after a block marker allows it: a space or tab, or the end of the
/// text, where Python's `$` also matches before a final newline.
fn ends_block_start(rest: &str) -> bool {
    rest.is_empty() || rest == "\n" || rest.starts_with([' ', '\t'])
}

/// The byte offset where `escape_block_start` puts its backslash: the marker group of the first of
/// the four block patterns that matches. The patterns exclude one another by their first
/// character, so the order does not matter here.
// upstream: xml.py::_BLOCK_START_RES
fn block_marker_offset(text: &str) -> Option<usize> {
    let indent = text.bytes().take_while(|&byte| byte == b' ').count();
    // ` {0,3}`: a fourth space is not a marker character, so no pattern matches
    if indent > 3 {
        return None;
    }
    let rest = text.get(indent..)?;
    match rest.chars().next()? {
        // _HEADING_START_RE
        '#' => {
            let hashes = rest.bytes().take_while(|&byte| byte == b'#').count();
            (hashes <= 6 && ends_block_start(rest.get(hashes..)?)).then_some(indent)
        }
        // _QUOTE_START_RE
        '>' => Some(indent),
        // _BULLET_START_RE
        '-' | '+' => ends_block_start(rest.get(1..)?).then_some(indent),
        // _ORDERED_START_RE: `\d` is any Nd digit
        first if decimal_value(first).is_some() => {
            let digits = || rest.chars().take_while(|&c| decimal_value(c).is_some());
            let digit_bytes: usize = digits().map(char::len_utf8).sum();
            let mut after = rest.get(digit_bytes..)?.chars();
            let marker = after.next();
            (digits().count() <= 9
                && matches!(marker, Some('.' | ')'))
                && ends_block_start(after.as_str()))
            .then_some(indent.saturating_add(digit_bytes))
        }
        _ => None,
    }
}

/// Escapes a leading heading, blockquote or list marker so a fresh paragraph or item line is not
/// read as block syntax.
// upstream: xml.py::_escape_block_start
pub(crate) fn escape_block_start(text: &str) -> Cow<'_, str> {
    match block_marker_offset(text).and_then(|offset| text.split_at_checked(offset)) {
        Some((head, tail)) => Cow::Owned(format!("{head}\\{tail}")),
        None => Cow::Borrowed(text),
    }
}

/// The maximal runs of backticks in `chars` as (start, length), and for each run the index of the
/// next run of the same length.
fn backtick_runs(chars: &[char]) -> (Vec<(usize, usize)>, Vec<Option<usize>>) {
    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        if chars.get(index) != Some(&'`') {
            index = index.saturating_add(1);
            continue;
        }
        let length = chars.iter().skip(index).take_while(|&&c| c == '`').count();
        runs.push((index, length));
        index = index.saturating_add(length);
    }
    let mut latest_of_length: HashMap<usize, usize> = HashMap::new();
    let mut next_same = vec![None; runs.len()];
    for (run, &(_, length)) in runs.iter().enumerate().rev() {
        if let Some(slot) = next_same.get_mut(run) {
            *slot = latest_of_length.insert(length, run);
        }
    }
    (runs, next_same)
}

/// Backslash-escapes each `[` and `]` that is not already escaped, outside code spans.
///
/// A code span opens at a backtick run that no backslash or backtick precedes and closes at the
/// next run of the same length, which a lazy `.+?` between them would find as the first run that
/// starts after a non-backtick and ends before one; its text stays as is.
// upstream: xml.py::_UNESCAPED_BRACKET_RE
fn escape_unescaped_brackets(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let (runs, next_same) = backtick_runs(&chars);
    let mut out = String::with_capacity(text.len());
    let (mut index, mut run) = (0usize, 0usize);
    while let Some(&ch) = chars.get(index) {
        if ch == '`' {
            // a run is always taken whole, so `index` is the start of `runs[run]`
            let Some(&(_, length)) = runs.get(run) else {
                // unreachable while `run` tracks `index`; keep the rest rather than cut the text
                out.push_str(&collect(chars_between(&chars, index, chars.len())));
                break;
            };
            let after_backslash = index
                .checked_sub(1)
                .is_some_and(|before| chars.get(before) == Some(&'\\'));
            let closing = if after_backslash {
                None
            } else {
                next_same.get(run).copied().flatten()
            };
            if let Some(&(start, closing_length)) = closing.and_then(|close| runs.get(close)) {
                let end = start.saturating_add(closing_length);
                out.push_str(&collect(chars_between(&chars, index, end)));
                index = end;
                run = closing.map_or(run, |close| close.saturating_add(1));
            } else {
                out.push_str(&collect(chars_between(
                    &chars,
                    index,
                    index.saturating_add(length),
                )));
                index = index.saturating_add(length);
                run = run.saturating_add(1);
            }
            continue;
        }
        let after_backslash = index
            .checked_sub(1)
            .is_some_and(|before| chars.get(before) == Some(&'\\'));
        if matches!(ch, '[' | ']') && !after_backslash {
            out.push('\\');
        }
        out.push(ch);
        index = index.saturating_add(1);
    }
    out
}

/// A Markdown link or image with escaped text and a CommonMark-safe target.
///
/// Only the brackets not already escaped are touched, so this is safe on pre-escaped and raw text.
/// Without a target the result is the bracketed text alone. A target holding a space, `<`, `>`,
/// `(` or `)` goes in angle brackets, with `\`, `<` and `>` backslash-escaped.
// upstream: xml.py::_md_link
pub fn md_link(text: &str, url: Option<&str>, image: bool) -> String {
    let escaped = escape_unescaped_brackets(text);
    let prefix = if image { "!" } else { "" };
    let Some(url) = url else {
        return format!("{prefix}[{escaped}]");
    };
    if !url.contains([' ', '<', '>', '(', ')']) {
        return format!("{prefix}[{escaped}]({url})");
    }
    let mut inner = String::with_capacity(url.len().saturating_add(2));
    for ch in url.chars() {
        if matches!(ch, '\\' | '<' | '>') {
            inner.push('\\');
        }
        inner.push(ch);
    }
    format!("{prefix}[{escaped}](<{inner}>)")
}

/// Escapes what would break a GFM table row when `in_cell`: pipes split columns, newlines split
/// rows.
// upstream: xml.py::_escape_cell
pub(crate) fn escape_cell(text: &str, in_cell: bool) -> Cow<'_, str> {
    if !in_cell || !text.contains(['|', '\n']) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len().saturating_add(1));
    for ch in text.chars() {
        match ch {
            '|' => out.push_str("\\|"),
            '\n' => out.push(' '),
            _ => out.push(ch),
        }
    }
    Cow::Owned(out)
}

#[cfg(test)]
mod tests;
