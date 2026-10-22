//! Tree dump: the text form in which the Python and Rust trees are compared.

use std::fmt::Write as _;
use std::sync::Arc;

use super::{LocalName, Node, NodeId, Ns, Tree};

/// Which tree the dump describes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DumpMode {
    /// The tree as parsed.
    Exact,
    /// The tree after the parity harness copy rule: invalid names and characters dropped.
    Normalized,
}

// The XML 1.0 name productions, as in tools/parity/parity/html5tree.py.
fn is_name_start(c: char) -> bool {
    matches!(c,
        'A'..='Z' | 'a'..='z' | '_'
        | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}'
        | '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}' | '\u{200C}'..='\u{200D}'
        | '\u{2070}'..='\u{218F}' | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}'
        | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}' | '\u{10000}'..='\u{EFFFF}')
}

fn is_name_char(c: char) -> bool {
    is_name_start(c)
        || matches!(c, '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}')
}

/// Element names may contain `:` (libxml2 keeps `o:p`); attribute names may not.
fn is_valid_name(name: &str, allow_colon: bool) -> bool {
    let colon_ok = |c: char| allow_colon && c == ':';
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| is_name_start(c) || colon_ok(c))
        && chars.all(|c| is_name_char(c) || colon_ok(c))
}

/// What lxml refuses in text and attribute values (upstream `utils.INVALID_XML_CHARS`).
fn is_invalid_char(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{8}' | '\u{B}' | '\u{C}' | '\u{E}'..='\u{1F}' | '\u{FFFE}' | '\u{FFFF}')
}

/// `text` without invalid characters; sets `changed` when any was removed.
fn strip_invalid(text: &str, changed: &mut bool) -> String {
    let cleaned: String = text.chars().filter(|&c| !is_invalid_char(c)).collect();
    *changed |= cleaned.len() != text.len();
    cleaned
}

fn copy_attrs(node: &Node, changed: &mut bool) -> Vec<(LocalName, Arc<str>)> {
    let mut attrs: Vec<(LocalName, Arc<str>)> = Vec::new();
    for (name, value) in &node.attrs {
        // An invalid name, or `href` next to `xlink:href` once namespaces are reduced: first wins.
        if !is_valid_name(name, false) || attrs.iter().any(|(kept, _)| kept == name) {
            *changed = true;
        } else {
            attrs.push((name.clone(), Arc::from(strip_invalid(value, changed))));
        }
    }
    attrs
}

/// Copies `source` as the parity harness copies html5lib trees into lxml: attributes with invalid
/// names are dropped, an element with an invalid name is replaced by its content, invalid
/// characters are stripped. Returns the copy and whether anything changed.
fn normalize(source: &Tree) -> (Tree, bool) {
    let mut changed = false;
    let src_root = source.root();
    let root = source.node(src_root);
    let mut out = Tree::new(root.name.clone(), root.ns, copy_attrs(root, &mut changed));
    let out_root = out.root();
    if let Some(text) = &root.text {
        out.add_text(out_root, &strip_invalid(text, &mut changed));
    }
    // (source element, output element receiving its children, next source child)
    let mut stack = vec![(src_root, out_root, root.first_child)];
    while let Some(frame) = stack.last_mut() {
        let (src_id, _, next) = *frame;
        let Some(child_id) = next else {
            stack.pop();
            // The tail follows the finished element in the enclosing output element.
            if let (Some(&(_, enclosing, _)), Some(tail)) =
                (stack.last(), &source.node(src_id).tail)
            {
                out.add_text(enclosing, &strip_invalid(tail, &mut changed));
            }
            continue;
        };
        let parent_out = frame.1;
        let child = source.node(child_id);
        frame.2 = child.next;
        let child_out = if is_valid_name(&child.name, true) {
            out.add_child(
                parent_out,
                child.name.clone(),
                child.ns,
                copy_attrs(child, &mut changed),
            )
        } else {
            // Not representable as an lxml tag: its content replaces it.
            changed = true;
            parent_out
        };
        if let Some(text) = &child.text {
            out.add_text(child_out, &strip_invalid(text, &mut changed));
        }
        stack.push((child_id, child_out, child.first_child));
    }
    (out, changed)
}

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

fn push_text_line(out: &mut String, depth: usize, text: Option<&str>) {
    let Some(text) = text.filter(|text| !text.is_empty()) else {
        return;
    };
    out.push_str(&"  ".repeat(depth));
    out.push_str("#text ");
    push_json_string(out, text);
    out.push('\n');
}

fn push_element_line(out: &mut String, depth: usize, node: &Node) {
    out.push_str(&"  ".repeat(depth));
    out.push_str(&node.name);
    match node.ns {
        Ns::Html => {}
        Ns::Svg => out.push_str("@svg"),
        Ns::MathMl => out.push_str("@math"),
    }
    let mut attrs: Vec<&(LocalName, Arc<str>)> = node.attrs.iter().collect();
    attrs.sort_by(|a, b| str::cmp(&a.0, &b.0));
    for (name, value) in attrs {
        out.push(' ');
        out.push_str(name);
        out.push('=');
        push_json_string(out, value);
    }
    out.push('\n');
}

/// One line per node in document order, two spaces of indent per depth.
///
/// Element: `name[@svg|@math] key="value" ...` with attributes sorted by name and values as JSON
/// strings. Text and tail: `#text "..."` at the child depth (text) or the element's own depth
/// (tail); whitespace-only text is kept, empty text is omitted. The flag is true when the copy rule
/// of `DumpMode::Normalized` changed the tree; it is always false for `Exact`.
pub fn dump(tree: &Tree, mode: DumpMode) -> (String, bool) {
    let (copy, changed) = match mode {
        DumpMode::Exact => (None, false),
        DumpMode::Normalized => {
            let (copy, changed) = normalize(tree);
            (Some(copy), changed)
        }
    };
    let tree = copy.as_ref().unwrap_or(tree);
    let root = tree.node(tree.root());
    let mut out = String::new();
    push_element_line(&mut out, 0, root);
    push_text_line(&mut out, 1, root.text.as_deref());
    // (element, depth, next child)
    let mut stack: Vec<(NodeId, usize, Option<NodeId>)> = vec![(tree.root(), 0, root.first_child)];
    while let Some(frame) = stack.last_mut() {
        let (id, depth, next) = *frame;
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
        push_element_line(&mut out, depth.saturating_add(1), child);
        push_text_line(&mut out, depth.saturating_add(2), child.text.as_deref());
        stack.push((child_id, depth.saturating_add(1), child.first_child));
    }
    (out, changed)
}
