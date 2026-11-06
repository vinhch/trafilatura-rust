//! The XML output of upstream's `control_xml_output`: the tree built from `doc` and `body`, the
//! sanitizing, and the blank pass. The blank pass is the text that libxml2 drops when the serialized
//! tree is parsed again with `remove_blank_text`. No parser runs. The reparse input is this crate's
//! own serialization, so the pass applies libxml2's `areBlanks` rules to each text and tail piece as
//! the parser would see it: a piece is split into the chunks the parser delivers, and a blank chunk
//! is dropped unless one of the rules keeps it.

use std::collections::HashMap;
use std::iter::Peekable;
use std::str::Chars;

use super::{
    build_tei_output, delete_element, keeps_empty, meta_items, remove_empty_elements,
    strip_double_tags,
};
use crate::cancel::Call;
use crate::dom::{NodeId, Tree};
use crate::error::ExtractError;
use crate::html_processing::tick;
use crate::pystr::strip;
use crate::settings::{Document, Extractor, Format};
use crate::utils::sanitize_tree;

/// The tags whose elements keep their attributes in the XML output; `clean_attributes` clears the
/// rest.
// upstream: xml.py::WITH_ATTRIBUTES
pub const WITH_ATTRIBUTES: &[&str] = &[
    "cell", "row", "del", "graphic", "head", "hi", "item", "list", "ref",
];

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

/// Removes, in document order, the elements of the subtree at `body` that have no child, a text and
/// a tail that are both `None` or empty, and that `keeps_empty` does not protect. Each one goes with
/// `keep_tail=false`. A space is text, so it keeps its element.
///
/// Checks `call` about every `CHECK_INTERVAL` nodes.
// upstream: core.py::determine_returnstring (the last cleaning loop before `control_xml_output`)
pub fn drop_empty_body_elements(
    tree: &mut Tree,
    body: NodeId,
    call: &Call<'_>,
) -> Result<(), ExtractError> {
    let nodes: Vec<NodeId> = tree.iter(body, &[]).collect();
    let mut rows = HashMap::new();
    let mut visited = 0;
    for element in nodes {
        tick(&mut visited, call)?;
        let node = tree.node(element);
        let empty = tree.first_child(element).is_none()
            && node.text.as_deref().is_none_or(str::is_empty)
            && node.tail.as_deref().is_none_or(str::is_empty);
        if empty && !keeps_empty(tree, element, &mut rows) {
            delete_element(tree, element, false);
        }
    }
    Ok(())
}

/// Clears the attributes of each element of the subtree at `root` whose tag is not in
/// `WITH_ATTRIBUTES`. Returns `root`.
///
/// Checks `call` about every `CHECK_INTERVAL` nodes.
// upstream: xml.py::clean_attributes
pub(super) fn clean_attributes(
    tree: &mut Tree,
    root: NodeId,
    call: &Call<'_>,
) -> Result<NodeId, ExtractError> {
    let nodes: Vec<NodeId> = tree.iter(root, &[]).collect();
    let mut visited = 0;
    for element in nodes {
        tick(&mut visited, call)?;
        if !tree
            .tag(element)
            .is_some_and(|tag| WITH_ATTRIBUTES.contains(&tag))
        {
            tree.clear_attrs(element);
        }
    }
    Ok(root)
}

/// Sets each metadata item of `items` as an attribute of `output`.
// upstream: xml.py::add_xml_meta
fn add_xml_meta(tree: &mut Tree, output: NodeId, items: &[(&'static str, String)]) {
    for (attribute, value) in items {
        tree.set(output, attribute, value);
    }
}

/// The `doc` tree of `doc`: its metadata as attributes, then the body renamed `main` and the
/// comments renamed `comments`, each with `clean_attributes`. Returns the `doc` root.
///
/// Checks `call` through `clean_attributes`.
// upstream: xml.py::build_xml_output
fn build_xml_output(doc: &mut Document, call: &Call<'_>) -> Result<NodeId, ExtractError> {
    let items = meta_items(doc);
    let (body, comments) = (doc.body, doc.comments_body);
    let tree = &mut doc.tree;
    let output = tree.new_element("doc");
    add_xml_meta(tree, output, &items);
    tree.set_name(body, "main");
    clean_attributes(tree, body, call)?;
    tree.append(output, body);
    tree.set_name(comments, "comments");
    clean_attributes(tree, comments, call)?;
    tree.append(output, comments);
    Ok(output)
}

/// The XML output of `doc`: the `doc` tree of `build_xml_output` (or the TEI tree of
/// `build_tei_output` for the XML-TEI format), sanitized, reparsed without the blank text libxml2
/// drops, pretty printed and stripped.
///
/// Checks `call` in each step; see the functions it runs.
// upstream: xml.py::control_xml_output
pub fn control_xml_output(
    doc: &mut Document,
    options: &Extractor,
    call: &Call<'_>,
) -> Result<String, ExtractError> {
    let body = doc.body;
    strip_double_tags(&mut doc.tree, body, call)?;
    remove_empty_elements(&mut doc.tree, body, call)?;
    let output = if options.format == Format::XmlTei {
        build_tei_output(doc, call)?
    } else {
        build_xml_output(doc, call)?
    };
    sanitize_tree(&mut doc.tree, output, call)?;
    remove_blank_text(&mut doc.tree, output, call)?;
    Ok(strip(&doc.tree.tostring(output, true, false)).to_owned())
}
