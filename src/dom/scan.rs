//! Counts the attributes of every tag html5ever's tokenizer reads, before the input is fed to it.
//!
//! html5ever's check for a repeated attribute name compares each attribute with every earlier one of
//! the same tag, so one tag with n attributes costs n^2 comparisons. The scan mirrors the tokenizer
//! states of html5ever (`tokenizer/mod.rs`) closely enough that its tag boundaries are the tokenizer's
//! own, and rejects the input once one tag passes `MAX_TAG_ATTRIBUTES`. It may count more than
//! html5ever keeps (repeated names, a tag cut off by the end of the input), never fewer.
//!
//! Two inputs of the tokenizer are not in the bytes: whether the tree builder switched it to RCDATA,
//! RAWTEXT, script data or PLAINTEXT after a start tag, and whether CDATA sections are open at a
//! `<![CDATA[`. The scan stops with a `Pause` at both and takes the answer from the caller.
//!
//! Byte-wise: every byte that changes a state is ASCII and any other byte is "anything else". `\r`
//! counts as `\n` (html5ever's input preprocessing); its skipping of an `\n` after `\r` never changes a
//! state. html5ever drops a U+FEFF at the start of each `feed` call, so the caller never starts a
//! piece with one, and a pause takes the U+FEFF run after its tag into the piece.

use html5ever::{LocalName, local_name};

use super::parse::MAX_TAG_ATTRIBUTES;
use crate::error::{ExtractError, Rejection};

#[cfg(test)]
mod tests;

/// Why `TagScanner::scan` stopped.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Pause {
    /// Reached `end`.
    End,
    /// Just past the `>` of a start tag named title, textarea, style, xmp, iframe, noembed,
    /// noframes, script or plaintext (ASCII-lowercased name), and past any U+FEFF that follows.
    RawTextTag(LocalName),
    /// At the `<` of `<![CDATA[` (case-sensitive, as html5ever's `eat_exact`) in the Data state;
    /// RCDATA, RAWTEXT, script data and PLAINTEXT never reach the markup declaration check.
    Cdata,
}

/// The start tags after which the tree builder may switch the tokenizer.
#[derive(Clone, Copy, PartialEq, Eq)]
enum RawTag {
    Title,
    Textarea,
    Style,
    Xmp,
    Iframe,
    Noembed,
    Noframes,
    Script,
    Plaintext,
}

impl RawTag {
    fn from_name(name: &[u8]) -> Option<Self> {
        Some(match name {
            b"title" => Self::Title,
            b"textarea" => Self::Textarea,
            b"style" => Self::Style,
            b"xmp" => Self::Xmp,
            b"iframe" => Self::Iframe,
            b"noembed" => Self::Noembed,
            b"noframes" => Self::Noframes,
            b"script" => Self::Script,
            b"plaintext" => Self::Plaintext,
            _ => return None,
        })
    }

    fn name(self) -> &'static [u8] {
        match self {
            Self::Title => b"title",
            Self::Textarea => b"textarea",
            Self::Style => b"style",
            Self::Xmp => b"xmp",
            Self::Iframe => b"iframe",
            Self::Noembed => b"noembed",
            Self::Noframes => b"noframes",
            Self::Script => b"script",
            Self::Plaintext => b"plaintext",
        }
    }

    fn local_name(self) -> LocalName {
        match self {
            Self::Title => local_name!("title"),
            Self::Textarea => local_name!("textarea"),
            Self::Style => local_name!("style"),
            Self::Xmp => local_name!("xmp"),
            Self::Iframe => local_name!("iframe"),
            Self::Noembed => local_name!("noembed"),
            Self::Noframes => local_name!("noframes"),
            Self::Script => local_name!("script"),
            Self::Plaintext => local_name!("plaintext"),
        }
    }

    /// The tokenizer state html5ever's tree builder switches to for this start tag.
    fn state(self) -> State {
        match self {
            Self::Title | Self::Textarea => State::Raw(Raw::Rcdata),
            Self::Style | Self::Xmp | Self::Iframe | Self::Noembed | Self::Noframes => {
                State::Raw(Raw::Rawtext)
            }
            Self::Script => State::Raw(Raw::Script),
            Self::Plaintext => State::Plaintext,
        }
    }
}

