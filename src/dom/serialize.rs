//! Serializer in the form of lxml's `etree.tostring`: the XML method over the arena, written with
//! one explicit stack so the call stack never grows with the depth of the tree.

use super::{NodeId, Tree};

/// libxml2 stops indenting after this many levels (two spaces each).
const MAX_INDENT_LEVEL: usize = 30;

/// One pending step of the walk.
enum Step {
    /// Write the element, preceded by a line break and its indent when `newline` is set.
    Open {
        id: NodeId,
        level: usize,
        newline: bool,
        format: bool,
    },
    /// Write the end tag, preceded by a line break and its indent when `newline` is set.
    Close {
        id: NodeId,
        level: usize,
        newline: bool,
    },
}

/// `text` as libxml2 writes it: `&`, `<`, `>` and `\r` become references, and in an attribute
/// value `"`, `\n` and `\t` too.
fn push_escaped(out: &mut String, text: &str, attribute: bool) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\r' => out.push_str("&#13;"),
            '"' if attribute => out.push_str("&quot;"),
            '\n' if attribute => out.push_str("&#10;"),
            '\t' if attribute => out.push_str("&#9;"),
            c => out.push(c),
        }
    }
}

fn push_line_start(out: &mut String, level: usize) {
    out.push('\n');
    out.extend(std::iter::repeat_n(' ', level.min(MAX_INDENT_LEVEL) * 2));
}

impl Tree {
    /// `etree.tostring(id, encoding="unicode", pretty_print=pretty_print, with_tail=with_tail)`.
    /// Elements are written under their local name, without namespace or prefix. With
    /// `pretty_print` the root is followed by a line break.
    pub fn tostring(&self, id: NodeId, pretty_print: bool, with_tail: bool) -> String {
        // upstream: lxml etree.tostring (pretty_print)
        let mut out = String::new();
        let mut stack = vec![Step::Open {
            id,
            level: 0,
            newline: false,
            format: pretty_print,
        }];
        while let Some(step) = stack.pop() {
            match step {
                Step::Open {
                    id: node_id,
                    level,
                    newline,
                    format,
                } => {
                    if newline {
                        push_line_start(&mut out, level);
                    }
                    let node = self.node(node_id);
                    out.push('<');
                    out.push_str(&node.name); // deviation: D-2
                    for (name, value) in &node.attrs {
                        out.push(' ');
                        out.push_str(name);
                        out.push_str("=\"");
                        push_escaped(&mut out, value, true);
                        out.push('"');
                    }
                    // `<x/>` only without any text node: `Some("")` gives `<x></x>`.
                    if node.text.is_none() && self.first_child(node_id).is_none() {
                        out.push_str("/>");
                        self.push_tail(&mut out, node_id, level, with_tail);
                        continue;
                    }
                    out.push('>');
                    push_escaped(&mut out, node.text.as_deref().unwrap_or(""), false);
                    // Any text node among the children turns formatting off for the subtree.
                    let child_format = format
                        && node.text.is_none()
                        && self
                            .children(node_id)
                            .all(|child| self.node(child).tail.is_none());
                    let has_children = self.first_child(node_id).is_some();
                    stack.push(Step::Close {
                        id: node_id,
                        level,
                        newline: child_format && has_children,
                    });
                    let child_level = level.saturating_add(1);
                    let mut child = self.last_child(node_id);
                    while let Some(child_id) = child {
                        stack.push(Step::Open {
                            id: child_id,
                            level: child_level,
                            newline: child_format,
                            format: child_format,
                        });
                        child = self.node(child_id).prev_sibling();
                    }
                }
                Step::Close {
                    id: node_id,
                    level,
                    newline,
                } => {
                    if newline {
                        push_line_start(&mut out, level);
                    }
                    out.push_str("</");
                    out.push_str(&self.node(node_id).name); // deviation: D-2
                    out.push('>');
                    self.push_tail(&mut out, node_id, level, with_tail);
                }
            }
        }
        if pretty_print {
            out.push('\n');
        }
        out
    }

    /// The escaped tail of `id`: always for a descendant (`level` > 0), only with `with_tail` for
    /// the element `tostring` was called on.
    fn push_tail(&self, out: &mut String, id: NodeId, level: usize, with_tail: bool) {
        if level > 0 || with_tail {
            push_escaped(out, self.node(id).tail.as_deref().unwrap_or(""), false);
        }
    }
}
