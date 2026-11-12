//! Serializers in the forms of lxml's `etree.tostring` (the XML method) and `lxml.html.tostring` (the
//! HTML method) over the arena, each written with one explicit stack so the call stack never grows
//! with the depth of the tree.

use std::borrow::Cow;
use std::sync::LazyLock;

use regex::Regex;

use super::parse::tables::BOOLEAN_ATTRIBUTES;
use super::{Node, NodeId, Ns, Tree};

/// libxml2 stops indenting after this many levels (two spaces each).
const MAX_INDENT_LEVEL: usize = 30;

/// Void elements of libxml2's HTML table: no end tag, and no content written.
const VOID_ELEMENTS: &[&str] = &[
    "area", "base", "basefont", "br", "col", "frame", "hr", "img", "input", "isindex", "link",
    "meta", "param",
];

const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";
const MATHML_NAMESPACE: &str = "http://www.w3.org/1998/Math/MathML";

/// lxml's `__str_replace_meta_content_type`, run on the whole output: case-sensitive, and `[^>]`
/// crosses newlines.
static CONTENT_TYPE_META: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r#"<meta http-equiv="Content-Type"[^>]*>"#).ok());

/// Which of lxml's two serializers the walk follows.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Method {
    /// `etree.tostring`: local names, text escaped as XML.
    Xml,
    /// `etree.tostring(method="html")`: libxml2's HTML output.
    Html,
}

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

/// Text and tail in the HTML method: only `&`, `<` and `>` are escaped.
fn push_html_text(out: &mut String, text: &str) {
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

fn push_line_start(out: &mut String, level: usize) {
    out.push('\n');
    out.extend(std::iter::repeat_n(' ', level.min(MAX_INDENT_LEVEL) * 2));
}

/// The namespace libxml2 declares for a foreign element; `None` for an HTML element.
fn namespace_uri(ns: Ns) -> Option<&'static str> {
    match ns {
        Ns::Html => None,
        Ns::Svg => Some(SVG_NAMESPACE),
        Ns::MathMl => Some(MATHML_NAMESPACE),
    }
}

/// The element name as the method writes it. The HTML method prefixes a foreign element with `ns0`.
fn push_name(out: &mut String, node: &Node, method: Method) {
    if method == Method::Html && node.ns != Ns::Html {
        out.push_str("ns0:");
    }
    out.push_str(&node.name); // deviation: D-2
}

/// Void elements and an empty `li` have no end tag in the HTML method. Only HTML elements count.
fn is_bare_html(node: &Node, empty: bool) -> bool {
    let name: &str = &node.name;
    node.ns == Ns::Html && (VOID_ELEMENTS.contains(&name) || (empty && name == "li"))
}

/// An element whose text is raw in the HTML method, whatever its namespace (libxml2 compares the name
/// without case).
fn is_raw_text_element(name: &str) -> bool {
    name.eq_ignore_ascii_case("script") || name.eq_ignore_ascii_case("style")
}

/// The text of `node`. In the HTML method the text of a raw text element is raw.
fn push_text(out: &mut String, node: &Node, method: Method) {
    let text = node.text.as_deref().unwrap_or("");
    match method {
        Method::Xml => push_escaped(out, text, false),
        Method::Html if is_raw_text_element(&node.name) => out.push_str(text),
        Method::Html => push_html_text(out, text),
    }
}

