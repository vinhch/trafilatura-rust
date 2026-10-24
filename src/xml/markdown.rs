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
/// spans are converted.
// upstream: xml.py::_escape_and_convert_math
// upstream: xml.py::_MATH_SPAN_RE
pub(crate) fn escape_and_convert_math(text: &str) -> String {
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
    out
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
mod tests {
    use regex::Regex;

    use super::*;

    #[test]
    fn escape_block_start_matches_python() {
        let cases = [
            // oracle: xml._escape_block_start("#\n")
            ("#\n", "\\#\n"),
            // oracle: xml._escape_block_start("#\nx")
            ("#\nx", "#\nx"),
            // oracle: xml._escape_block_start("1.\n")
            ("1.\n", "1\\.\n"),
            // oracle: xml._escape_block_start("\u0661. x")
            ("\u{661}. x", "\u{661}\\. x"),
            // oracle: xml._escape_block_start("\u0661\n")
            ("\u{661}\n", "\u{661}\n"),
            // oracle: xml._escape_block_start("-\n")
            ("-\n", "\\-\n"),
            // oracle: xml._escape_block_start("   #  a")
            ("   #  a", "   \\#  a"),
            // oracle: xml._escape_block_start("    # a")
            ("    # a", "    # a"),
            // oracle: xml._escape_block_start("\uff11.")
            ("\u{ff11}.", "\u{ff11}\\."),
            // oracle: xml._escape_block_start("1234567890. x")
            ("1234567890. x", "1234567890. x"),
            // oracle: xml._escape_block_start("-\n\n")
            ("-\n\n", "-\n\n"),
            // oracle: xml._escape_block_start("## ")
            ("## ", "\\## "),
            // oracle: xml._escape_block_start("#######\n")
            ("#######\n", "#######\n"),
            // oracle: xml._escape_block_start("1.\n\n")
            ("1.\n\n", "1.\n\n"),
            // oracle: xml._escape_block_start("123456789) y")
            ("123456789) y", "123456789\\) y"),
            // oracle: xml._escape_block_start("a. x")
            ("a. x", "a. x"),
            // oracle: xml._escape_block_start(" > q")
            (" > q", " \\> q"),
            // oracle: xml._escape_block_start("+\n")
            ("+\n", "\\+\n"),
            // oracle: xml._escape_block_start("1) x")
            ("1) x", "1\\) x"),
            // oracle: xml._escape_block_start("\u0661\u0662. x")
            ("\u{661}\u{662}. x", "\u{661}\u{662}\\. x"),
            // oracle: xml._escape_block_start("#\t")
            ("#\t", "\\#\t"),
            // oracle: xml._escape_block_start("- a")
            ("- a", "\\- a"),
            // oracle: xml._escape_block_start("12345678. ")
            ("12345678. ", "12345678\\. "),
            // oracle: xml._escape_block_start("\u0966\u0967) \n")
            ("\u{966}\u{967}) \n", "\u{966}\u{967}\\) \n"),
        ];
        for (text, expected) in cases {
            assert_eq!(escape_block_start(text), expected, "{text:?}");
        }
    }

    #[test]
    fn math_spans_match_python() {
        // (input, escape_and_convert_math, convert_math)
        let cases = [
            // oracle: xml._escape_and_convert_math("x\\[a \\[b\\]"), xml._convert_math("x\\[a \\[b\\]")
            (
                "x\\[a \\[b\\]",
                "x\\\\\\[a \n$$\nb\n$$\n",
                "x\\[a \n$$\nb\n$$\n",
            ),
            // oracle: xml._escape_and_convert_math("\x1c\\[a\\]"), xml._convert_math("\x1c\\[a\\]")
            (
                "\u{1c}\\[a\\]",
                "\u{1c}\n$$\na\n$$\n",
                "\u{1c}\n$$\na\n$$\n",
            ),
            // oracle: xml._escape_and_convert_math("a\\(x\\)b\\(y"), xml._convert_math("a\\(x\\)b\\(y")
            ("a\\(x\\)b\\(y", "a$x$b\\(y", "a$x$b\\(y"),
            // oracle: xml._escape_and_convert_math("$a_1$ \\(a_1\\)"), xml._convert_math("$a_1$ \\(a_1\\)")
            ("$a_1$ \\(a_1\\)", "$a\\_1$ $a_1$", "$a_1$ $a_1$"),
            // oracle: xml._escape_and_convert_math("\\(a \\[b\\] c\\)"), xml._convert_math("\\(a \\[b\\] c\\)")
            (
                "\\(a \\[b\\] c\\)",
                "\\(a \n$$\nb\n$$\n c\\)",
                "\\(a \n$$\nb\n$$\n c\\)",
            ),
            // oracle: xml._escape_and_convert_math("\\[a\\]\\[b\\]"), xml._convert_math("\\[a\\]\\[b\\]")
            (
                "\\[a\\]\\[b\\]",
                "\n$$\na\n$$\n\\\\\\[b\\\\\\]",
                "\n$$\na\n$$\n\\[b\\]",
            ),
            // oracle: xml._escape_and_convert_math("  \\[ x \\]"), xml._convert_math("  \\[ x \\]")
            ("  \\[ x \\]", "  \n$$\nx\n$$\n", "  \n$$\nx\n$$\n"),
            // oracle: xml._escape_and_convert_math("\\[\\]]"), xml._convert_math("\\[\\]]")
            ("\\[\\]]", "\\\\\\[\\\\\\]\\]", "\\[\\]]"),
            // oracle: xml._escape_and_convert_math("\\(\nx\\) \\(y\\)"), xml._convert_math("\\(\nx\\) \\(y\\)")
            ("\\(\nx\\) \\(y\\)", "\\(\nx\\) $y$", "\\(\nx\\) $y$"),
            // oracle: xml._escape_and_convert_math("\\(a\nb\\) \\(c\\)"), xml._convert_math("\\(a\nb\\) \\(c\\)")
            ("\\(a\nb\\) \\(c\\)", "\\(a\nb\\) $c$", "\\(a\nb\\) $c$"),
            // oracle: xml._escape_and_convert_math("a_b \\[x_1\\] c*d"), xml._convert_math("a_b \\[x_1\\] c*d")
            (
                "a_b \\[x_1\\] c*d",
                "a\\_b \n$$\nx_1\n$$\n c\\*d",
                "a_b \n$$\nx_1\n$$\n c*d",
            ),
            // oracle: xml._escape_and_convert_math("\\[a\\"), xml._convert_math("\\[a\\")
            ("\\[a\\", "\\\\\\[a\\", "\\[a\\"),
            // oracle: xml._escape_and_convert_math("\\[\n a_b \n\\] z\\(q\\)"), xml._convert_math("\\[\n a_b \n\\] z\\(q\\)")
            (
                "\\[\n a_b \n\\] z\\(q\\)",
                "\n$$\na_b\n$$\n z$q$",
                "\n$$\na_b\n$$\n z$q$",
            ),
            // oracle: xml._escape_and_convert_math("\xa0\\[a\\]"), xml._convert_math("\xa0\\[a\\]")
            (
                "\u{a0}\\[a\\]",
                "\u{a0}\n$$\na\n$$\n",
                "\u{a0}\n$$\na\n$$\n",
            ),
            // oracle: xml._escape_and_convert_math("\\(\\)"), xml._convert_math("\\(\\)")
            ("\\(\\)", "\\(\\)", "\\(\\)"),
            // oracle: xml._escape_and_convert_math("\\(x\\) \\("), xml._convert_math("\\(x\\) \\(")
            ("\\(x\\) \\(", "$x$ \\(", "$x$ \\("),
            // oracle: xml._escape_and_convert_math("\\[x\\] \\(y\\) *z*"), xml._convert_math("\\[x\\] \\(y\\) *z*")
            (
                "\\[x\\] \\(y\\) *z*",
                "\n$$\nx\n$$\n $y$ \\*z\\*",
                "\n$$\nx\n$$\n $y$ *z*",
            ),
        ];
        for (text, escaped, converted) in cases {
            assert_eq!(escape_and_convert_math(text), escaped, "escape {text:?}");
            assert_eq!(convert_math(text), converted, "convert {text:?}");
        }
    }

    #[test]
    fn math_scanners_stay_linear_on_unclosed_openers() {
        let text = "\\(".repeat(50_000) + &"\\[".repeat(50_000);
        assert_eq!(convert_math(&text), text);
        assert!(escape_and_convert_math(&text).len() > text.len());
    }

    #[test]
    fn md_link_matches_python() {
        // (text, url, image, expected)
        let cases = [
            // oracle: xml._md_link("`a[b]`", None)
            ("`a[b]`", None, false, "[`a[b]`]"),
            // oracle: xml._md_link("`a[b]`", "http://x/a b")
            (
                "`a[b]`",
                Some("http://x/a b"),
                false,
                "[`a[b]`](<http://x/a b>)",
            ),
            // oracle: xml._md_link("`a[b]`", "u", True)
            ("`a[b]`", Some("u"), true, "![`a[b]`](u)"),
            // oracle: xml._md_link("\\[x]", None)
            ("\\[x]", None, false, "[\\[x\\]]"),
            // oracle: xml._md_link("\\[x]", "http://x/a b")
            (
                "\\[x]",
                Some("http://x/a b"),
                false,
                "[\\[x\\]](<http://x/a b>)",
            ),
            // oracle: xml._md_link("\\[x]", "u", True)
            ("\\[x]", Some("u"), true, "![\\[x\\]](u)"),
            // oracle: xml._md_link("a[b]", None)
            ("a[b]", None, false, "[a\\[b\\]]"),
            // oracle: xml._md_link("a[b]", "http://x/a b")
            (
                "a[b]",
                Some("http://x/a b"),
                false,
                "[a\\[b\\]](<http://x/a b>)",
            ),
            // oracle: xml._md_link("a[b]", "u", True)
            ("a[b]", Some("u"), true, "![a\\[b\\]](u)"),
            // oracle: xml._md_link("``x`[y]", None)
            ("``x`[y]", None, false, "[``x`\\[y\\]]"),
            // oracle: xml._md_link("``x`[y]", "http://x/a b")
            (
                "``x`[y]",
                Some("http://x/a b"),
                false,
                "[``x`\\[y\\]](<http://x/a b>)",
            ),
            // oracle: xml._md_link("``x`[y]", "u", True)
            ("``x`[y]", Some("u"), true, "![``x`\\[y\\]](u)"),
            // oracle: xml._md_link("\\\\[z]", None)
            ("\\\\[z]", None, false, "[\\\\[z\\]]"),
            // oracle: xml._md_link("\\\\[z]", "http://x/a b")
            (
                "\\\\[z]",
                Some("http://x/a b"),
                false,
                "[\\\\[z\\]](<http://x/a b>)",
            ),
            // oracle: xml._md_link("\\\\[z]", "u", True)
            ("\\\\[z]", Some("u"), true, "![\\\\[z\\]](u)"),
            // oracle: xml._md_link("``a`b`` [c] `d", None)
            ("``a`b`` [c] `d", None, false, "[``a`b`` \\[c\\] `d]"),
            // oracle: xml._md_link("``a`b`` [c] `d", "http://x/a b")
            (
                "``a`b`` [c] `d",
                Some("http://x/a b"),
                false,
                "[``a`b`` \\[c\\] `d](<http://x/a b>)",
            ),
            // oracle: xml._md_link("`[x]", None)
            ("`[x]", None, false, "[`\\[x\\]]"),
            // oracle: xml._md_link("`[x]", "u", True)
            ("`[x]", Some("u"), true, "![`\\[x\\]](u)"),
            // oracle: xml._md_link("[a]`b`[c]`", None)
            ("[a]`b`[c]`", None, false, "[\\[a\\]`b`\\[c\\]`]"),
            // oracle: xml._md_link("[a]`b`[c]`", "http://x/a b")
            (
                "[a]`b`[c]`",
                Some("http://x/a b"),
                false,
                "[\\[a\\]`b`\\[c\\]`](<http://x/a b>)",
            ),
            // oracle: xml._md_link("\\`a[b]`", None)
            ("\\`a[b]`", None, false, "[\\`a\\[b\\]`]"),
            // oracle: xml._md_link("`a``b[c]`", None)
            ("`a``b[c]`", None, false, "[`a``b[c]`]"),
            // oracle: xml._md_link("``a` b[c] ``", None)
            ("``a` b[c] ``", None, false, "[``a` b[c] ``]"),
            // oracle: xml._md_link("a\\`[b]`c`[d]", None)
            ("a\\`[b]`c`[d]", None, false, "[a\\`\\[b\\]`c`\\[d\\]]"),
            // oracle: xml._md_link("`a\n[b]`", None)
            ("`a\n[b]`", None, false, "[`a\n[b]`]"),
            // oracle: xml._md_link("``[x]`` [y] `[z]`", None)
            ("``[x]`` [y] `[z]`", None, false, "[``[x]`` \\[y\\] `[z]`]"),
        ];
        for (text, url, image, expected) in cases {
            assert_eq!(md_link(text, url, image), expected, "{text:?} {url:?}");
        }
    }

    #[test]
    fn md_link_backtick_runs_stay_linear() {
        // runs of strictly increasing length never close, each failing run once
        let text: String = (1..=2000).map(|n| format!("{} ", "`".repeat(n))).collect();
        assert_eq!(md_link(&text, None, false), format!("[{text}]"));
    }

    #[test]
    fn code_fence_and_span_match_python() {
        // (text, code_fence min 1, code_fence min 3, code_span)
        let cases = [
            // oracle: xml._code_fence("a``b"), xml._code_fence("a``b", 3), xml._code_span("a``b")
            ("a``b", "```", "```", "```a``b```"),
            // oracle: xml._code_fence("`x"), xml._code_fence("`x", 3), xml._code_span("`x")
            ("`x", "``", "```", "`` `x ``"),
            // oracle: xml._code_fence("x`"), xml._code_fence("x`", 3), xml._code_span("x`")
            ("x`", "``", "```", "`` x` ``"),
            // oracle: xml._code_fence(""), xml._code_fence("", 3), xml._code_span("")
            ("", "`", "```", "``"),
            // oracle: xml._code_fence("```"), xml._code_fence("```", 3), xml._code_span("```")
            ("```", "````", "````", "```` ``` ````"),
            // oracle: xml._code_fence("a`b``c```d"), xml._code_fence("a`b``c```d", 3), xml._code_span("a`b``c```d")
            ("a`b``c```d", "````", "````", "````a`b``c```d````"),
        ];
        for (text, fence, fence_min_3, span) in cases {
            assert_eq!(code_fence(text, 1), fence, "{text:?}");
            assert_eq!(code_fence(text, 3), fence_min_3, "{text:?}");
            assert_eq!(code_span(text), span, "{text:?}");
        }
    }

    #[test]
    fn md_wrap_python_whitespace() {
        // oracle: xml._md_wrap("\x1c a \x1f", "**")
        assert_eq!(
            md_wrap("\u{1c} a \u{1f}", "**", None),
            "\u{1c} **a** \u{1f}"
        );
        // oracle: xml._md_code("\x85x\u3000")
        assert_eq!(md_code("\u{85}x\u{3000}"), "\u{85}`x`\u{3000}");
        // oracle: xml._md_wrap("  ", "*")
        assert_eq!(md_wrap("  ", "*", None), "  ");
        // oracle: xml._md_wrap(" a b ", "<u>", "</u>")
        assert_eq!(md_wrap(" a b ", "<u>", Some("</u>")), " <u>a b</u> ");
        // oracle: xml._md_code(" `a` ")
        assert_eq!(md_code(" `a` "), " `` `a` `` ");
        // oracle: xml._md_code("  ")
        assert_eq!(md_code("  "), "  ");
        // oracle: xml._md_wrap("x", "~~", None)
        assert_eq!(md_wrap("x", "~~", None), "~~x~~");
    }

    #[test]
    fn escape_cell_escapes_only_in_a_cell() {
        // upstream: xml.py::_escape_cell, `text.replace("|", "\\|").replace("\n", " ")`
        assert_eq!(escape_cell("a|b\nc", true), "a\\|b c");
        assert_eq!(escape_cell("a|b\nc", false), "a|b\nc");
        assert_eq!(escape_cell("plain", true), "plain");
    }

    /// Upstream's `_escape_inline_specials`: one `\\*c` substitution per character, in order.
    fn escape_inline_specials_in_seven_passes(text: &str) -> String {
        let mut text = text.to_string();
        for ch in INLINE_SPECIAL_CHARS {
            if !text.contains(ch) {
                continue;
            }
            let pattern = Regex::new(&format!(r"\\*{}", regex::escape(&ch.to_string()))).unwrap();
            text = pattern
                .replace_all(&text, |m: &regex::Captures<'_>| {
                    let run = m[0].matches('\\').count();
                    format!("{}{ch}", "\\".repeat(2 * run + 1))
                })
                .into_owned();
        }
        text
    }

    #[test]
    fn escape_inline_specials_single_pass_equals_seven() {
        // every sequence of one to three specials, each behind 0 to 3 backslashes
        let atoms: Vec<String> = INLINE_SPECIAL_CHARS
            .iter()
            .flat_map(|ch| (0..=3).map(move |run| format!("{}{ch}", "\\".repeat(run))))
            .collect();
        let mut texts: Vec<String> = Vec::new();
        for first in &atoms {
            texts.push(first.clone());
            for second in &atoms {
                texts.push(format!("{first}{second}"));
                for third in &atoms {
                    texts.push(format!("{first}a{second}{third}\\"));
                    texts.push(format!("{first}{second}{third}"));
                }
            }
        }
        for text in &texts {
            assert_eq!(
                escape_inline_specials(text),
                escape_inline_specials_in_seven_passes(text),
                "{text:?}"
            );
        }
        assert_eq!(escape_inline_specials("a\\b"), "a\\b");
    }
}
