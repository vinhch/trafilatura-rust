//! The libxml2 tree builder: the tree `lxml.html.document_fromstring` builds from the tokens of the emitter.
//!
//! The rules are libxml2 2.14.6's HTML parser (`HTMLparser.c`) over lxml's SAX2 tree: implied `html`,
//! `head` and `body`, start tags that close open elements, end tags that close up to their element,
//! misplaced `html`, `head` and `body` tags that are discarded, and a `/>` that closes whatever is open.
//! lxml keeps blank text (`keepBlanks`), so libxml2's `areBlanks` never decides anything and is not ported.

use std::sync::Arc;

use html5ever::{LocalName, local_name};

use super::ParseLimits;
use super::emitter::{Emitter, Token, tokens};
use super::tables::{BOOLEAN_ATTRIBUTES, element, end_priority, start_closes};
use crate::cancel::{CHECK_INTERVAL, Call};
use crate::dom::{NodeId, Ns, Tree, valueless};
use crate::error::{ExtractError, Rejection, TreeLimit};

/// libxml2's `htmlInsertMode`: how far the implied `head` and `body` have got. An implied element is
/// not inserted again once its stage has been passed.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Insert {
    Initial,
    InHead,
    InBody,
}

/// What the tree builder keeps while the tokens run: libxml2's open-element stack and counters, and the tree.
struct Builder {
    /// `None` until the first element, which is the root.
    tree: Option<Tree>,
    /// The open elements, outermost first, with their ids (`ctxt->nameTab`).
    open: Vec<(NodeId, LocalName)>,
    /// Misplaced `html`, `head` and `body` start tags discarded and not yet matched by an end tag (`ctxt->depth`).
    discarded: usize,
    insert: Insert,
    max_depth: usize,
    max_nodes: Option<usize>,
    /// The depth limit was crossed: the parse ends and the tree so far is kept.
    stopped: bool,
}

impl Builder {
    fn new(limits: &ParseLimits) -> Self {
        Self {
            tree: None,
            open: Vec::new(),
            discarded: 0,
            insert: Insert::Initial,
            max_depth: limits.max_depth,
            max_nodes: limits.max_nodes,
            stopped: false,
        }
    }

    fn top(&self) -> Option<NodeId> {
        self.open.last().map(|(id, _)| *id)
    }

    fn top_name(&self) -> Option<&str> {
        self.open.last().map(|(_, name)| &**name)
    }

    /// libxml2's `htmlnamePush` and the `startElement` SAX call: opens `name` under the current element,
    /// or as a document-level element when none is open.
    // libxml2: HTMLparser.c::htmlnamePush
    fn push(
        &mut self,
        name: &LocalName,
        attrs: Vec<(LocalName, Arc<str>)>,
    ) -> Result<(), ExtractError> {
        if self.stopped {
            return Ok(());
        }
        // The element would sit at depth `open + 1`; past the limit the parse ends (`docs/architecture.md`).
        if self.open.len() >= self.max_depth {
            self.stopped = true;
            return Ok(());
        }
        let parent = self.top();
        let id = match self.tree.as_mut() {
            Some(tree) => match parent {
                Some(parent) => tree.add_child(parent, name.clone(), Ns::Html, attrs),
                None => tree.add_document_root(name.clone(), Ns::Html, attrs),
            },
            None => {
                let tree = Tree::new(name.clone(), Ns::Html, attrs);
                let root = tree.root();
                self.tree = Some(tree);
                root
            }
        };
        if let Some(max) = self.max_nodes
            && self.tree.as_ref().is_some_and(|tree| tree.len() > max)
        {
            return Err(ExtractError::Rejected(Rejection::TreeTooLarge(
                TreeLimit::MaxNodes,
            )));
        }
        self.open.push((id, name.clone()));
        match &**name {
            "head" if self.insert < Insert::InHead => self.insert = Insert::InHead,
            "body" if self.insert < Insert::InBody => self.insert = Insert::InBody,
            _ => {}
        }
        Ok(())
    }

    /// The `endElement` SAX call and `htmlnamePop`: closes the current element.
    fn pop(&mut self) {
        self.open.pop();
    }

    /// Appends text at the end of the current element's content.
    fn add_text(&mut self, text: &str) {
        if self.stopped || text.is_empty() {
            return;
        }
        let Some(parent) = self.top() else { return };
        if let Some(tree) = self.tree.as_mut() {
            tree.add_text_owned(parent, text.to_owned());
        }
    }

