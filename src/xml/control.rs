//! The blank pass of upstream's `control_xml_output`: the text that libxml2 drops when the serialized
//! tree is parsed again with `remove_blank_text`. No parser runs. The reparse input is this crate's
//! own serialization, so the pass applies libxml2's `areBlanks` rules to each text and tail piece as
//! the parser would see it: a piece is split into the chunks the parser delivers, and a blank chunk
//! is dropped unless one of the rules keeps it.

use std::iter::Peekable;
use std::str::Chars;

use crate::cancel::Call;
use crate::dom::{NodeId, Tree};
use crate::error::ExtractError;
use crate::html_processing::tick;

/// What the pass knows about the element whose content it reads: the nodes libxml2 would have
/// built under it by now, and whether a chunk was delivered as text in it.
struct Frame {
    id: NodeId,
    /// Next child element still to visit, in document order.
    next_child: Option<NodeId>,
    /// A chunk was delivered as text: later blank chunks in the element are kept. A child always
    /// starts unsticky, since the sticky state never passes from parent to child.
    sticky: bool,
    has_child: bool,
    first_is_text: bool,
    last_is_text: bool,
}

impl Frame {
    fn new(id: NodeId, first_child: Option<NodeId>) -> Self {
        Self {
            id,
            next_child: first_child,
            sticky: false,
            has_child: false,
            first_is_text: false,
            last_is_text: false,
        }
    }

    /// Records a node the parser appends under the element.
    fn note_node(&mut self, is_text: bool) {
        if !self.has_child {
            self.has_child = true;
            self.first_is_text = is_text;
        }
        self.last_is_text = is_text;
    }
}

/// Removes from the subtree at `root` the blank text that libxml2 drops when the output is parsed
/// again, and turns every empty text or tail into `None`. Text the parser would keep stays as it is.
///
/// Checks `call` about every `CHECK_INTERVAL` nodes.
// upstream: xml.py::control_xml_output (the CONTROL_PARSER reparse; libxml2 parser.c::areBlanks)
pub fn remove_blank_text(
    tree: &mut Tree,
    root: NodeId,
    call: &Call<'_>,
) -> Result<(), ExtractError> {
    let mut visited = 0;
    tick(&mut visited, call)?;
    let mut stack = vec![open(tree, root)];
    while let Some(frame) = stack.last_mut() {
        match frame.next_child {
            Some(child) => {
                frame.next_child = tree.node(child).next_sibling();
                frame.note_node(false);
                tick(&mut visited, call)?;
                stack.push(open(tree, child));
            }
            None => {
                let Some(done) = stack.pop() else { break };
                // a tail follows its element's end tag and belongs to the parent's content
                if let Some(parent) = stack.last_mut() {
                    let end_tag = tree.node(done.id).next_sibling().is_none();
                    clean_piece(&mut tree.node_mut(done.id).tail, end_tag, parent);
                }
            }
        }
    }
    let root_tail = &mut tree.node_mut(root).tail;
    if root_tail.as_deref() == Some("") {
        *root_tail = None;
    }
    Ok(())
}

/// Starts the element `id` and cleans its text, which the parser reads before the first child.
fn open(tree: &mut Tree, id: NodeId) -> Frame {
    let first_child = tree.first_child(id);
    let mut frame = Frame::new(id, first_child);
    clean_piece(
        &mut tree.node_mut(id).text,
        first_child.is_none(),
        &mut frame,
    );
    frame
}

/// Cleans one text or tail piece in place. `end_tag` tells whether the parser meets the parent's end
/// tag after the piece, rather than a child's start tag. An empty piece, or one whose chunks are all
/// dropped, becomes `None`.
fn clean_piece(slot: &mut Option<String>, end_tag: bool, frame: &mut Frame) {
    let Some(text) = slot.take() else { return };
    let kept = keep_chunks(&text, end_tag, frame);
    *slot = (!kept.is_empty()).then_some(kept);
}

/// The piece with its dropped blank chunks removed, feeding each chunk and reference into `frame`
/// in the order the parser reads them.
fn keep_chunks(text: &str, end_tag: bool, frame: &mut Frame) -> String {
    let mut kept = String::with_capacity(text.len());
    let mut chars: Peekable<Chars<'_>> = text.chars().peekable();
    while let Some(first) = chars.next() {
        // `&`, `<`, `>` and CR reach the parser as references (the serializer escapes them). The
        // parser delivers a reference as text but never drops it.
        if is_reference(first) {
            frame.note_node(true);
            kept.push(first);
            continue;
        }
        // The parser delivers a run of ASCII text up to the next reference or non-ASCII byte as one
        // chunk. A run that starts with a non-ASCII byte is never blank.
        let chunk_start = kept.len();
        let ascii = first.is_ascii();
        let mut blank = ascii && is_blank(first);
        kept.push(first);
        while let Some(&next) = chars.peek() {
            if is_reference(next) || (ascii && !next.is_ascii()) {
                break;
            }
            chars.next();
            blank &= is_blank(next);
            kept.push(next);
        }
        let is_last = chars.peek().is_none();
        if blank && !keeps_blank(frame, is_last, end_tag) {
            kept.truncate(chunk_start);
        } else {
            frame.note_node(true);
            frame.sticky = true;
        }
    }
    kept
}

/// Whether libxml2's `areBlanks` keeps a blank chunk. A chunk followed by a reference or a non-ASCII
/// byte is kept, because only markup after it can make it ignorable. Otherwise the chunk is kept
/// when the element is sticky, when it has no child yet and meets its end tag, or when its first or
/// last child is text.
fn keeps_blank(frame: &Frame, is_last: bool, end_tag: bool) -> bool {
    !is_last
        || frame.sticky
        || (!frame.has_child && end_tag)
        || frame.first_is_text
        || frame.last_is_text
}

/// The characters the serializer writes as references: `&`, `<`, `>` and CR.
fn is_reference(c: char) -> bool {
    matches!(c, '&' | '<' | '>' | '\r')
}

/// The characters libxml2's `IS_BLANK_CH` accepts. A chunk made only of them is blank.
fn is_blank(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}