/// A URI attribute value in the HTML method: leading blanks are dropped, then every byte outside
/// `0x21..=0x7E` is written as `%XX`.
fn uri_escape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for &byte in value.trim_start_matches([' ', '\t', '\n', '\r']).as_bytes() {
        if matches!(byte, 0x21..=0x7E) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Whether the `&` at `i` starts `&{` with a `}` after the brace. libxml2 writes that `&` raw.
fn brace_follows(bytes: &[u8], i: usize, last_brace: Option<usize>) -> bool {
    let brace = i.saturating_add(1);
    bytes.get(brace) == Some(&b'{') && last_brace.is_some_and(|last| last > brace)
}

/// The byte index just past the `-->` that closes the `<!--` at `start`, if one follows. The search
/// starts at the third byte, so `<!-->` closes itself (oracle rows `<!-->`, `<!--->`, `<!-->x-->`).
fn comment_end(bytes: &[u8], start: usize, last_closer: Option<usize>) -> Option<usize> {
    if !bytes.get(start..)?.starts_with(b"<!--") {
        return None;
    }
    let from = start.saturating_add(2);
    if !last_closer.is_some_and(|closer| closer >= from) {
        return None;
    }
    let found = bytes
        .get(from..)?
        .windows(3)
        .position(|window| window == b"-->")?;
    Some(from.saturating_add(found).saturating_add(3))
}

/// The quoted value of an attribute in the HTML method. `<` and `>` are escaped except inside a
/// comment region, where `&`, `<` and `>` stay raw; `"` is escaped when the quote is `"`.
fn push_attribute_value(out: &mut String, value: &str, quote: char) {
    let bytes = value.as_bytes();
    let last_brace = bytes.iter().rposition(|&byte| byte == b'}');
    let last_closer = bytes.windows(3).rposition(|window| window == b"-->");
    // Bytes before this index sit inside the comment region that the last `<!--` opened.
    let mut raw_until = 0;
    for (i, c) in value.char_indices() {
        let raw = i < raw_until;
        match c {
            '"' if quote == '"' => out.push_str("&quot;"),
            '&' if raw || brace_follows(bytes, i, last_brace) => out.push('&'),
            '&' => out.push_str("&amp;"),
            '<' if raw => out.push('<'),
            '<' => match comment_end(bytes, i, last_closer) {
                Some(end) => {
                    raw_until = end;
                    out.push('<');
                }
                None => out.push_str("&lt;"),
            },
            '>' if raw => out.push('>'),
            '>' => out.push_str("&gt;"),
            c => out.push(c),
        }
    }
}

/// One attribute in the HTML method: a boolean name is written bare. Any other value is quoted with
/// `'` when it holds `"` and no `'`, and is written by `push_attribute_value`.
fn push_html_attribute(out: &mut String, element: &Node, name: &str, value: &str) {
    out.push(' ');
    out.push_str(name);
    if BOOLEAN_ATTRIBUTES
        .iter()
        .any(|flag| name.eq_ignore_ascii_case(flag))
    {
        return;
    }
    let uri = element.ns == Ns::Html
        && (["href", "action", "src"]
            .iter()
            .any(|attr| name.eq_ignore_ascii_case(attr))
            || (name.eq_ignore_ascii_case("name") && &*element.name == "a"));
    let value = if uri {
        Cow::Owned(uri_escape(value))
    } else {
        Cow::Borrowed(value)
    };
    let quote = if value.contains('"') && !value.contains('\'') {
        '\''
    } else {
        '"'
    };
    out.push('=');
    out.push(quote);
    push_attribute_value(out, &value, quote);
    out.push(quote);
}

impl Tree {
    /// `etree.tostring(id, encoding="unicode", pretty_print=pretty_print, with_tail=with_tail)`.
    /// Elements are written under their local name, without namespace or prefix. With
    /// `pretty_print` the root is followed by a line break.
    // upstream: lxml etree.tostring (pretty_print)
    pub fn tostring(&self, id: NodeId, pretty_print: bool, with_tail: bool) -> String {
        self.serialize(id, Method::Xml, pretty_print, with_tail)
    }

    /// `lxml.html.tostring(e, encoding="unicode", with_tail=with_tail)`: the HTML method of
    /// libxml2 for `id`, then every `<meta http-equiv="Content-Type">` is removed from the output.
    // upstream: lxml html/__init__.py::tostring
    pub fn tostring_html(&self, id: NodeId, with_tail: bool) -> String {
        let html = self.serialize(id, Method::Html, false, with_tail);
        match CONTENT_TYPE_META.as_ref() {
            Some(pattern) => pattern.replace_all(&html, "").into_owned(),
            None => html,
        }
    }

    /// The walk both methods share, with one explicit stack.
    fn serialize(&self, id: NodeId, method: Method, pretty_print: bool, with_tail: bool) -> String {
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
                    self.push_start_tag(&mut out, node_id, method);
                    // `<x/>` only without any text node: `Some("")` gives `<x></x>`.
                    let empty = node.text.is_none() && self.first_child(node_id).is_none();
                    if method == Method::Xml && empty {
                        out.push_str("/>");
                        self.push_tail(&mut out, node_id, level, with_tail, method);
                        continue;
                    }
                    out.push('>');
                    if method == Method::Html && is_bare_html(node, empty) {
                        self.push_tail(&mut out, node_id, level, with_tail, method);
                        continue;
                    }
                    push_text(&mut out, node, method);
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
                    push_name(&mut out, self.node(node_id), method);
                    out.push('>');
                    self.push_tail(&mut out, node_id, level, with_tail, method);
                }
            }
        }
        if pretty_print {
            out.push('\n');
        }
        out
    }

    /// `<name` and the attributes of `id`. The HTML method declares the namespace on the outermost
    /// element of a foreign subtree, before its attributes.
    fn push_start_tag(&self, out: &mut String, id: NodeId, method: Method) {
        let node = self.node(id);
        out.push('<');
        push_name(out, node, method);
        let foreign = match method {
            Method::Html => namespace_uri(node.ns),
            Method::Xml => None,
        };
        let parent_ns = node.parent().map(|parent| self.node(parent).ns);
        if let Some(uri) = foreign.filter(|_| parent_ns != Some(node.ns)) {
            out.push_str(" xmlns:ns0=\"");
            out.push_str(uri);
            out.push('"');
        }
        for (name, value) in &node.attrs {
            match method {
                Method::Xml => {
                    out.push(' ');
                    out.push_str(name);
                    out.push_str("=\"");
                    push_escaped(out, value, true);
                    out.push('"');
                }
                Method::Html => push_html_attribute(out, node, name, value),
            }
        }
    }

    /// The tail of `id`: always for a descendant (`level` > 0), only with `with_tail` for the element
    /// the walk started at. A tail belongs to its parent, so in the HTML method it is raw when that
    /// parent is a raw text element.
    fn push_tail(
        &self,
        out: &mut String,
        id: NodeId,
        level: usize,
        with_tail: bool,
        method: Method,
    ) {
        if level > 0 || with_tail {
            let node = self.node(id);
            let tail = node.tail.as_deref().unwrap_or("");
            let raw_parent = node
                .parent()
                .is_some_and(|parent| is_raw_text_element(&self.node(parent).name));
            match method {
                Method::Xml => push_escaped(out, tail, false),
                Method::Html if raw_parent => out.push_str(tail),
                Method::Html => push_html_text(out, tail),
            }
        }
    }
}
