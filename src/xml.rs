//! The part of upstream `xml.py` that serves the plain-text and Markdown output: removing and merging
//! elements, the empty-element pass, the element text and `xml_to_text`. The XML, TEI, JSON and CSV
//! builders and the TEI checks are not ported.

use crate::cancel::{CHECK_INTERVAL, Call};
use crate::dom::{LiveIter, LocalName, NodeId, Ns, Tree};
use crate::error::ExtractError;
use crate::html_processing::tick;
use crate::pyhtml::unescape;
use crate::pystr::{is_all_space, is_space, lstrip, rstrip, strip};
use crate::settings::{INLINE_CARRIED, INLINE_CONSUMING, INLINE_FORMATTABLE};
use crate::utils::{sanitize, text_chars_test};

use self::markdown::{
    code_fence, escape_and_convert_math, escape_block_start, escape_cell, escape_inline_specials,
    md_code, md_wrap,
};

/// The elements that start on a line of their own.
// upstream: xml.py::NEWLINE_ELEMS
pub(crate) const NEWLINE_ELEMS: [&str; 8] = [
    "graphic", "head", "lb", "list", "p", "quote", "row", "table",
];

/// The elements whose text carries its own spacing, so no separator follows them.
// upstream: xml.py::SPECIAL_FORMATTING
pub(crate) const SPECIAL_FORMATTING: [&str; 7] =
    ["code", "del", "head", "hi", "ref", "item", "cell"];

/// The `hi` renditions rendered as Markdown, with their markers.
// upstream: xml.py::HI_FORMATTING
pub(crate) const HI_FORMATTING: [(&str, &str); 3] = [("#b", "**"), ("#i", "*"), ("#t", "`")];

/// The `hi` renditions with no Markdown syntax, rendered as inline HTML tags.
// upstream: xml.py::HI_HTML_TAGS
pub(crate) const HI_HTML_TAGS: [(&str, &str); 3] = [("#sup", "sup"), ("#sub", "sub"), ("#u", "u")];

/// The heading levels a `head` `rend` may name after its first character.
// upstream: xml.py::HEADING_LEVELS
pub(crate) const HEADING_LEVELS: [char; 6] = ['1', '2', '3', '4', '5', '6'];

/// The `hi` renditions that `merge_adjacent_hi` joins: `HI_FORMATTING` and `#u`. Adjacent `sup`
/// and `sub` stay apart, as they are often distinct footnote references.
// upstream: xml.py::MERGEABLE_RENDS
pub(crate) const MERGEABLE_RENDS: [&str; 4] = ["#b", "#i", "#t", "#u"];

/// The preceding characters that already separate content, so no extra space or newline is needed;
/// the empty string stands for nothing emitted yet.
// upstream: xml.py::SEPARATORS
pub(crate) const SEPARATORS: [&str; 4] = [" ", "\n", "|", ""];

/// The parents inside which `strip_double_tags` leaves a nested element alone.
// upstream: xml.py::NESTING_WHITELIST
pub(crate) const NESTING_WHITELIST: [&str; 5] = ["cell", "figure", "item", "note", "quote"];

/// Removes the element with its children and text; with `keep_tail`, its tail is joined to the
/// previous sibling's tail or to the parent's text. Does nothing without a parent.
// upstream: xml.py::delete_element
pub fn delete_element(tree: &mut Tree, element: NodeId, keep_tail: bool) {
    let Some(parent) = tree.node(element).parent() else {
        return;
    };
    if keep_tail
        && let Some(tail) = tree
            .node(element)
            .tail
            .clone()
            .filter(|tail| !tail.is_empty())
    {
        let target = match tree.node(element).prev_sibling() {
            Some(previous) => &mut tree.node_mut(previous).tail,
            None => &mut tree.node_mut(parent).text,
        };
        target.get_or_insert_with(String::new).push_str(&tail);
    }
    tree.remove(element);
}

/// Merges `element` into its parent and converts its text to Markdown when `include_formatting`:
/// the text goes to the previous sibling's tail or the parent's text, joined by a space. The
/// element's text and tail are kept. When the element is itself a `hi`, `ref` or `del`, the inline
/// text of its children is kept too; for any other element (`p`, `head`, `code`) all its children
/// are lost, `hi` included. Does nothing without a parent.
///
/// Checks `call` while it walks the inline children of the element.
// upstream: xml.py::merge_with_parent
pub fn merge_with_parent(
    tree: &mut Tree,
    element: NodeId,
    include_formatting: bool,
    call: &Call<'_>,
) -> Result<(), ExtractError> {
    let Some(parent) = tree.node(element).parent() else {
        return Ok(());
    };
    let mut full_text = replace_element_text(tree, element, include_formatting, false, call)?;
    if let Some(tail) = &tree.node(element).tail {
        full_text.push_str(tail);
    }
    match tree.node(element).prev_sibling() {
        Some(previous) => {
            let slot = &mut tree.node_mut(previous).tail;
            *slot = Some(match slot.take() {
                Some(tail) if !tail.is_empty() => format!("{tail} {full_text}"),
                _ => full_text,
            });
        }
        None => {
            let slot = &mut tree.node_mut(parent).text;
            *slot = Some(match slot.take() {
                Some(text) => format!("{text} {full_text}"),
                None => full_text,
            });
        }
    }
    tree.remove(element);
    Ok(())
}

