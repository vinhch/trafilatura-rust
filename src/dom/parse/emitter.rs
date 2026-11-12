//! The html5gum emitter behind the libxml2 tree builder: the tokens of a page, with libxml2's name,
//! attribute and limit rules applied while the tokenizer runs.

use std::collections::{HashSet, VecDeque};
use std::mem;
use std::sync::Arc;

use html5ever::LocalName;
use html5gum::{Error, State};

use super::tables::{DataMode, element};
use super::{ATTRIBUTES_PER_NODE, BOM, MAX_TAG_ATTRIBUTES, ParseLimits, is_unread_text_element};
use crate::dom::Ns;
use crate::error::{ExtractError, Rejection, TreeLimit};

/// Longest tag or attribute name kept, in bytes.
// libxml2: HTMLparser.c::htmlParseHTMLName
const NAME_BYTES: usize = 100;

/// One token of the tree builder's input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Token {
    Start(StartTag),
    /// A start tag cut off by the end of input. The tree builder still runs its implied-end and
    /// implied-start rules for it, then discards it.
    CutStart(LocalName),
    End(LocalName),
    Text(String),
    /// Markup or text the tree builder never reads; the loop still counts it.
    Skipped,
    /// A limit was crossed. It is the last token: the loop stops on it.
    Abort(ExtractError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StartTag {
    pub(super) name: LocalName,
    /// Kept attributes in source order: the first of each name, values decoded.
    pub(super) attrs: Vec<(LocalName, Arc<str>)>,
    /// Names written with no value at all, in the order kept.
    pub(super) valueless: Vec<LocalName>,
    pub(super) self_closing: bool,
}

/// What the tokenizer's current token is.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Current {
    Start,
    End,
    /// A comment, a doctype or nothing yet.
    Other,
}

/// The state html5gum's emitter callbacks keep, and the token queue they fill.
pub(super) struct Emitter {
    queue: VecDeque<Token>,
    /// Bytes of the text run in progress, decoded when the run is flushed.
    text: Vec<u8>,
    /// A run of unread text was dropped since the last flush.
    text_dropped: bool,
    /// Some unread text was dropped in this parse.
    dropped_any: bool,
    drop_script_text: bool,
    /// Inside an unread `style` or `script`: its text is dropped up to the next end tag.
    in_unread_text: bool,
    current: Current,
    name: Vec<u8>,
    self_closing: bool,
    /// Name of the last start tag, cut as its tag was kept.
    last_start: Option<LocalName>,
    attrs: Vec<(LocalName, Arc<str>)>,
    valueless: Vec<LocalName>,
    /// Names kept on the current tag, for the first-wins check.
    seen: HashSet<LocalName>,
    /// Attributes of the current tag, duplicates included, against `MAX_TAG_ATTRIBUTES`.
    tag_attributes: usize,
    /// Attributes kept over the whole parse, against `attribute_budget`.
    kept_attributes: usize,
    attribute_budget: Option<usize>,
    attr_open: bool,
    attr_has_value: bool,
    attr_name: Vec<u8>,
    attr_value: Vec<u8>,
    /// Set at the first limit crossed; every later call changes nothing.
    done: bool,
}

impl Emitter {
    pub(super) fn new(limits: &ParseLimits, drop_script_text: bool) -> Self {
        Self {
            queue: VecDeque::new(),
            text: Vec::new(),
            text_dropped: false,
            dropped_any: false,
            drop_script_text,
            in_unread_text: false,
            current: Current::Other,
            name: Vec::new(),
            self_closing: false,
            last_start: None,
            attrs: Vec::new(),
            valueless: Vec::new(),
            seen: HashSet::new(),
            tag_attributes: 0,
            kept_attributes: 0,
            attribute_budget: limits
                .max_nodes
                .map(|max| max.saturating_mul(ATTRIBUTES_PER_NODE)),
            attr_open: false,
            attr_has_value: false,
            attr_name: Vec::new(),
            attr_value: Vec::new(),
            done: false,
        }
    }

    /// Whether some unread `style` or `script` text was dropped.
    pub(super) fn dropped_text(&self) -> bool {
        self.dropped_any
    }