    /// `htmlAutoClose`: closes the open elements that the start tag `new_tag` implicitly ends.
    // libxml2: HTMLparser.c::htmlAutoClose
    fn auto_close(&mut self, new_tag: &str) {
        while self
            .top_name()
            .is_some_and(|open| start_closes(new_tag, open))
        {
            self.pop();
        }
    }

    /// `htmlCheckImplied`: opens the implied `html`, `head` or `body` that a start tag `new_tag` needs.
    // libxml2: HTMLparser.c::htmlCheckImplied
    fn check_implied(&mut self, new_tag: &str) -> Result<(), ExtractError> {
        if new_tag == "html" {
            return Ok(());
        }
        if self.open.is_empty() {
            self.push(&local_name!("html"), Vec::new())?;
        }
        if matches!(new_tag, "body" | "head") {
            return Ok(());
        }
        if self.open.len() <= 1
            && matches!(
                new_tag,
                "script" | "style" | "meta" | "link" | "title" | "base"
            )
        {
            if self.insert >= Insert::InHead {
                return Ok(());
            }
            return self.push(&local_name!("head"), Vec::new());
        }
        if matches!(new_tag, "noframes" | "frame" | "frameset") {
            return Ok(());
        }
        if self.insert >= Insert::InBody
            || self
                .open
                .iter()
                .any(|(_, open)| matches!(&**open, "body" | "head"))
        {
            return Ok(());
        }
        self.push(&local_name!("body"), Vec::new())
    }

    /// `htmlStartCharData`: the implied elements that non-blank text needs.
    // libxml2: HTMLparser.c::htmlStartCharData
    fn start_char_data(&mut self) -> Result<(), ExtractError> {
        if self.top_name() == Some("head") {
            self.auto_close("p");
        }
        self.check_implied("p")
    }

    /// Character data. Blank text directly in `html` or `head` (or outside any element) is kept as
    /// text there (outside any element it is dropped); the rest opens the implied elements first.
    // libxml2: HTMLparser.c::htmlCharDataSAXCallback
    fn characters(&mut self, text: &str) -> Result<(), ExtractError> {
        if self.stopped {
            return Ok(());
        }
        let mut rest = text;
        if self
            .top_name()
            .is_none_or(|name| matches!(name, "html" | "head"))
        {
            let trimmed = text.trim_start_matches(is_html_space);
            if let Some((blank, tail)) = text.split_at_checked(text.len() - trimmed.len()) {
                self.add_text(blank);
                rest = tail;
            }
            if rest.is_empty() {
                return Ok(());
            }
            self.start_char_data()?;
        }
        self.add_text(rest);
        Ok(())
    }

    /// `htmlAutoCloseOnClose`: an end tag `new_tag` closes the open elements up to its own, unless an
    /// element of higher end priority sits in between.
    // libxml2: HTMLparser.c::htmlAutoCloseOnClose
    fn auto_close_on_close(&mut self, new_tag: &str) {
        let priority = end_priority(new_tag);
        let mut found = false;
        for (_, open) in self.open.iter().rev() {
            if &**open == new_tag {
                found = true;
                break;
            }
            if end_priority(open) > priority {
                return;
            }
        }
        if !found {
            return;
        }
        while self.top_name().is_some_and(|open| open != new_tag) {
            self.pop();
        }
    }

    /// `htmlParseEndTag`: an end tag closes the open elements up to its own element, unless one of higher
    /// end priority sits in between and the end tag is ignored. The end tag of a discarded `html`, `head`
    /// or `body` only ends the discard; one whose element is not open is dropped.
    // libxml2: HTMLparser.c::htmlParseEndTag
    fn end_tag(&mut self, name: &str) {
        if self.discarded > 0 && matches!(name, "html" | "body" | "head") {
            self.discarded -= 1;
            return;
        }
        if !self.open.iter().any(|(_, open)| &**open == name) {
            return;
        }
        self.auto_close_on_close(name);
        if self.top_name() == Some(name) {
            self.pop();
        }
    }