/// html5ever's `states::RawKind` and `ScriptEscapeKind` in one.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Raw {
    Rcdata,
    Rawtext,
    Script,
    Escaped,
    DoubleEscaped,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Data,
    TagOpen,
    EndTagOpen,
    TagName,
    BeforeAttrName,
    AttrName,
    AfterAttrName,
    BeforeAttrValue,
    ValueDoubleQuoted,
    ValueSingleQuoted,
    ValueUnquoted,
    AfterValueQuoted,
    SelfClosing,
    MarkupDecl,
    /// Bogus comment, and every DOCTYPE state: all end at the first `>`.
    BogusComment,
    CommentStart,
    CommentStartDash,
    Comment,
    CommentLessThan,
    CommentLessThanBang,
    CommentLessThanBangDash,
    CommentLessThanBangDashDash,
    CommentEndDash,
    CommentEnd,
    CommentEndBang,
    Cdata,
    CdataBracket,
    CdataEnd,
    Plaintext,
    Raw(Raw),
    RawLessThan(Raw),
    RawEndTagOpen(Raw),
    RawEndTagName(Raw),
    EscapeStart,
    EscapeStartDash,
    EscapeDash(Raw),
    EscapeDashDash(Raw),
    DoubleEscapeStart,
    DoubleEscapeEnd,
    /// Consuming `skip` bytes, then `skip_to`.
    Skip,
    /// After `Pause::RawTextTag`, until `resume_after_tag`.
    AwaitTag,
    /// After `Pause::Cdata`, until `resume_at_cdata`.
    AwaitCdata,
}

/// What `step` did with a byte.
enum Step {
    Consume,
    Reconsume,
    /// The byte is the `>` of a tag; the state is Data.
    EndOfTag,
}

/// Longest name `RawTag` knows.
const NAME_CAP: usize = 9;

const BOM_UTF8: &[u8] = &[0xEF, 0xBB, 0xBF];

const CDATA_OPEN: &[u8] = b"<![CDATA[";

fn is_space(byte: u8) -> bool {
    matches!(byte, b'\t' | b'\n' | b'\r' | 0x0C | b' ')
}

/// Index of the first `needle` in `bytes[from..end]`, or `end`.
fn find(bytes: &[u8], from: usize, end: usize, needle: u8) -> usize {
    bytes
        .get(from..end)
        .and_then(|window| window.iter().position(|&byte| byte == needle))
        .map_or(end, |offset| from.saturating_add(offset))
}

/// Index of the first `a` or `b` in `bytes[from..end]`, or `end`.
fn find_either(bytes: &[u8], from: usize, end: usize, a: u8, b: u8) -> usize {
    bytes
        .get(from..end)
        .and_then(|window| window.iter().position(|&byte| byte == a || byte == b))
        .map_or(end, |offset| from.saturating_add(offset))
}

/// The tokenizer's position, as far as tag boundaries and attribute counts go; see the module docs.
pub(super) struct TagScanner {
    state: State,
    /// Attributes read on the tag being scanned.
    attrs: usize,
    is_start_tag: bool,
    /// ASCII-lowercased name of the tag being scanned, or the script data "temporary buffer"; bytes
    /// past `NAME_CAP` are not kept, `name_len` saturates at `NAME_CAP + 1`.
    name: [u8; NAME_CAP],
    name_len: usize,
    /// Raw-text start tag that put the tokenizer in its current RCDATA, RAWTEXT or script state: the
    /// name of the appropriate end tag.
    raw_tag: Option<RawTag>,
    /// Tag a pause is waiting on.
    pending: Option<RawTag>,
    skip: usize,
    skip_to: State,
}