    /// Queues the text run in progress, or a `Skipped` when only dropped text was pending.
    fn flush_text(&mut self) {
        if self.done {
            return;
        }
        let dropped = mem::take(&mut self.text_dropped);
        if !self.text.is_empty() {
            let text = decode(&self.text);
            self.text.clear();
            self.queue.push_back(Token::Text(text));
        } else if dropped {
            self.queue.push_back(Token::Skipped);
        }
    }

    /// Resets the tag in progress. Text is not flushed here: in RCDATA and RAWTEXT an end tag that
    /// is not the appropriate one is text.
    fn begin_tag(&mut self, current: Current) {
        self.current = current;
        self.name.clear();
        self.self_closing = false;
        self.attrs.clear();
        self.valueless.clear();
        self.seen.clear();
        self.tag_attributes = 0;
        self.attr_open = false;
    }

    /// Queues `Skipped` for a comment or doctype the tree builder never reads.
    fn drop_markup(&mut self) {
        if self.done {
            return;
        }
        self.flush_text();
        self.queue.push_back(Token::Skipped);
    }

    fn abort(&mut self, limit: TreeLimit) {
        self.flush_text();
        self.queue.push_back(Token::Abort(ExtractError::Rejected(
            Rejection::TreeTooLarge(limit),
        )));
        self.done = true;
    }

    /// Stores the attribute in progress. The first of each name is kept; end tags keep none.
    fn commit_attr(&mut self) {
        if !mem::take(&mut self.attr_open) || self.current != Current::Start {
            return;
        }
        let name = local_name(&self.attr_name);
        if !self.seen.insert(name.clone()) {
            return;
        }
        self.kept_attributes = self.kept_attributes.saturating_add(1);
        if self
            .attribute_budget
            .is_some_and(|budget| self.kept_attributes > budget)
        {
            self.abort(TreeLimit::MaxNodes);
            return;
        }
        let value: Arc<str> = if self.attr_has_value {
            Arc::from(decode(&self.attr_value))
        } else {
            self.valueless.push(name.clone());
            Arc::from("")
        };
        self.attrs.push((name, value));
    }
}

/// Decodes the bytes of a token. Flushes fall on ASCII markup, so the bytes are whole characters
/// for `&str` input and the lossy fallback is not reached; it keeps the decode panic-free.
fn decode(bytes: &[u8]) -> String {
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => String::from_utf8_lossy(bytes).into_owned(),
    }
}

/// A tag or attribute name as libxml2 keeps it: at most `NAME_BYTES` bytes, whole characters only.
// libxml2: HTMLparser.c::htmlParseHTMLName
fn local_name(bytes: &[u8]) -> LocalName {
    let text = String::from_utf8_lossy(bytes);
    if text.len() <= NAME_BYTES {
        return LocalName::from(text.as_ref());
    }
    let cut = text
        .char_indices()
        .take_while(|&(index, ch)| index + ch.len_utf8() <= NAME_BYTES)
        .last()
        .map_or(0, |(index, ch)| index + ch.len_utf8());
    LocalName::from(text.get(..cut).unwrap_or_default())
}

/// The state a start tag of this data mode switches the tokenizer to.
fn raw_state(mode: DataMode) -> Option<State> {
    match mode {
        DataMode::Normal => None,
        DataMode::RcData => Some(State::RcData),
        DataMode::RawText => Some(State::RawText),
        DataMode::Script => Some(State::ScriptData),
        DataMode::PlainText => Some(State::PlainText),
    }
}

/// The tokens of `html`, after one leading U+FEFF is stripped.
pub(super) fn tokens<'e>(
    html: &'e str,
    emitter: &'e mut Emitter,
) -> impl Iterator<Item = Token> + 'e {
    let html = html.strip_prefix(BOM).unwrap_or(html);
    html5gum::Tokenizer::new_with_emitter(html, emitter).map(|token| match token {
        Ok(token) => token,
        Err(never) => match never {},
    })
}

impl html5gum::Emitter for &mut Emitter {
    type Token = Token;

    fn set_last_start_tag(&mut self, last_start_tag: Option<&[u8]>) {
        self.last_start = last_start_tag.map(local_name);
    }

    fn emit_eof(&mut self) {
        self.flush_text();
    }

