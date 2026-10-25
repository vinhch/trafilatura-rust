//! Python's `json.loads` and the string part of `json.dumps`, over a flat document.
//!
//! `serde_json` differs from CPython's decoder where it changes which JSON-LD scripts parse (lone
//! surrogates, raw control characters with `strict=False`, `NaN`, the 4300-digit integer limit, the
//! nesting cap), so the grammar is written here. The document holds no type that contains itself,
//! and the parser and every reader are loops, so a 9998-deep document neither overflows the stack
//! while parsing nor while dropping.

use std::collections::HashMap;
use std::ops::Range;

use crate::cancel::Call;
use crate::error::{ExtractError, Rejection};
use crate::html_processing::tick;
use crate::pystr::MAX_STR_DIGITS;

/// Deepest nesting of lists and objects that parses; CPython's decoder raises `RecursionError` one
/// level deeper.
const MAX_DEPTH: usize = 9998;

/// An object scans its entries linearly for a repeated key up to this many entries, then keeps a
/// hash index.
const LINEAR_SCAN_LIMIT: usize = 16;

/// What a node of a [`Json`] document holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// `null`.
    Null,
    /// `true` or `false`; the value is not kept.
    Bool,
    /// Any number, `NaN` and `Infinity` included; the value is not kept.
    Number,
    /// A string, read with [`Json::as_str`].
    Str,
    /// A list, read with [`Json::items`].
    List,
    /// An object, read with [`Json::entries`] and [`Json::get`].
    Object,
}

/// A node of a [`Json`] document, valid for the document that returned it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsonRef(u32);

/// `start` and `len` select a range of `strings` for a string, of `children` for a list and of
/// `entries` for an object; they are zero for the other kinds.
#[derive(Clone, Copy, Debug)]
struct Node {
    kind: Kind,
    start: u32,
    len: u32,
}

/// An object entry; the key is a range of `strings`.
#[derive(Clone, Copy, Debug)]
struct Entry {
    key_start: u32,
    key_len: u32,
    value: u32,
}

/// A parsed JSON document in flat vectors, so that dropping, cloning and printing it never recurse.
#[derive(Clone, Debug)]
pub struct Json {
    nodes: Vec<Node>,
    strings: String,
    children: Vec<u32>,
    entries: Vec<Entry>,
    root: u32,
}

fn span(start: u32, len: u32) -> Range<usize> {
    let start = start as usize;
    start..start.saturating_add(len as usize)
}

impl Json {
    /// The top-level value.
    pub fn root(&self) -> JsonRef {
        JsonRef(self.root)
    }

    /// The kind of `r`.
    pub fn kind(&self, r: JsonRef) -> Kind {
        self.node(r).map_or(Kind::Null, |node| node.kind)
    }

    /// The text of a string node, `None` for any other kind. A lone surrogate escape reads as
    /// U+FFFF.
    pub fn as_str(&self, r: JsonRef) -> Option<&str> {
        let node = self.node(r).filter(|node| node.kind == Kind::Str)?;
        self.strings.get(span(node.start, node.len))
    }

    /// The elements of a list in order, none for any other kind.
    pub fn items(&self, r: JsonRef) -> impl ExactSizeIterator<Item = JsonRef> + '_ {
        let ids: &[u32] = match self.node(r) {
            Some(node) if node.kind == Kind::List => self
                .children
                .get(span(node.start, node.len))
                .unwrap_or_default(),
            _ => &[],
        };
        ids.iter().map(|&id| JsonRef(id))
    }

    /// The `(key, value)` pairs of an object, none for any other kind. A repeated key keeps the
    /// position of its first occurrence and the value of its last. A key written with a lone
    /// surrogate escape reads with U+FFFF, so two distinct keys can read alike.
    pub fn entries(&self, r: JsonRef) -> impl ExactSizeIterator<Item = (&str, JsonRef)> + '_ {
        let entries: &[Entry] = match self.node(r) {
            Some(node) if node.kind == Kind::Object => self
                .entries
                .get(span(node.start, node.len))
                .unwrap_or_default(),
            _ => &[],
        };
        entries.iter().map(|entry| {
            let key = self
                .strings
                .get(span(entry.key_start, entry.key_len))
                .unwrap_or_default();
            (key, JsonRef(entry.value))
        })
    }

    /// The value of the first entry of an object whose readable key is `key`; callers look up
    /// ASCII keys only.
    pub fn get(&self, r: JsonRef, key: &str) -> Option<JsonRef> {
        self.entries(r)
            .find_map(|(entry_key, value)| (entry_key == key).then_some(value))
    }

    fn node(&self, r: JsonRef) -> Option<Node> {
        self.nodes.get(r.0 as usize).copied()
    }
}