/// Whether the element is a line break before inline content.
// upstream: xml.py::separates_inline
pub(crate) fn separates_inline(tree: &Tree, element: NodeId) -> bool {
    tree.tag(element) == Some("lb")
        && tree.node(element).next_sibling().is_some_and(|next| {
            tree.tag(next)
                .is_some_and(|name| INLINE_CARRIED.contains(&name))
        })
}

/// Removes, bottom-up, the elements without children, text and tail that `keeps_empty` does not
/// protect, from `tree`'s subtree at `root`.
///
/// Checks `call` about every `CHECK_INTERVAL` elements.
// upstream: xml.py::remove_empty_elements
pub fn remove_empty_elements(
    tree: &mut Tree,
    root: NodeId,
    call: &Call<'_>,
) -> Result<(), ExtractError> {
    // bottom-up: document order never revisits a parent that its own children just emptied
    let nodes: Vec<NodeId> = tree.iter(root, &[]).collect();
    for (index, element) in nodes.into_iter().rev().enumerate() {
        if index.is_multiple_of(CHECK_INTERVAL) {
            call.check()?;
        }
        let node = tree.node(element);
        if tree.first_child(element).is_none()
            && !text_chars_test(node.text.as_deref())
            && !text_chars_test(node.tail.as_deref())
            && !keeps_empty(tree, element)
        {
            delete_element(tree, element, false);
        }
    }
    Ok(())
}

/// Whether an empty element must stay.
// upstream: xml.py::keeps_empty
pub(crate) fn keeps_empty(tree: &Tree, element: NodeId) -> bool {
    let Some(parent) = tree.node(element).parent() else {
        return true;
    };
    tree.tag(element) == Some("graphic")
        || tree.tag(parent) == Some("code")
        || separates_inline(tree, element)
        || is_padding_cell(tree, element)
}

/// Whether the element is an empty cell keeping the following values of its row in their columns
/// (upstream issue #633).
// upstream: xml.py::is_padding_cell
pub(crate) fn is_padding_cell(tree: &Tree, element: NodeId) -> bool {
    tree.tag(element) == Some("cell")
        && tree
            .node(element)
            .parent()
            .is_some_and(|parent| text_chars_test(Some(&tree.itertext(parent).collect::<String>())))
}

/// Merges each `head`, `code` or `p` nested in an element of its own tag into its parent, unless the
/// parent is in `NESTING_WHITELIST`. Returns `root`.
///
/// The elements are visited last first and each one's descendants through a live walk, as
/// upstream does, so the walk goes on inside a subtree that a merge has just detached. Checks
/// `call` about every `CHECK_INTERVAL` elements.
// upstream: xml.py::strip_double_tags
pub fn strip_double_tags(
    tree: &mut Tree,
    root: NodeId,
    call: &Call<'_>,
) -> Result<NodeId, ExtractError> {
    const TAGS: [&str; 3] = ["code", "head", "p"];
    let candidates: Vec<NodeId> = tree.iterdescendants(root, &TAGS).collect();
    let mut visited = 0usize;
    for element in candidates.into_iter().rev() {
        tick(&mut visited, call)?;
        let mut walk = LiveIter::descendants(tree, element, &TAGS);
        while let Some(nested) = walk.next(tree) {
            tick(&mut visited, call)?;
            let parent_tag = tree
                .node(nested)
                .parent()
                .and_then(|parent| tree.tag(parent));
            if tree.tag(nested) == tree.tag(element)
                && parent_tag.is_none_or(|tag| !NESTING_WHITELIST.contains(&tag))
            {
                merge_with_parent(tree, nested, false, call)?;
            }
        }
    }
    Ok(root)
}

/// Whether `replace_element_text` emits the element's children inline, instead of the walk of
/// `process_element` recursing into them.
// upstream: xml.py::_consumes_inline_children
pub(crate) fn consumes_inline_children(tree: &Tree, element: NodeId) -> bool {
    tree.tag(element)
        .is_some_and(|tag| INLINE_CONSUMING.contains(&tag))
        && tree.first_child(element).is_some()
}

/// The Markdown `#` marker of a `head`: its level from `rend` (`h1` to `h6`), else 2.
// upstream: xml.py::_heading_prefix
pub(crate) fn heading_prefix(tree: &Tree, element: NodeId) -> String {
    let level = tree
        .get(element, "rend")
        .and_then(|rend| rend.chars().nth(1))
        .filter(|digit| HEADING_LEVELS.contains(digit))
        .and_then(|digit| digit.to_digit(10));
    "#".repeat(level.map_or(2, |number| number as usize))
}