    fn emit_error(&mut self, error: Error) {
        if self.done {
            return;
        }
        match error {
            // A start tag cut off by the end of input is kept for the tree builder to close.
            Error::EofInTag if self.current == Current::Start && !self.name.is_empty() => {
                self.flush_text();
                let name = local_name(&self.name);
                self.queue.push_back(Token::CutStart(name));
            }
            // `c=>` reads an empty value, not a valueless attribute.
            // libxml2: HTMLparser.c::htmlParseAttValue
            Error::MissingAttributeValue => self.attr_has_value = true,
            _ => {}
        }
    }

    /// The errors above are the input of two rules, so they are on.
    fn should_emit_errors(&mut self) -> bool {
        true
    }

    fn pop_token(&mut self) -> Option<Token> {
        self.queue.pop_front()
    }

    fn emit_string(&mut self, c: &[u8]) {
        if self.done {
            return;
        }
        if self.in_unread_text {
            self.text_dropped = true;
            self.dropped_any = true;
        } else {
            self.text.extend_from_slice(c);
        }
    }

    fn init_start_tag(&mut self) {
        self.flush_text();
        self.begin_tag(Current::Start);
    }

    fn init_end_tag(&mut self) {
        self.begin_tag(Current::End);
    }

    fn init_comment(&mut self) {
        self.current = Current::Other;
    }

    fn init_doctype(&mut self) {
        self.current = Current::Other;
    }

    fn emit_current_tag(&mut self) -> Option<State> {
        if self.done {
            return None;
        }
        self.flush_text();
        self.commit_attr();
        if self.done {
            return None;
        }
        let name = local_name(&self.name);
        if self.current == Current::End {
            self.in_unread_text = false;
            self.queue.push_back(Token::End(name));
            return None;
        }
        let attrs = mem::take(&mut self.attrs);
        let valueless = mem::take(&mut self.valueless);
        let self_closing = self.self_closing;
        self.in_unread_text = self.drop_script_text
            && !self_closing
            && is_unread_text_element(&name, Ns::Html, &attrs);
        let state = if self_closing {
            None
        } else {
            element(&name).and_then(|desc| raw_state(desc.data_mode))
        };
        self.last_start = Some(name.clone());
        self.queue.push_back(Token::Start(StartTag {
            name,
            attrs,
            valueless,
            self_closing,
        }));
        state
    }

    fn emit_current_comment(&mut self) {
        self.drop_markup();
    }

    fn emit_current_doctype(&mut self) {
        self.drop_markup();
    }

    fn set_self_closing(&mut self) {
        self.self_closing = true;
    }

    fn set_force_quirks(&mut self) {}

    fn push_tag_name(&mut self, s: &[u8]) {
        self.name.extend_from_slice(s);
    }

    fn push_comment(&mut self, _: &[u8]) {}

    fn push_doctype_name(&mut self, _: &[u8]) {}

    fn set_doctype_public_identifier(&mut self, _: &[u8]) {}

    fn set_doctype_system_identifier(&mut self, _: &[u8]) {}

    fn push_doctype_public_identifier(&mut self, _: &[u8]) {}

    fn push_doctype_system_identifier(&mut self, _: &[u8]) {}

    fn init_attribute(&mut self) {
        if self.done {
            return;
        }
        self.commit_attr();
        if self.done {
            return;
        }
        self.tag_attributes = self.tag_attributes.saturating_add(1);
        if self.tag_attributes > MAX_TAG_ATTRIBUTES {
            self.abort(TreeLimit::TagAttributes);
            return;
        }
        self.attr_open = true;
        self.attr_has_value = false;
        self.attr_name.clear();
        self.attr_value.clear();
    }

    fn init_attribute_value(&mut self) {
        self.attr_has_value = true;
    }

    fn push_attribute_name(&mut self, s: &[u8]) {
        self.attr_name.extend_from_slice(s);
    }

    fn push_attribute_value(&mut self, s: &[u8]) {
        self.attr_value.extend_from_slice(s);
    }

    fn current_is_appropriate_end_tag_token(&mut self) -> bool {
        self.current == Current::End && self.last_start.as_ref() == Some(&local_name(&self.name))
    }
}