impl TagScanner {
    pub(super) fn new() -> Self {
        Self {
            state: State::Data,
            attrs: 0,
            is_start_tag: false,
            name: [0; NAME_CAP],
            name_len: 0,
            raw_tag: None,
            pending: None,
            skip: 0,
            skip_to: State::Data,
        }
    }

    /// Scans `html[from..end]` (`end` a char boundary, not before a U+FEFF); returns the position it
    /// stopped at, a char boundary in `from..=end`, and why. Err(Rejected(TreeTooLarge)) once one tag
    /// passes MAX_TAG_ATTRIBUTES. Called again while a pause is unanswered, it returns the same pause
    /// at `from`.
    pub(super) fn scan(
        &mut self,
        html: &str,
        from: usize,
        end: usize,
    ) -> Result<(usize, Pause), ExtractError> {
        let bytes = html.as_bytes();
        let mut i = from;
        while i < end {
            match self.state {
                State::AwaitTag => return Ok((from, self.tag_pause())),
                State::AwaitCdata => return Ok((from, Pause::Cdata)),
                State::Skip => {
                    let step = self.skip.min(end - i);
                    i += step;
                    self.skip -= step;
                    if self.skip == 0 {
                        self.state = self.skip_to;
                    }
                    continue;
                }
                State::Plaintext => {
                    i = end;
                    continue;
                }
                State::Data => {
                    i = find(bytes, i, end, b'<');
                    if i >= end {
                        break;
                    }
                    if bytes
                        .get(i..)
                        .is_some_and(|rest| rest.starts_with(CDATA_OPEN))
                    {
                        self.state = State::AwaitCdata;
                        return Ok((i, Pause::Cdata));
                    }
                    self.state = State::TagOpen;
                    i += 1;
                    continue;
                }
                State::MarkupDecl => {
                    let rest = bytes.get(i..).unwrap_or_default();
                    if rest.starts_with(b"--") {
                        self.skip_then(2, State::CommentStart);
                    } else if rest
                        .get(..7)
                        .is_some_and(|word| word.eq_ignore_ascii_case(b"doctype"))
                    {
                        self.skip_then(7, State::BogusComment);
                    } else {
                        self.state = State::BogusComment;
                    }
                    continue;
                }
                State::ValueDoubleQuoted => i = find(bytes, i, end, b'"'),
                State::ValueSingleQuoted => i = find(bytes, i, end, b'\''),
                State::BogusComment => i = find(bytes, i, end, b'>'),
                State::Cdata => i = find(bytes, i, end, b']'),
                State::Comment => i = find_either(bytes, i, end, b'<', b'-'),
                State::Raw(Raw::Rcdata | Raw::Rawtext | Raw::Script) => {
                    i = find(bytes, i, end, b'<');
                }
                State::Raw(Raw::Escaped | Raw::DoubleEscaped) => {
                    i = find_either(bytes, i, end, b'<', b'-');
                }
                _ => {}
            }
            if i >= end {
                break;
            }
            let Some(&byte) = bytes.get(i) else { break };
            match self.step(byte)? {
                Step::Consume => i += 1,
                Step::Reconsume => {}
                Step::EndOfTag => {
                    i += 1;
                    if let Some(tag) = self.raw_text_start_tag() {
                        while i + BOM_UTF8.len() <= end
                            && bytes
                                .get(i..)
                                .is_some_and(|rest| rest.starts_with(BOM_UTF8))
                        {
                            i += BOM_UTF8.len();
                        }
                        self.pending = Some(tag);
                        self.state = State::AwaitTag;
                        return Ok((i, self.tag_pause()));
                    }
                }
            }
        }
        Ok((end, Pause::End))
    }

    /// After `RawTextTag`: whether html5ever switched its tokenizer (the sink received that HTML element).
    pub(super) fn resume_after_tag(&mut self, switched: bool) {
        if self.state != State::AwaitTag {
            return;
        }
        match self.pending.take() {
            Some(tag) if switched => {
                self.raw_tag = Some(tag);
                self.state = tag.state();
            }
            _ => self.state = State::Data,
        }
    }