/// Why a parse stopped.
enum Stop {
    /// Python raises `JSONDecodeError` or `RecursionError`: the text is no document.
    Invalid,
    /// The call was cancelled, or a size limit of this port was reached.
    Call(ExtractError),
}

impl From<ExtractError> for Stop {
    fn from(error: ExtractError) -> Self {
        Self::Call(error)
    }
}

fn to_u32(n: usize) -> Result<u32, Stop> {
    u32::try_from(n).map_err(|_| Stop::Call(ExtractError::Rejected(Rejection::InputTooLarge)))
}

/// Keys of one large object. A key that held a lone surrogate is identified by its UTF-16 units,
/// any other by its text, so the escapes for D800, D801 and FFFF stay three keys although all
/// read as U+FFFF. The maps use std's randomly seeded hasher, so a page cannot craft collisions.
#[derive(Default)]
struct KeyIndex {
    plain: HashMap<Box<str>, usize>,
    lone: HashMap<Box<[u16]>, usize>,
}

impl KeyIndex {
    fn find(&self, key: &str, units: Option<&[u16]>) -> Option<usize> {
        match units {
            Some(units) => self.lone.get(units),
            None => self.plain.get(key),
        }
        .copied()
    }

    fn insert(&mut self, key: &str, units: Option<&[u16]>, at: usize) {
        match units {
            Some(units) => self.lone.insert(units.into(), at),
            None => self.plain.insert(key.into(), at),
        };
    }
}

/// A list or object being parsed. `start` is where its children begin in the parser's pending
/// vector.
enum Frame {
    List {
        start: usize,
    },
    Object {
        start: usize,
        /// The pending entry that the value being parsed belongs to.
        current: usize,
        /// Built once the object passes `LINEAR_SCAN_LIMIT` entries.
        index: Option<KeyIndex>,
    },
}

/// The UTF-16 units of an object key that held a lone surrogate, `None` for any other key.
type KeyUnits = Option<Box<[u16]>>;

/// An object entry before its object closes, with the UTF-16 units of its key when the key held a
/// lone surrogate.
type PendingEntry = (Entry, KeyUnits);

struct Parser<'a> {
    text: &'a str,
    pos: usize,
    strict: bool,
    out: Json,
    pending: Vec<u32>,
    pending_entries: Vec<PendingEntry>,
    frames: Vec<Frame>,
    visited: usize,
}

/// Appends `text` to the string being built and, when it tracks the UTF-16 units of a key, to them.
fn append(strings: &mut String, units: &mut Option<Vec<u16>>, text: &str) {
    strings.push_str(text);
    if let Some(units) = units {
        units.extend(text.encode_utf16());
    }
}

fn hex4(bytes: &[u8], at: usize) -> Option<u32> {
    let digits = bytes.get(at..at.checked_add(4)?)?;
    digits.iter().try_fold(0u32, |value, &byte| {
        Some(value << 4 | char::from(byte).to_digit(16)?)
    })
}

/// The length of the number that starts `rest`, as CPython's scanner reads it: an optional `-`, an
/// integer without leading zeros, an optional fraction and an optional exponent. An integer of
/// more than `MAX_STR_DIGITS` digits fails, as `int()` does; a float has no limit.
fn number_len(rest: &[u8]) -> Result<usize, Stop> {
    let digits_from = |from: usize| {
        rest.get(from..)
            .unwrap_or_default()
            .iter()
            .take_while(|byte| byte.is_ascii_digit())
            .count()
    };
    let mut end = usize::from(rest.first() == Some(&b'-'));
    let integer_digits = match rest.get(end) {
        Some(b'1'..=b'9') => digits_from(end),
        Some(b'0') => 1,
        _ => return Err(Stop::Invalid),
    };
    end += integer_digits;
    let mut is_float = false;
    if rest.get(end) == Some(&b'.') && rest.get(end + 1).is_some_and(u8::is_ascii_digit) {
        is_float = true;
        end += 1 + digits_from(end + 1);
    }
    if matches!(rest.get(end), Some(b'e' | b'E')) {
        let sign = usize::from(matches!(rest.get(end + 1), Some(b'+' | b'-')));
        let exponent_digits = digits_from(end + 1 + sign);
        if exponent_digits > 0 {
            is_float = true;
            end += 1 + sign + exponent_digits;
        }
    }
    if !is_float && integer_digits > MAX_STR_DIGITS {
        return Err(Stop::Invalid);
    }
    Ok(end)
}