/// The Markdown image of a `graphic`: `![alt](src)`, with the alt text made of `title` and `alt`.
// upstream: xml.py::_image_markup
pub(crate) fn image_markup(tree: &Tree, element: NodeId, include_formatting: bool) -> String {
    let attribute = |name| tree.get(element, name).unwrap_or("");
    let alt = format!("{} {}", attribute("title"), attribute("alt"));
    let alt = strip(&alt);
    // alt and title come from attributes, so the tree-wide escaping pass never reaches them
    let alt = if include_formatting {
        escape_inline_specials(alt)
    } else {
        alt.into()
    };
    md_link(&alt, Some(attribute("src")), true)
}

/// The text of an inline element with the text of its children: images, line breaks and the
/// inline formatting children rendered in place, any other child by its own text.
///
/// Ticks `visited` per child.
// upstream: xml.py::_collect_inline_text
fn collect_inline_text(
    tree: &mut Tree,
    element: NodeId,
    include_formatting: bool,
    call: &Call<'_>,
    visited: &mut usize,
) -> Result<String, ExtractError> {
    let mut collected = tree.node(element).text.clone().unwrap_or_default();
    let mut next = tree.first_child(element);
    while let Some(child) = next {
        next = tree.node(child).next_sibling();
        tick(visited, call)?;
        let tag = tree.tag(child);
        if tag == Some("graphic") {
            collected.push_str(&image_markup(tree, child, include_formatting));
        } else if tag == Some("lb") {
            collected.push('\n');
        } else if tag.is_some_and(|name| INLINE_FORMATTABLE.contains(&name)) {
            // `replace_text` calls back into this function: the depth is the tree's, bounded by
            // `MAX_DEPTH` and proven by `output_deep_nesting_fits_512k_stack`
            let piece = replace_text(tree, child, include_formatting, false, call, visited)?;
            collected.push_str(&piece);
        } else if let Some(text) = &tree.node(child).text {
            // fallback: a structural tag nested inside an inline one (malformed input)
            collected.push_str(text);
        }
        if let Some(tail) = &tree.node(child).tail {
            collected.push_str(tail);
        }
    }
    Ok(collected)
}

/// The text of `element` alone, converted to Markdown when `include_formatting`: the caller deals
/// with the tail. An inline element carries its children in the text. In Markdown mode a `code`
/// with line breaks loses them from the tree.
///
/// Checks `call` about every `CHECK_INTERVAL` inline children.
// upstream: xml.py::replace_element_text
pub fn replace_element_text(
    tree: &mut Tree,
    element: NodeId,
    include_formatting: bool,
    in_cell: bool,
    call: &Call<'_>,
) -> Result<String, ExtractError> {
    replace_text(tree, element, include_formatting, in_cell, call, &mut 0)
}

/// `replace_element_text` with the walk counter of its caller, so that the check interval spans the
/// whole inline subtree.
fn replace_text(
    tree: &mut Tree,
    element: NodeId,
    include_formatting: bool,
    in_cell: bool,
    call: &Call<'_>,
    visited: &mut usize,
) -> Result<String, ExtractError> {
    let mut text = if consumes_inline_children(tree, element) {
        collect_inline_text(tree, element, include_formatting, call, visited)?
    } else {
        tree.node(element).text.clone().unwrap_or_default()
    };
    // a fresh paragraph, item or quote line must not start with a marker that reads as block
    // syntax; table cells are inline-only in GFM, so this never applies there
    if include_formatting
        && !text.is_empty()
        && !in_cell
        && matches!(tree.tag(element), Some("p" | "quote" | "item"))
    {
        text = escape_block_start(&text).into_owned();
    }
    if include_formatting && !text.is_empty() {
        text = to_markdown(tree, element, text, in_cell, call, visited)?;
    }
    if tree.tag(element) == Some("ref") {
        let stripped = strip(&text);
        if !stripped.is_empty() {
            let target = tree
                .get(element, "target")
                .filter(|target| !target.is_empty());
            let link = md_link(stripped, target, false);
            text = text.replacen(stripped, &link, 1);
        }
    }
    if tree.tag(element) == Some("cell") {
        text = strip(&text).to_owned();
        // separate the cell's text from its children
        if !text.is_empty() && tree.first_child(element).is_some() {
            text.push(' ');
        }
    }
    Ok(text)
}

