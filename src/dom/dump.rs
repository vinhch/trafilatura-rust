//! Tree dump: the text form in which the Python and Rust trees are compared.

use std::fmt::Write as _;
use std::sync::Arc;

use super::{LocalName, Node, NodeId, Ns, Tree};

// The XML 1.0 name productions.
pub(crate) fn is_name_start(c: char) -> bool {
    matches!(c,
        'A'..='Z' | 'a'..='z' | '_'
        | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}'
        | '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}' | '\u{200C}'..='\u{200D}'
        | '\u{2070}'..='\u{218F}' | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}'
        | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}' | '\u{10000}'..='\u{EFFFF}')
}

pub(crate) fn is_name_char(c: char) -> bool {
    is_name_start(c)
        || matches!(c, '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}')
}

/// Whether `name` is an XML Name without `:`.
pub(crate) fn is_valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(is_name_start) && chars.all(is_name_char)
}

#[cfg_attr(not(feature = "__internals"), allow(dead_code))]
fn push_json_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{C}' => out.push_str("\\f"),
            c if c < ' ' => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg_attr(not(feature = "__internals"), allow(dead_code))]
fn push_text_line(out: &mut String, depth: usize, text: Option<&str>) {
    let Some(text) = text.filter(|text| !text.is_empty()) else {
        return;
    };
    out.push_str(&"  ".repeat(depth));
    out.push_str("#text ");
    push_json_string(out, text);
    out.push('\n');
}

/// The `@svg`/`@math` tag of a foreign element: from its namespace in a tree that has any
/// namespaced element, else from the outermost `svg`/`math` ancestor-or-self (`inherited` is the
/// tag the parent passed down, read only when `plain`). Mirrors
/// `tools/parity/parity/treedump.py::dump_tree`.
#[cfg_attr(not(feature = "__internals"), allow(dead_code))]
fn foreign_tag(node: &Node, plain: bool, inherited: Option<&'static str>) -> Option<&'static str> {
    if !plain {
        return match node.ns {
            Ns::Html => None,
            Ns::Svg => Some("svg"),
            Ns::MathMl => Some("math"),
        };
    }
    inherited.or(match &*node.name {
        "svg" => Some("svg"),
        "math" => Some("math"),
        _ => None,
    })
}

/// True when no element of the subtree at `start` has a namespace.
#[cfg_attr(not(feature = "__internals"), allow(dead_code))]
fn is_plain(tree: &Tree, start: NodeId) -> bool {
    let mut stack = vec![start];
    while let Some(id) = stack.pop() {
        if tree.node(id).ns != Ns::Html {
            return false;
        }
        stack.extend(tree.children(id));
    }
    true
}

#[cfg_attr(not(feature = "__internals"), allow(dead_code))]
fn push_element_line(out: &mut String, depth: usize, node: &Node, foreign: Option<&str>) {
    out.push_str(&"  ".repeat(depth));
    out.push_str(&node.name);
    if let Some(tag) = foreign {
        out.push('@');
        out.push_str(tag);
    }
    let mut attrs: Vec<&(LocalName, Arc<str>)> = node.attrs.iter().collect();
    attrs.sort_by(|a, b| str::cmp(&a.0, &b.0));
    for (name, value) in attrs {
        out.push(' ');
        out.push_str(attribute_local_name(name));
        out.push('=');
        push_json_string(out, value);
    }
    out.push('\n');
}

/// The attribute name `treedump.py` writes: the local part of a Clark name `{ns}local`, empty when no
/// `}` closes it. A plain name is kept.
// Mirrors tools/parity/parity/treedump.py::_split, the harness rule.
fn attribute_local_name(name: &str) -> &str {
    match name.strip_prefix('{') {
        Some(rest) => rest.split_once('}').map_or("", |(_, local)| local),
        None => name,
    }
}

/// One line per node in document order, two spaces of indent per depth.
///
/// Element: `name[@svg|@math] key="value" ...` with attributes sorted by name and values as JSON
/// strings; the tag follows `treedump.py` (namespace, or `svg`/`math` ancestor-or-self when no
/// element has a namespace). Text and tail: `#text "..."` at the child depth (text) or the element's own depth
/// (tail); whitespace-only text is kept, empty text is omitted.
#[cfg_attr(not(feature = "__internals"), allow(dead_code))]
pub fn dump(tree: &Tree) -> String {
    dump_from(tree, tree.root())
}

/// `dump` of the subtree at `start`, which may be detached from the root (a copy that `copy` returned).
/// The element at `start` is the root of the dump, so its tail is left out.
#[cfg_attr(not(feature = "__internals"), allow(dead_code))]
pub fn dump_from(tree: &Tree, start: NodeId) -> String {
    let plain = is_plain(tree, start);
    let root = tree.node(start);
    let root_tag = foreign_tag(root, plain, None);
    let mut out = String::new();
    push_element_line(&mut out, 0, root, root_tag);
    push_text_line(&mut out, 1, root.text.as_deref());
    // (element, depth, next child, tag the element passes down)
    let mut stack: Vec<(NodeId, usize, Option<NodeId>, Option<&'static str>)> =
        vec![(start, 0, root.first_child, root_tag)];
    while let Some(frame) = stack.last_mut() {
        let (id, depth, next, inherited) = *frame;
        let Some(child_id) = next else {
            stack.pop();
            // The root has no tail; any other element's tail sits at its own depth.
            if depth > 0 {
                push_text_line(&mut out, depth, tree.node(id).tail.as_deref());
            }
            continue;
        };
        let child = tree.node(child_id);
        frame.2 = child.next;
        let child_tag = foreign_tag(child, plain, inherited);
        push_element_line(&mut out, depth.saturating_add(1), child, child_tag);
        push_text_line(&mut out, depth.saturating_add(2), child.text.as_deref());
        stack.push((
            child_id,
            depth.saturating_add(1),
            child.first_child,
            child_tag,
        ));
    }
    out
}