    /// `htmlParseStartTag` with `htmlParseElementInternal`'s end of tag. A start tag cut off by the end of
    /// input (`cut`) runs the implied-end and implied-start rules, then is discarded. A misplaced `html`,
    /// `head` or `body` is discarded and counted; its attributes are not merged. A `/>` closes the
    /// current element, which is the new one or, after a discarded tag, the one already open.
    // libxml2: HTMLparser.c::htmlParseStartTag
    // libxml2: HTMLparser.c::htmlParseElementInternal
    fn start(
        &mut self,
        name: &LocalName,
        attrs: Vec<(LocalName, Arc<str>)>,
        self_closing: bool,
        cut: bool,
    ) -> Result<(), ExtractError> {
        self.auto_close(name);
        self.check_implied(name)?;
        let misplaced = match &**name {
            "html" => usize::from(!self.open.is_empty()),
            "head" => usize::from(self.open.len() != 1),
            "body" => self
                .open
                .iter()
                .filter(|(_, open)| &**open == "body")
                .count(),
            _ => 0,
        };
        self.discarded = self.discarded.saturating_add(misplaced);
        if cut {
            return Ok(());
        }
        if misplaced == 0 {
            self.push(name, attrs)?;
        }
        // libxml2 pops an empty element at once (`htmlTagLookup`), so the `empty` column is read for the
        // top, which is the new element or, after a discarded tag, the one already open.
        let empty = self
            .top_name()
            .and_then(element)
            .is_some_and(|desc| desc.empty);
        if self_closing || empty {
            self.pop();
        }
        Ok(())
    }
}

/// libxml2's `IS_WS_HTML`: space, tab, line feed, form feed and carriage return.
// libxml2: HTMLparser.c::IS_WS_HTML
fn is_html_space(ch: char) -> bool {
    matches!(ch, ' ' | '\t' | '\n' | '\x0C' | '\r')
}

/// The attributes a kept start tag carries. A valueless attribute is its own name when `htmlBooleanAttrs`
/// lists it, else the shared valueless value (`valueless`), which the HTML method writes bare.
// libxml2: HTMLtree.c::htmlIsBooleanAttr
fn attributes(
    mut attrs: Vec<(LocalName, Arc<str>)>,
    valueless_names: &[LocalName],
) -> Vec<(LocalName, Arc<str>)> {
    for (name, value) in &mut attrs {
        if !valueless_names.contains(name) {
            continue;
        }
        *value = if BOOLEAN_ATTRIBUTES.contains(&&**name) {
            Arc::from(&**name)
        } else {
            valueless()
        };
    }
    attrs
}

/// What one build did, for the limit tests.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct BuildStats {
    /// Tokens the loop took from the emitter, the abort included.
    pub(super) tokens: usize,
    /// Tokens the emitter still held when the loop stopped on an abort. It queues none after its first
    /// limit, so this is 0.
    pub(super) tokens_after_abort: usize,
}

/// The tree of `html` as libxml2 2.14.6 builds it: the root is the first `html`, and the other
/// document-level elements are listed in `Tree::document_roots`. Fails with `EmptyTree` when no element
/// is built. Cancellation is checked every `CHECK_INTERVAL` tokens.
pub(super) fn build_document(
    html: &str,
    limits: &ParseLimits,
    call: &Call<'_>,
) -> Result<Tree, ExtractError> {
    build_with(html, limits, call).0
}

/// `build_document` with the counts of the run, which the error path keeps too.
pub(super) fn build_with(
    html: &str,
    limits: &ParseLimits,
    call: &Call<'_>,
) -> (Result<Tree, ExtractError>, BuildStats) {
    let mut stats = BuildStats::default();
    let result = run(html, limits, call, &mut stats);
    (result, stats)
}

fn run(
    html: &str,
    limits: &ParseLimits,
    call: &Call<'_>,
    stats: &mut BuildStats,
) -> Result<Tree, ExtractError> {
    call.check()?;
    let mut emitter = Emitter::new(limits, limits.drop_script_text);
    let mut builder = Builder::new(limits);
    let mut abort = None;
    for token in tokens(html, &mut emitter) {
        if stats.tokens.is_multiple_of(CHECK_INTERVAL) {
            call.check()?;
        }
        stats.tokens += 1;
        match token {
            Token::Start(tag) => {
                let attrs = attributes(tag.attrs, &tag.valueless);
                builder.start(&tag.name, attrs, tag.self_closing, false)?;
            }
            Token::CutStart(name) => builder.start(&name, Vec::new(), false, true)?,
            Token::End(name) => builder.end_tag(&name),
            // libxml2: HTMLparser.c::htmlParseCharData, case '\0'
            Token::Text(text) if text.contains('\0') => {
                builder.characters(&text.replace('\0', "\u{FFFD}"))?;
            }
            Token::Text(text) => builder.characters(&text)?,
            Token::Skipped => {}
            Token::Abort(error) => {
                abort = Some(error);
                break;
            }
        }
        if builder.stopped {
            break;
        }
    }
    if let Some(error) = abort {
        stats.tokens_after_abort = emitter.queued();
        return Err(error);
    }
    let dropped = emitter.dropped_text();
    let Some(mut tree) = builder.tree else {
        return Err(ExtractError::Rejected(Rejection::EmptyTree));
    };
    tree.dropped_script_text = dropped;
    Ok(tree)
}