/// The Markdown form of the non-empty `text` of `element`, by its tag.
fn to_markdown(
    tree: &mut Tree,
    element: NodeId,
    text: String,
    in_cell: bool,
    call: &Call<'_>,
    visited: &mut usize,
) -> Result<String, ExtractError> {
    Ok(match tree.tag(element) {
        Some("article" | "list" | "table") => strip(&text).to_owned(),
        Some("head") if !in_cell => format!("{} {text}", heading_prefix(tree, element)),
        // the tree-wide pass already escaped every `~`, so no literal `~~` survives in the text
        Some("del") => md_wrap(&text, "~~", None),
        Some("hi") => {
            let rend = tree.get(element, "rend").unwrap_or("");
            let html_tag = HI_HTML_TAGS
                .iter()
                .find(|(key, _)| *key == rend)
                .map(|(_, name)| *name);
            match (hi_marker(rend), html_tag) {
                // inline code, the flanking whitespace stays outside
                (Some("`"), _) => md_code(&text),
                (Some(marker), _) => md_wrap(&text, marker, None),
                (None, Some(name)) => {
                    md_wrap(&text, &format!("<{name}>"), Some(&format!("</{name}>")))
                }
                (None, None) => text,
            }
        }
        Some("code") => code_markup(tree, element, text, call, visited)?,
        _ => text,
    })
}

/// A `code` element as a Markdown fenced block when it holds a line break, else as a code span.
/// The `lb` descendants are removed from the tree, each adding a newline and its tail to the text.
fn code_markup(
    tree: &mut Tree,
    element: NodeId,
    mut text: String,
    call: &Call<'_>,
    visited: &mut usize,
) -> Result<String, ExtractError> {
    let breaks: Vec<NodeId> = tree.iterdescendants(element, &["lb"]).collect();
    if !text.contains('\n') && breaks.is_empty() {
        return Ok(md_code(&text));
    }
    for line_break in breaks {
        tick(visited, call)?;
        text.push('\n');
        if let Some(tail) = &tree.node(line_break).tail {
            text.push_str(tail);
        }
        tree.remove(line_break);
    }
    let fence = code_fence(&text, 3);
    Ok(format!("{fence}\n{text}\n{fence}\n"))
}

/// The local name of an HTML element, owned so the tree can be borrowed mutably while the name is
/// in use; `None` for SVG and MathML, as `Tree::tag`.
fn tag_name(tree: &Tree, element: NodeId) -> Option<LocalName> {
    let node = tree.node(element);
    (node.ns == Ns::Html).then(|| node.name.clone())
}

/// Whether the element has a non-empty text, as Python's `bool(element.text)`.
fn has_text(tree: &Tree, element: NodeId) -> bool {
    tree.node(element)
        .text
        .as_deref()
        .is_some_and(|text| !text.is_empty())
}

/// The `HI_FORMATTING` marker of a `hi` rendition.
fn hi_marker(rend: &str) -> Option<&'static str> {
    HI_FORMATTING
        .iter()
        .find(|(key, _)| *key == rend)
        .map(|(_, marker)| *marker)
}

/// The output of `xml_to_text`, as one string. Upstream keeps a list of pieces and `_last_char`
/// reads the last character of the last piece, where an empty piece reads as `""`; this keeps where
/// that character starts, so an empty piece gives `""` too.
struct Output {
    text: String,
    last_start: usize,
}

impl Output {
    fn new() -> Self {
        Self {
            text: String::new(),
            last_start: 0,
        }
    }

    /// Appends one piece, which may be empty.
    fn push(&mut self, piece: &str) {
        let start = self.text.len();
        self.text.push_str(piece);
        self.last_start = piece
            .char_indices()
            .next_back()
            .map_or(self.text.len(), |(offset, _)| start.saturating_add(offset));
    }

    /// The last character emitted so far, `""` if nothing yet or if the last piece was empty.
    // upstream: xml.py::_last_char
    fn last_char(&self) -> &str {
        self.text.get(self.last_start..).unwrap_or("")
    }

    /// Whether the last character already separates content.
    fn ends_with_separator(&self) -> bool {
        SEPARATORS.contains(&self.last_char())
    }

    /// Appends `text`, dropping a leading space after a separator; an empty text is not appended.
    // upstream: xml.py::_append_spaced
    fn append_spaced(&mut self, text: &str) {
        let text = match text.strip_prefix(' ') {
            Some(rest) if self.ends_with_separator() => rest,
            _ => text,
        };
        if !text.is_empty() {
            self.push(text);
        }
    }
}

/// The state of one `xml_to_text` walk: the output so far and the count of elements visited, for
/// `tick`.
struct State {
    out: Output,
    visited: usize,
}

/// What stays the same down the walk.
#[derive(Clone, Copy)]
struct Walk<'c, 'a> {
    include_formatting: bool,
    call: &'c Call<'a>,
}

/// The Markdown marker of a list item: `N. ` or `- `, indented by its nesting. Numbering is
/// Markdown-only: in plain text it would inject digit tokens, so the marker is `- `.
///
/// The parent is whatever element holds the item, and an item with no `list` ancestor has no
/// indent.
// upstream: xml.py::_list_marker
pub(crate) fn list_marker(tree: &Tree, item: NodeId, include_formatting: bool) -> String {
    let depth = tree.iterancestors(item, &["list"]).count();
    // upstream computes `"  " * (depth - 1)`, which is empty for a negative count
    let indent = "  ".repeat(depth.saturating_sub(1));
    let ordered = include_formatting
        && tree
            .node(item)
            .parent()
            .is_some_and(|parent| tree.get(parent, "rend") == Some("ol"));
    if !ordered {
        return format!("{indent}- ");
    }
    let preceding = std::iter::successors(tree.node(item).prev_sibling(), |&sibling| {
        tree.node(sibling).prev_sibling()
    })
    .filter(|&sibling| tree.tag(sibling) == Some("item"))
    .count();
    format!("{indent}{}. ", preceding.saturating_add(1))
}