    /// After `Cdata`: whether the tree builder's adjusted current node is foreign.
    pub(super) fn resume_at_cdata(&mut self, foreign: bool) {
        if self.state != State::AwaitCdata {
            return;
        }
        if foreign {
            self.skip_then(CDATA_OPEN.len(), State::Cdata);
        } else {
            self.skip_then(b"<!".len(), State::BogusComment);
        }
    }

    fn skip_then(&mut self, bytes: usize, then: State) {
        self.skip = bytes;
        self.skip_to = then;
        self.state = State::Skip;
    }

    fn tag_pause(&self) -> Pause {
        match self.pending {
            Some(tag) => Pause::RawTextTag(tag.local_name()),
            None => Pause::End,
        }
    }

    /// The raw-text start tag that just ended, if it is one.
    fn raw_text_start_tag(&self) -> Option<RawTag> {
        if !self.is_start_tag {
            return None;
        }
        RawTag::from_name(self.name.get(..self.name_len)?)
    }

    /// html5ever's `create_tag`: a new tag starts with no attributes.
    fn open_tag(&mut self, is_start_tag: bool, first: u8) {
        self.attrs = 0;
        self.is_start_tag = is_start_tag;
        self.name_len = 0;
        self.push_name(first);
    }

    fn push_name(&mut self, byte: u8) {
        if let Some(slot) = self.name.get_mut(self.name_len) {
            *slot = byte.to_ascii_lowercase();
        }
        self.name_len = (self.name_len + 1).min(NAME_CAP + 1);
    }

    fn name_is(&self, expected: &[u8]) -> bool {
        self.name.get(..self.name_len) == Some(expected)
    }

    /// html5ever's `create_attribute`.
    fn new_attribute(&mut self) -> Result<(), ExtractError> {
        self.attrs += 1;
        if self.attrs > MAX_TAG_ATTRIBUTES {
            return Err(ExtractError::Rejected(Rejection::TreeTooLarge));
        }
        Ok(())
    }

    fn to(&mut self, state: State) -> Step {
        self.state = state;
        Step::Consume
    }

    fn reconsume_in(&mut self, state: State) -> Step {
        self.state = state;
        Step::Reconsume
    }

    fn end_tag(&mut self) -> Step {
        self.state = State::Data;
        Step::EndOfTag
    }