impl<'a> Parser<'a> {
    fn new(text: &'a str, strict: bool) -> Self {
        Self {
            text,
            pos: 0,
            strict,
            out: Json {
                nodes: Vec::new(),
                strings: String::new(),
                children: Vec::new(),
                entries: Vec::new(),
                root: 0,
            },
            pending: Vec::new(),
            pending_entries: Vec::new(),
            frames: Vec::new(),
            visited: 0,
        }
    }

    fn peek(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    /// Skips JSON whitespace, which is `" \t\n\r"` only.
    fn skip_space(&mut self) {
        let rest = self.text.as_bytes().get(self.pos..).unwrap_or_default();
        self.pos += rest
            .iter()
            .take_while(|byte| matches!(byte, b' ' | b'\t' | b'\n' | b'\r'))
            .count();
    }

    fn push_node(&mut self, kind: Kind, start: usize, len: usize) -> Result<u32, Stop> {
        let id = to_u32(self.out.nodes.len())?;
        self.out.nodes.push(Node {
            kind,
            start: to_u32(start)?,
            len: to_u32(len)?,
        });
        Ok(id)
    }

    /// Parses the whole text into `out`. One loop: a value is parsed, then closed into the
    /// innermost open list or object, which may close in turn; no call nests with the document.
    fn run(&mut self, call: &Call<'_>) -> Result<(), Stop> {
        loop {
            self.skip_space();
            let Some(first) = self.peek() else {
                return Err(Stop::Invalid);
            };
            tick(&mut self.visited, call)?;
            let value = match first {
                b'{' => self.open_object()?,
                b'[' => self.open_list()?,
                b'"' => {
                    self.pos += 1;
                    let (start, _) = self.string(false)?;
                    let len = self.out.strings.len() - start;
                    Some(self.push_node(Kind::Str, start, len)?)
                }
                _ => Some(self.scalar(first)?),
            };
            // `None`: a container opened and waits for its first value.
            let Some(mut node) = value else { continue };
            loop {
                match self.frames.last_mut() {
                    None => {
                        self.skip_space();
                        if self.pos < self.text.len() {
                            return Err(Stop::Invalid);
                        }
                        self.out.root = node;
                        return Ok(());
                    }
                    Some(Frame::List { start }) => {
                        let start = *start;
                        self.pending.push(node);
                        self.skip_space();
                        match self.peek() {
                            Some(b']') => {
                                self.pos += 1;
                                self.frames.pop();
                                let at = self.out.children.len();
                                let len = self.pending.len() - start;
                                self.out.children.extend(self.pending.drain(start..));
                                node = self.push_node(Kind::List, at, len)?;
                            }
                            Some(b',') => {
                                self.pos += 1;
                                break;
                            }
                            _ => return Err(Stop::Invalid),
                        }
                    }
                    Some(Frame::Object { start, current, .. }) => {
                        let (start, current) = (*start, *current);
                        let (entry, _) =
                            self.pending_entries.get_mut(current).ok_or(Stop::Invalid)?;
                        entry.value = node;
                        self.skip_space();
                        match self.peek() {
                            Some(b'}') => {
                                self.pos += 1;
                                self.frames.pop();
                                let at = self.out.entries.len();
                                let len = self.pending_entries.len() - start;
                                let closed = self.pending_entries.drain(start..);
                                self.out.entries.extend(closed.map(|(entry, _)| entry));
                                node = self.push_node(Kind::Object, at, len)?;
                            }
                            Some(b',') => {
                                self.pos += 1;
                                self.skip_space();
                                self.read_key()?;
                                break;
                            }
                            _ => return Err(Stop::Invalid),
                        }
                    }
                }
            }
        }
    }

    /// Consumes `[` or `{` and the whitespace after it, within the nesting cap.
    fn enter(&mut self) -> Result<(), Stop> {
        if self.frames.len() >= MAX_DEPTH {
            return Err(Stop::Invalid);
        }
        self.pos += 1;
        self.skip_space();
        Ok(())
    }

    /// At `[`: the node of an empty list, or `None` after opening a frame.
    fn open_list(&mut self) -> Result<Option<u32>, Stop> {
        self.enter()?;
        if self.peek() == Some(b']') {
            self.pos += 1;
            let at = self.out.children.len();
            return Ok(Some(self.push_node(Kind::List, at, 0)?));
        }
        self.frames.push(Frame::List {
            start: self.pending.len(),
        });
        Ok(None)
    }

    /// At `{`: the node of an empty object, or `None` after opening a frame and reading the first
    /// key.
    fn open_object(&mut self) -> Result<Option<u32>, Stop> {
        self.enter()?;
        if self.peek() == Some(b'}') {
            self.pos += 1;
            let at = self.out.entries.len();
            return Ok(Some(self.push_node(Kind::Object, at, 0)?));
        }
        self.frames.push(Frame::Object {
            start: self.pending_entries.len(),
            current: 0,
            index: None,
        });
        self.read_key()?;
        Ok(None)
    }

    /// Reads `"key" :` and registers the entry in the open object.
    fn read_key(&mut self) -> Result<(), Stop> {
        if self.peek() != Some(b'"') {
            return Err(Stop::Invalid);
        }
        self.pos += 1;
        let (key_start, units) = self.string(true)?;
        self.skip_space();
        if self.peek() != Some(b':') {
            return Err(Stop::Invalid);
        }
        self.pos += 1;
        self.add_entry(key_start, units)
    }

    /// Registers the key that `strings` holds from `key_start`. A repeated key reuses its first
    /// entry, whose value the next value replaces.
    fn add_entry(&mut self, key_start: usize, units: KeyUnits) -> Result<(), Stop> {
        let Some(Frame::Object {
            start,
            current,
            index,
        }) = self.frames.last_mut()
        else {
            return Err(Stop::Invalid);
        };
        let strings = &self.out.strings;
        let key = strings.get(key_start..).ok_or(Stop::Invalid)?;
        let existing = match index {
            Some(index) => index.find(key, units.as_deref()),
            None => self
                .pending_entries
                .get(*start..)
                .unwrap_or_default()
                .iter()
                .position(|(entry, entry_units)| {
                    *entry_units == units
                        && strings.get(span(entry.key_start, entry.key_len)) == Some(key)
                })
                .map(|offset| offset + *start),
        };
        if let Some(found) = existing {
            self.out.strings.truncate(key_start);
            *current = found;
            return Ok(());
        }
        let entry = Entry {
            key_start: to_u32(key_start)?,
            key_len: to_u32(key.len())?,
            value: 0,
        };
        let at = self.pending_entries.len();
        if let Some(index) = index {
            index.insert(key, units.as_deref(), at);
        }
        self.pending_entries.push((entry, units));
        *current = at;
        if index.is_none() && at + 1 - *start > LINEAR_SCAN_LIMIT {
            let mut built = KeyIndex::default();
            let entries = self.pending_entries.get(*start..).unwrap_or_default();
            for (offset, (entry, entry_units)) in entries.iter().enumerate() {
                let text = strings
                    .get(span(entry.key_start, entry.key_len))
                    .unwrap_or_default();
                built.insert(text, entry_units.as_deref(), offset + *start);
            }
            *index = Some(built);
        }
        Ok(())
    }

    /// Reads a string whose opening quote is consumed, appending its text to `strings` from the
    /// returned offset. For a key, also returns its UTF-16 units when it held a lone surrogate.
    fn string(&mut self, is_key: bool) -> Result<(usize, KeyUnits), Stop> {
        let start = self.out.strings.len();
        let mut units: Option<Vec<u16>> = None;
        let text = self.text;
        loop {
            let rest = text.get(self.pos..).ok_or(Stop::Invalid)?;
            let end = rest.find(['"', '\\']).ok_or(Stop::Invalid)?;
            let run = rest.get(..end).unwrap_or_default();
            if self.strict && run.bytes().any(|byte| byte < 0x20) {
                return Err(Stop::Invalid);
            }
            append(&mut self.out.strings, &mut units, run);
            let delimiter = rest.as_bytes().get(end).copied();
            self.pos += end + 1;
            if delimiter == Some(b'"') {
                break;
            }
            let escape = self.peek().ok_or(Stop::Invalid)?;
            self.pos += 1;
            let plain = match escape {
                b'"' => '"',
                b'\\' => '\\',
                b'/' => '/',
                b'b' => '\u{8}',
                b'f' => '\u{c}',
                b'n' => '\n',
                b'r' => '\r',
                b't' => '\t',
                b'u' => {
                    self.unicode_escape(start, is_key, &mut units)?;
                    continue;
                }
                _ => return Err(Stop::Invalid),
            };
            append(
                &mut self.out.strings,
                &mut units,
                plain.encode_utf8(&mut [0; 4]),
            );
        }
        Ok((start, units.map(Vec::into_boxed_slice)))
    }

    /// After `\u`: appends the code point. A high surrogate escape followed by a low one joins
    /// into one character, a lone surrogate reads as U+FFFF; a key keeps the lone surrogate's
    /// unit in `units`, started from the key's text so far at `start`.
    fn unicode_escape(
        &mut self,
        start: usize,
        is_key: bool,
        units: &mut Option<Vec<u16>>,
    ) -> Result<(), Stop> {
        let bytes = self.text.as_bytes();
        let mut code = hex4(bytes, self.pos).ok_or(Stop::Invalid)?;
        self.pos += 4;
        let next_is_escape = bytes
            .get(self.pos..)
            .is_some_and(|rest| rest.starts_with(b"\\u"));
        if (0xD800..0xDC00).contains(&code) && next_is_escape {
            // CPython reads the second escape whether or not it joins, so a malformed one fails.
            let low = hex4(bytes, self.pos + 2).ok_or(Stop::Invalid)?;
            if (0xDC00..0xE000).contains(&low) {
                code = 0x10000 + ((code - 0xD800) << 10 | (low - 0xDC00));
                self.pos += 6;
            }
        }
        if let Some(c) = char::from_u32(code) {
            append(&mut self.out.strings, units, c.encode_utf8(&mut [0; 4]));
            return Ok(());
        }
        if is_key {
            let tracked = units.get_or_insert_with(|| {
                let so_far = self.out.strings.get(start..).unwrap_or_default();
                so_far.encode_utf16().collect()
            });
            tracked.push(u16::try_from(code).unwrap_or(0xFFFF));
        }
        self.out.strings.push('\u{FFFF}');
        Ok(())
    }

    /// Parses a literal or a number at `pos`, whose first byte is `first`.
    fn scalar(&mut self, first: u8) -> Result<u32, Stop> {
        let rest = self.text.as_bytes().get(self.pos..).unwrap_or_default();
        let (kind, len) = match first {
            b'n' if rest.starts_with(b"null") => (Kind::Null, 4),
            b't' if rest.starts_with(b"true") => (Kind::Bool, 4),
            b'f' if rest.starts_with(b"false") => (Kind::Bool, 5),
            b'N' if rest.starts_with(b"NaN") => (Kind::Number, 3),
            b'I' if rest.starts_with(b"Infinity") => (Kind::Number, 8),
            b'-' if rest.starts_with(b"-Infinity") => (Kind::Number, 9),
            _ => (Kind::Number, number_len(rest)?),
        };
        self.pos += len;
        self.push_node(kind, 0, 0)
    }
}

/// Parses `text` as Python's `json.loads` does: `strict` rejects code points below U+0020 inside
/// strings, `NaN`, `Infinity` and `-Infinity` are numbers, a lone surrogate escape reads as U+FFFF,
/// an integer of more than 4300 digits and nesting deeper than 9998 levels fail. `Ok(None)` is a
/// Python parse failure; `Err` is a cancellation or deadline from `call`, which is checked once per
/// value, or `Rejection::InputTooLarge` when a node count or offset does not fit 32 bits (text over
/// 4 GiB).
// upstream: json.loads (CPython)
pub fn loads(text: &str, strict: bool, call: &Call<'_>) -> Result<Option<Json>, ExtractError> {
    let mut parser = Parser::new(text, strict);
    match parser.run(call) {
        Ok(()) => Ok(Some(parser.out)),
        Err(Stop::Invalid) => Ok(None),
        Err(Stop::Call(error)) => Err(error),
    }
}

/// `s` as one JSON string, as `json.dumps(s, ensure_ascii=False)` writes it: `"`, `\`, `\n`, `\r`,
/// `\t`, `\b` and `\f` escaped, other code points below U+0020 as `\u00XX`, the rest raw.
// upstream: json.dumps (CPython)
pub fn dumps_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        let short = match c {
            '"' => '"',
            '\\' => '\\',
            '\n' => 'n',
            '\r' => 'r',
            '\t' => 't',
            '\u{8}' => 'b',
            '\u{c}' => 'f',
            c if c < ' ' => {
                out.push('\\');
                out.push('u');
                out.push_str("00");
                let code = u32::from(c);
                out.extend(
                    [code >> 4, code & 15]
                        .map(|nibble| char::from_digit(nibble, 16).unwrap_or('0')),
                );
                continue;
            }
            c => {
                out.push(c);
                continue;
            }
        };
        out.push('\\');
        out.push(short);
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests;