/// Whether the element carries the trailing content of its enclosing `item` or `cell`: the
/// container itself when it has no children, else an element with no next sibling, or, in an
/// `item`, one followed by another `item`.
// upstream: xml.py::_closes_container
pub(crate) fn closes_container(tree: &Tree, element: NodeId, tag: &str) -> bool {
    if tree.tag(element) == Some(tag) {
        return tree.first_child(element).is_none();
    }
    tree.node(element)
        .next_sibling()
        .is_none_or(|next| tag == "item" && tree.tag(next) == Some("item"))
}

/// Whether nothing in the enclosing `item` renders before the element.
// upstream: xml.py::_opens_item
pub(crate) fn opens_item(tree: &Tree, element: NodeId) -> bool {
    let mut current = element;
    while tree.tag(current) != Some("item") {
        let node = tree.node(current);
        let Some(parent) = node.parent() else {
            return false;
        };
        if node.prev_sibling().is_some() || text_chars_test(tree.node(parent).text.as_deref()) {
            return false;
        }
        current = parent;
    }
    true
}

/// Whether the `item` renders content of its own before any sublist, whose items carry their own
/// markers. Ticks `visited` per child.
// upstream: xml.py::_item_needs_marker
pub(crate) fn item_needs_marker(
    tree: &Tree,
    item: NodeId,
    call: &Call<'_>,
    visited: &mut usize,
) -> Result<bool, ExtractError> {
    if text_chars_test(tree.node(item).text.as_deref()) {
        return Ok(true);
    }
    for child in tree.children(item) {
        tick(visited, call)?;
        if tree.tag(child) == Some("list") {
            return Ok(false);
        }
        // `itertext` leaves out the tail of the child, which belongs to the item's content
        let mut content: String = tree.itertext(child).collect();
        if let Some(tail) = &tree.node(child).tail {
            content.push_str(tail);
        }
        if text_chars_test(Some(&content)) || tree.iter(child, &["graphic"]).next().is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Appends the tail of `element` as the context calls for.
// upstream: xml.py::_append_tail
fn append_tail(
    tree: &Tree,
    element: NodeId,
    out: &mut Output,
    in_cell: bool,
    in_item: bool,
    include_formatting: bool,
) {
    let Some(tail) = tree
        .node(element)
        .tail
        .as_deref()
        .filter(|tail| !tail.is_empty())
    else {
        return;
    };
    let tag = tree.tag(element);
    let parent = tree.node(element).parent();
    let stripped = strip(tail);
    if include_formatting
        && tag == Some("list")
        && in_item
        && !in_cell
        && !stripped.is_empty()
        && let Some(parent) = parent
    {
        // a blank line and the parent item's indent, else CommonMark continues the last sub-item
        let indent = list_marker(tree, parent, true).chars().count();
        out.push(&format!("\u{2424}\n{}{stripped}", " ".repeat(indent)));
        return;
    }
    if in_cell || in_item || tag == Some("list") {
        let mut core = stripped.to_owned();
        // keep the source separators, block elements lost theirs during extraction
        let in_formattable = tag.is_some_and(|name| INLINE_FORMATTABLE.contains(&name));
        if !core.is_empty() && (tail.chars().next().is_some_and(is_space) || !in_formattable) {
            core.insert(0, ' ');
        }
        let next_is_inline = tree.node(element).next_sibling().is_some_and(|next| {
            tree.tag(next)
                .is_some_and(|name| INLINE_CARRIED.contains(&name))
        });
        if tail.chars().next_back().is_some_and(is_space) && next_is_inline {
            core.push(' ');
        }
        out.append_spaced(&escape_cell(&core, in_cell));
        return;
    }
    if tag.is_some_and(|name| NEWLINE_ELEMS.contains(&name)) {
        // block elements already end on their own line, so source HTML indentation in the tail is
        // noise
        out.push(lstrip(tail));
    } else {
        out.push(tail);
    }
}

/// Appends what comes before the children of `element`: separators, the cell and item markers and
/// the element's own text. Returns whether the children are consumed by that text, and whether the
/// element renders inline text at all.
// upstream: xml.py::process_element, up to the loop over the children
fn open_element(
    tree: &mut Tree,
    element: NodeId,
    state: &mut State,
    walk: Walk<'_, '_>,
    in_cell: bool,
    in_item: bool,
) -> Result<(bool, bool), ExtractError> {
    let tag = tag_name(tree, element);
    let tag = tag.as_deref();
    let State { out, visited } = state;
    if tag == Some("cell") && tree.node(element).prev_sibling().is_none() {
        out.push("| ");
    }

    let is_block = tag.is_some_and(|name| NEWLINE_ELEMS.contains(&name));
    if is_block && !in_cell && !in_item && !out.ends_with_separator() {
        // a block element starts on its own line, not mashed onto preceding loose text
        out.push("\n");
    } else if matches!(tag, Some("head" | "p" | "quote" | "table"))
        && !is_all_space(out.last_char())
        && !out.ends_with_separator()
        && (has_text(tree, element)
            || tree
                .first_child(element)
                .is_none_or(|first| tree.tag(first) != Some("lb")))
    {
        // flattened into an item or a cell, it still needs a separator
        out.push(" ");
    }

    let consumes_children = consumes_inline_children(tree, element);
    let renders_inline = has_text(tree, element) || consumes_children;

    if tag == Some("item") && !in_cell && item_needs_marker(tree, element, walk.call, visited)? {
        out.push(&list_marker(tree, element, walk.include_formatting));
    }

    // whitespace-only item text would split the marker from the content
    if renders_inline
        && (tag != Some("item")
            || consumes_children
            || text_chars_test(tree.node(element).text.as_deref()))
    {
        let text = replace_text(
            tree,
            element,
            walk.include_formatting,
            in_cell,
            walk.call,
            visited,
        )?;
        out.push(&escape_cell(&text, in_cell));
    } else if walk.include_formatting
        && tag == Some("head")
        && !in_cell
        && tree.first_child(element).is_some()
    {
        // a heading starting with an inline child still needs its # prefix, the children render below
        out.push(&format!("{} ", heading_prefix(tree, element)));
    }

    // a sublist starts on its own line, not mashed onto the parent item
    if tag == Some("list") && in_item && !matches!(out.last_char(), "\n" | "") {
        out.push("\n");
    }
    Ok((consumes_children, renders_inline))
}

/// Appends what comes after the children of `element`: its newline or image, the separator, the
/// tail and the end of its item.
// upstream: xml.py::process_element, from the loop over the children on
fn close_element(
    tree: &Tree,
    element: NodeId,
    out: &mut Output,
    include_formatting: bool,
    in_cell: bool,
    in_item: bool,
    renders_inline: bool,
) {
    let tag = tree.tag(element);
    let is_block = tag.is_some_and(|name| NEWLINE_ELEMS.contains(&name));
    if !renders_inline {
        if tag == Some("graphic") {
            out.push(&escape_cell(
                &image_markup(tree, element, include_formatting),
                in_cell,
            ));
            if let Some(tail) = tree
                .node(element)
                .tail
                .as_deref()
                .filter(|tail| !tail.is_empty())
            {
                out.push(&escape_cell(&format!(" {}", strip(tail)), in_cell));
            }
        } else if is_block {
            // newlines for textless elements
            if tag == Some("row") {
                let cells = || {
                    tree.children(element)
                        .filter(|&cell| tree.tag(cell) == Some("cell"))
                };
                // rows are materialized to full width upstream; draw the head separator below
                if cells().any(|cell| tree.get(cell, "role") == Some("head")) {
                    out.push(&format!("\n|{}\n", "---|".repeat(cells().count())));
                }
            } else if !in_cell && !(tag == Some("lb") && in_item && opens_item(tree, element)) {
                // block elements inside a cell must not inject a row-breaking newline, and a
                // leading line break in an item must not split the marker from the text
                out.push("\n");
            }
        } else if !matches!(tag, Some("cell" | "item")) {
            // cells still need to append vertical bars
            if in_cell {
                append_tail(tree, element, out, in_cell, in_item, false);
            }
            return;
        }
    }

    let last_in_item = in_item && closes_container(tree, element, "item");
    let last_in_cell = in_cell && closes_container(tree, element, "cell");
    if is_block && !in_cell && !in_item {
        out.push(if include_formatting && tag != Some("row") {
            "\n\u{2424}\n"
        } else {
            "\n"
        });
    } else if tag == Some("cell") {
        out.push(" | ");
    } else if matches!(tag, Some("head" | "item")) && in_cell && !last_in_cell {
        // separate flattened block elements inside a cell (e.g. list items) instead of mashing them
        out.append_spaced(" ");
    } else if !tag.is_some_and(|name| SPECIAL_FORMATTING.contains(&name))
        && !last_in_item
        && !last_in_cell
    {
        out.append_spaced(" ");
    }

    // a graphic's tail is already handled above
    if tag != Some("graphic") {
        append_tail(tree, element, out, in_cell, in_item, include_formatting);
    }

    // deal with list items alone
    if last_in_item && !in_cell {
        out.push("\n");
    }
}

/// Converts `element` and its children to the flattened text of the output, appended to `state`.
///
/// Recursive: the depth is the tree's, bounded by `MAX_DEPTH` and proven by
/// `output_deep_nesting_fits_512k_stack`. Ticks `state.visited` per element.
// upstream: xml.py::process_element
fn process_element(
    tree: &mut Tree,
    element: NodeId,
    state: &mut State,
    walk: Walk<'_, '_>,
    in_cell: bool,
    in_item: bool,
) -> Result<(), ExtractError> {
    tick(&mut state.visited, walk.call)?;
    // in_cell and in_item are inherited down the recursion instead of re-walking the ancestors
    let in_cell = in_cell || tree.tag(element) == Some("cell");
    let in_item = in_item || tree.tag(element) == Some("item");

    let (consumes_children, renders_inline) =
        open_element(tree, element, state, walk, in_cell, in_item)?;
    if !consumes_children {
        let mut next = tree.first_child(element);
        while let Some(child) = next {
            next = tree.node(child).next_sibling();
            process_element(tree, child, state, walk, in_cell, in_item)?;
        }
    }
    close_element(
        tree,
        element,
        &mut state.out,
        walk.include_formatting,
        in_cell,
        in_item,
        renders_inline,
    );
    Ok(())
}

/// Escapes the inline-syntax characters and rewrites the LaTeX math of the text and tails below
/// `element`, in place. A `code` element, and a `hi` rendered as inline code, are verbatim and
/// skipped whole; the tail of such an element is prose and is still converted.
///
/// Recursive, bounded like `process_element`. Ticks `visited` per element.
// upstream: xml.py::_escape_and_convert_math_tree
fn escape_and_convert_math_tree(
    tree: &mut Tree,
    element: NodeId,
    call: &Call<'_>,
    visited: &mut usize,
) -> Result<(), ExtractError> {
    tick(visited, call)?;
    let tag = tree.tag(element);
    let is_code_span =
        tag == Some("hi") && hi_marker(tree.get(element, "rend").unwrap_or("")) == Some("`");
    if tag == Some("code") || is_code_span {
        return Ok(());
    }
    if let Some(text) = tree
        .node(element)
        .text
        .as_deref()
        .filter(|text| !text.is_empty())
    {
        let converted = escape_and_convert_math(text);
        tree.node_mut(element).text = Some(converted);
    }
    let mut next = tree.first_child(element);
    while let Some(child) = next {
        next = tree.node(child).next_sibling();
        escape_and_convert_math_tree(tree, child, call, visited)?;
        if let Some(tail) = tree
            .node(child)
            .tail
            .as_deref()
            .filter(|tail| !tail.is_empty())
        {
            let converted = escape_and_convert_math(tail);
            tree.node_mut(child).tail = Some(converted);
        }
    }
    Ok(())
}

/// The bit of a `hi` rendition among the `HI_FORMATTING` ones, for `collapse_emphasis`.
fn hi_bit(rend: Option<&str>) -> Option<u8> {
    let rend = rend?;
    HI_FORMATTING
        .iter()
        .position(|(key, _)| *key == rend)
        .map(|index| 1u8 << index)
}

/// The only child of the `hi` `element` that `collapse_emphasis` splices out: a `hi` whose emphasis
/// is already in `active`, in a chain with no text of its own around it.
fn redundant_child(tree: &Tree, element: NodeId, active: u8) -> Option<NodeId> {
    let child = tree.first_child(element)?;
    let only_child = tree.last_child(element) == Some(child);
    let blank = |text: &Option<String>| strip(text.as_deref().unwrap_or("")).is_empty();
    let covered = hi_bit(tree.get(child, "rend")).is_some_and(|bit| active & bit != 0);
    (blank(&tree.node(element).text)
        && only_child
        && tree.tag(child) == Some("hi")
        && blank(&tree.node(child).tail)
        && covered)
        .then_some(child)
}

/// Splices out the redundant emphasis levels in the linear `hi` chains below `element`: nested
/// `font` junk gives `***X***`, not `*******X*******`. Mutates the tree in place.
///
/// Recursive, bounded like `process_element`. Ticks `visited` per element and per splice.
// upstream: xml.py::_collapse_emphasis
fn collapse_emphasis(
    tree: &mut Tree,
    element: NodeId,
    mut active: u8,
    call: &Call<'_>,
    visited: &mut usize,
) -> Result<(), ExtractError> {
    tick(visited, call)?;
    if tree.tag(element) == Some("hi") {
        if let Some(bit) = hi_bit(tree.get(element, "rend")) {
            active |= bit;
        }
        while let Some(child) = redundant_child(tree, element, active) {
            tick(visited, call)?;
            let merged = format!(
                "{}{}",
                tree.node(element).text.as_deref().unwrap_or(""),
                tree.node(child).text.as_deref().unwrap_or("")
            );
            tree.node_mut(element).text = Some(merged);
            let grandchildren: Vec<NodeId> = tree.children(child).collect();
            for grandchild in grandchildren {
                tree.append(element, grandchild);
            }
            tree.remove(child);
        }
    }
    let mut next = tree.first_child(element);
    while let Some(child) = next {
        next = tree.node(child).next_sibling();
        collapse_emphasis(tree, child, active, call, visited)?;
    }
    Ok(())
}

/// Merges the adjacent `hi` siblings with the same `MERGEABLE_RENDS` rend below `element` into one,
/// to avoid `**X** **Y**`. Mutates the tree in place.
///
/// Recursive, bounded like `process_element`. Ticks `visited` per element.
// upstream: xml.py::_merge_adjacent_hi
fn merge_adjacent_hi(
    tree: &mut Tree,
    element: NodeId,
    call: &Call<'_>,
    visited: &mut usize,
) -> Result<(), ExtractError> {
    tick(visited, call)?;
    let children: Vec<NodeId> = tree.children(element).collect();
    for &child in &children {
        merge_adjacent_hi(tree, child, call, visited)?;
    }
    // reverse pass: removing the next element never disturbs the pairs still to be visited
    for pair in children.windows(2).rev() {
        tick(visited, call)?;
        let &[current, next] = pair else { continue };
        let rend = tree.get(current, "rend");
        let mergeable = tree.tag(current) == Some("hi")
            && tree.tag(next) == Some("hi")
            && rend == tree.get(next, "rend")
            && rend.is_some_and(|rend| MERGEABLE_RENDS.contains(&rend))
            && tree.first_child(current).is_none()
            && tree.first_child(next).is_none()
            && strip(tree.node(current).tail.as_deref().unwrap_or("")).is_empty();
        if !mergeable {
            continue;
        }
        let joined: String = [
            tree.node(current).text.as_deref(),
            tree.node(current).tail.as_deref(),
            tree.node(next).text.as_deref(),
        ]
        .into_iter()
        .flatten()
        .collect();
        let next_tail = tree.node(next).tail.clone();
        let node = tree.node_mut(current);
        node.text = Some(joined);
        node.tail = next_tail;
        tree.remove(next);
    }
    Ok(())
}

/// Strips the whitespace around the text of every `p`, `quote` and `head` below `root` (it is
/// not meaningful in block Markdown): the leading whitespace of the text of an element with children,
/// both ends otherwise, and the trailing whitespace of the last child's tail.
///
/// Ticks `visited` per element.
// upstream: xml.py::_strip_block_whitespace
fn strip_block_whitespace(
    tree: &mut Tree,
    root: NodeId,
    call: &Call<'_>,
    visited: &mut usize,
) -> Result<(), ExtractError> {
    const TAGS: [&str; 3] = ["p", "quote", "head"];
    let mut walk = LiveIter::new(tree, root, &TAGS);
    while let Some(block) = walk.next(tree) {
        tick(visited, call)?;
        let last_child = tree.last_child(block);
        let node = tree.node_mut(block);
        if let Some(text) = node.text.as_mut().filter(|text| !text.is_empty()) {
            *text = if last_child.is_some() {
                lstrip(text).to_owned()
            } else {
                strip(text).to_owned()
            };
        }
        if let Some(last) = last_child
            && let Some(tail) = tree
                .node_mut(last)
                .tail
                .as_mut()
                .filter(|tail| !tail.is_empty())
        {
            *tail = rstrip(tail).to_owned();
        }
    }
    Ok(())
}

/// Converts the tree at `root` to plain text, or to Markdown when `include_formatting`. `None`
/// gives an empty string. With formatting the passes run on a copy of the tree, so `root` is never
/// changed; without, nothing is mutated.
///
/// Fails with `Rejection::InvalidValue` for a decimal character reference of more than 4300 digits
/// in the text, which `html.unescape` raises on upstream and nothing catches.
///
/// Checks `call` about every `CHECK_INTERVAL` elements.
// upstream: xml.py::xmltotxt
pub fn xml_to_text(
    tree: &mut Tree,
    root: Option<NodeId>,
    include_formatting: bool,
    call: &Call<'_>,
) -> Result<String, ExtractError> {
    let Some(root) = root else {
        return Ok(String::new());
    };
    let walk = Walk {
        include_formatting,
        call,
    };
    let mut state = State {
        out: Output::new(),
        visited: 0,
    };
    let root = if include_formatting {
        // the math rewrite, the escaping, the emphasis collapse and the `lb` removal mutate the
        // tree: protect the caller's
        let copy = tree.copy(root);
        escape_and_convert_math_tree(tree, copy, call, &mut state.visited)?;
        collapse_emphasis(tree, copy, 0, call, &mut state.visited)?;
        merge_adjacent_hi(tree, copy, call, &mut state.visited)?;
        strip_block_whitespace(tree, copy, call, &mut state.visited)?;
        copy
    } else {
        root
    };
    process_element(tree, root, &mut state, walk, false, false)?;
    let sanitized = sanitize(&state.out.text, true, false).unwrap_or_default();
    Ok(unescape(&sanitized)?.into_owned())
}

mod markdown;
pub use markdown::md_link;

#[cfg(test)]
mod tests;