    fn step(&mut self, byte: u8) -> Result<Step, ExtractError> {
        let space = is_space(byte);
        Ok(match self.state {
            // `scan` handles these before a byte reaches `step`.
            State::Data
            | State::MarkupDecl
            | State::Plaintext
            | State::Skip
            | State::AwaitTag
            | State::AwaitCdata => Step::Consume,

            State::TagOpen => match byte {
                b'!' => self.to(State::MarkupDecl),
                b'/' => self.to(State::EndTagOpen),
                b'?' => self.to(State::BogusComment),
                _ if byte.is_ascii_alphabetic() => {
                    self.open_tag(true, byte);
                    self.to(State::TagName)
                }
                _ => self.reconsume_in(State::Data),
            },
            State::EndTagOpen => match byte {
                b'>' => self.to(State::Data),
                _ if byte.is_ascii_alphabetic() => {
                    self.open_tag(false, byte);
                    self.to(State::TagName)
                }
                _ => self.to(State::BogusComment),
            },
            State::TagName => match byte {
                _ if space => self.to(State::BeforeAttrName),
                b'/' => self.to(State::SelfClosing),
                b'>' => self.end_tag(),
                _ => {
                    self.push_name(byte);
                    Step::Consume
                }
            },
            State::BeforeAttrName => match byte {
                _ if space => Step::Consume,
                b'/' => self.to(State::SelfClosing),
                b'>' => self.end_tag(),
                _ => {
                    self.new_attribute()?;
                    self.to(State::AttrName)
                }
            },
            State::AttrName => match byte {
                _ if space => self.to(State::AfterAttrName),
                b'/' => self.to(State::SelfClosing),
                b'>' => self.end_tag(),
                b'=' => self.to(State::BeforeAttrValue),
                _ => Step::Consume,
            },
            State::AfterAttrName => match byte {
                _ if space => Step::Consume,
                b'/' => self.to(State::SelfClosing),
                b'=' => self.to(State::BeforeAttrValue),
                b'>' => self.end_tag(),
                _ => {
                    self.new_attribute()?;
                    self.to(State::AttrName)
                }
            },
            State::BeforeAttrValue => match byte {
                _ if space => Step::Consume,
                b'"' => self.to(State::ValueDoubleQuoted),
                b'\'' => self.to(State::ValueSingleQuoted),
                b'>' => self.end_tag(),
                _ => self.to(State::ValueUnquoted),
            },
            State::ValueDoubleQuoted => match byte {
                b'"' => self.to(State::AfterValueQuoted),
                _ => Step::Consume,
            },
            State::ValueSingleQuoted => match byte {
                b'\'' => self.to(State::AfterValueQuoted),
                _ => Step::Consume,
            },
            State::ValueUnquoted => match byte {
                _ if space => self.to(State::BeforeAttrName),
                b'>' => self.end_tag(),
                _ => Step::Consume,
            },
            State::AfterValueQuoted => match byte {
                _ if space => self.to(State::BeforeAttrName),
                b'/' => self.to(State::SelfClosing),
                b'>' => self.end_tag(),
                _ => self.reconsume_in(State::BeforeAttrName),
            },
            State::SelfClosing => match byte {
                b'>' => self.end_tag(),
                _ => self.reconsume_in(State::BeforeAttrName),
            },

            State::BogusComment => match byte {
                b'>' => self.to(State::Data),
                _ => Step::Consume,
            },
            State::CommentStart => match byte {
                b'-' => self.to(State::CommentStartDash),
                b'>' => self.to(State::Data),
                _ => self.to(State::Comment),
            },
            State::CommentStartDash => match byte {
                b'-' => self.to(State::CommentEnd),
                b'>' => self.to(State::Data),
                _ => self.to(State::Comment),
            },
            State::Comment => match byte {
                b'<' => self.to(State::CommentLessThan),
                b'-' => self.to(State::CommentEndDash),
                _ => Step::Consume,
            },
            State::CommentLessThan => match byte {
                b'!' => self.to(State::CommentLessThanBang),
                b'<' => Step::Consume,
                _ => self.reconsume_in(State::Comment),
            },
            State::CommentLessThanBang => match byte {
                b'-' => self.to(State::CommentLessThanBangDash),
                _ => self.reconsume_in(State::Comment),
            },
            State::CommentLessThanBangDash => match byte {
                b'-' => self.to(State::CommentLessThanBangDashDash),
                _ => self.reconsume_in(State::CommentEndDash),
            },
            State::CommentLessThanBangDashDash => self.reconsume_in(State::CommentEnd),
            State::CommentEndDash => match byte {
                b'-' => self.to(State::CommentEnd),
                _ => self.to(State::Comment),
            },
            State::CommentEnd => match byte {
                b'>' => self.to(State::Data),
                b'!' => self.to(State::CommentEndBang),
                b'-' => Step::Consume,
                _ => self.reconsume_in(State::Comment),
            },
            State::CommentEndBang => match byte {
                b'-' => self.to(State::CommentEndDash),
                b'>' => self.to(State::Data),
                _ => self.to(State::Comment),
            },

            State::Cdata => match byte {
                b']' => self.to(State::CdataBracket),
                _ => Step::Consume,
            },
            State::CdataBracket => match byte {
                b']' => self.to(State::CdataEnd),
                _ => self.reconsume_in(State::Cdata),
            },
            State::CdataEnd => match byte {
                b']' => Step::Consume,
                b'>' => self.to(State::Data),
                _ => self.reconsume_in(State::Cdata),
            },

            State::Raw(raw) => match (raw, byte) {
                (_, b'<') => self.to(State::RawLessThan(raw)),
                (Raw::Escaped | Raw::DoubleEscaped, b'-') => self.to(State::EscapeDash(raw)),
                _ => Step::Consume,
            },
            State::RawLessThan(Raw::Escaped) => match byte {
                b'/' => self.to(State::RawEndTagOpen(Raw::Escaped)),
                _ if byte.is_ascii_alphabetic() => {
                    self.name_len = 0;
                    self.push_name(byte);
                    self.to(State::DoubleEscapeStart)
                }
                _ => self.reconsume_in(State::Raw(Raw::Escaped)),
            },
            State::RawLessThan(Raw::DoubleEscaped) => match byte {
                b'/' => {
                    self.name_len = 0;
                    self.to(State::DoubleEscapeEnd)
                }
                _ => self.reconsume_in(State::Raw(Raw::DoubleEscaped)),
            },
            State::RawLessThan(raw) => match byte {
                b'/' => self.to(State::RawEndTagOpen(raw)),
                b'!' if raw == Raw::Script => self.to(State::EscapeStart),
                _ => self.reconsume_in(State::Raw(raw)),
            },
            State::RawEndTagOpen(raw) => {
                if byte.is_ascii_alphabetic() {
                    self.open_tag(false, byte);
                    self.to(State::RawEndTagName(raw))
                } else {
                    self.reconsume_in(State::Raw(raw))
                }
            }
            State::RawEndTagName(raw) => {
                // html5ever's `have_appropriate_end_tag`: this end tag names the last start tag.
                let appropriate = self.raw_tag.is_some_and(|tag| self.name_is(tag.name()));
                match byte {
                    _ if appropriate && space => self.to(State::BeforeAttrName),
                    b'/' if appropriate => self.to(State::SelfClosing),
                    b'>' if appropriate => self.end_tag(),
                    _ if byte.is_ascii_alphabetic() => {
                        self.push_name(byte);
                        Step::Consume
                    }
                    _ => self.reconsume_in(State::Raw(raw)),
                }
            }
            State::EscapeStart => match byte {
                b'-' => self.to(State::EscapeStartDash),
                _ => self.reconsume_in(State::Raw(Raw::Script)),
            },
            State::EscapeStartDash => match byte {
                b'-' => self.to(State::EscapeDashDash(Raw::Escaped)),
                _ => self.reconsume_in(State::Raw(Raw::Script)),
            },
            State::EscapeDash(raw) => match byte {
                b'-' => self.to(State::EscapeDashDash(raw)),
                b'<' => self.to(State::RawLessThan(raw)),
                _ => self.to(State::Raw(raw)),
            },
            State::EscapeDashDash(raw) => match byte {
                b'-' => Step::Consume,
                b'<' => self.to(State::RawLessThan(raw)),
                b'>' => self.to(State::Raw(Raw::Script)),
                _ => self.to(State::Raw(raw)),
            },
            State::DoubleEscapeStart => match byte {
                _ if space || matches!(byte, b'/' | b'>') => {
                    let next = if self.name_is(b"script") {
                        Raw::DoubleEscaped
                    } else {
                        Raw::Escaped
                    };
                    self.to(State::Raw(next))
                }
                _ if byte.is_ascii_alphabetic() => {
                    self.push_name(byte);
                    Step::Consume
                }
                _ => self.reconsume_in(State::Raw(Raw::Escaped)),
            },
            State::DoubleEscapeEnd => match byte {
                _ if space || matches!(byte, b'/' | b'>') => {
                    let next = if self.name_is(b"script") {
                        Raw::Escaped
                    } else {
                        Raw::DoubleEscaped
                    };
                    self.to(State::Raw(next))
                }
                _ if byte.is_ascii_alphabetic() => {
                    self.push_name(byte);
                    Step::Consume
                }
                _ => self.reconsume_in(State::Raw(Raw::DoubleEscaped)),
            },
        })
    }
}
