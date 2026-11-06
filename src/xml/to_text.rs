//! The text and Markdown walk of upstream `xml.py`: `xml_to_text` and the element handlers behind it.

use crate::cancel::Call;
use crate::dom::{NodeId, Tree};
use crate::error::ExtractError;
use crate::html_processing::tick;
use crate::pyhtml::unescape;
use crate::pystr::{is_all_space, is_space, lstrip, strip};
use crate::settings::{INLINE_CARRIED, INLINE_FORMATTABLE};
use crate::utils::{sanitize, text_chars_test};

use super::{
    collapse_emphasis, consumes_inline_children, escape_and_convert_math_tree, has_text,
    heading_prefix, image_markup, merge_adjacent_hi, replace_text, strip_block_whitespace,
    tag_name,
};

use super::markdown::escape_cell;

/// The elements that start on a line of their own.
// upstream: xml.py::NEWLINE_ELEMS
pub(crate) const NEWLINE_ELEMS: [&str; 8] = [
    "graphic", "head", "lb", "list", "p", "quote", "row", "table",
];

/// The elements whose text carries its own spacing, so no separator follows them.
// upstream: xml.py::SPECIAL_FORMATTING
pub(crate) const SPECIAL_FORMATTING: [&str; 7] =
    ["code", "del", "head", "hi", "ref", "item", "cell"];

/// The preceding characters that already separate content, so no extra space or newline is needed;
/// the empty string stands for nothing emitted yet.
// upstream: xml.py::SEPARATORS
pub(crate) const SEPARATORS: [&str; 4] = [" ", "\n", "|", ""];

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
/// `items_before` is the number of `item` elements among the preceding siblings of `item`. The
/// parent is whatever element holds the item, and an item with no `list` ancestor has no indent.
// upstream: xml.py::_list_marker
pub(crate) fn list_marker(
    tree: &Tree,
    item: NodeId,
    items_before: usize,
    include_formatting: bool,
) -> String {
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
    format!("{indent}{}. ", items_before.saturating_add(1))
}

/// The number of `item` elements among the preceding siblings of `id`.
fn preceding_items(tree: &Tree, id: NodeId) -> usize {
    std::iter::successors(tree.node(id).prev_sibling(), |&sibling| {
        tree.node(sibling).prev_sibling()
    })
    .filter(|&sibling| tree.tag(sibling) == Some("item"))
    .count()
}

/// The preceding `item` siblings of an element and of its parent, which `list_marker` numbers by.
#[derive(Clone, Copy)]
struct ItemCounts {
    own: usize,
    parent: usize,
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
    parent_items_before: usize,
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
        let indent = list_marker(tree, parent, parent_items_before, true)
            .chars()
            .count();
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
    items_before: usize,
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
        out.push(&list_marker(
            tree,
            element,
            items_before,
            walk.include_formatting,
        ));
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
#[allow(clippy::too_many_arguments)] // the walk's inherited context, as upstream's arguments
fn close_element(
    tree: &Tree,
    element: NodeId,
    out: &mut Output,
    include_formatting: bool,
    in_cell: bool,
    in_item: bool,
    renders_inline: bool,
    parent_items_before: usize,
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
                append_tail(
                    tree,
                    element,
                    out,
                    in_cell,
                    in_item,
                    false,
                    parent_items_before,
                );
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
        append_tail(
            tree,
            element,
            out,
            in_cell,
            in_item,
            include_formatting,
            parent_items_before,
        );
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
    counts: ItemCounts,
) -> Result<(), ExtractError> {
    tick(&mut state.visited, walk.call)?;
    // in_cell and in_item are inherited down the recursion instead of re-walking the ancestors
    let in_cell = in_cell || tree.tag(element) == Some("cell");
    let in_item = in_item || tree.tag(element) == Some("item");

    let (consumes_children, renders_inline) =
        open_element(tree, element, state, walk, in_cell, in_item, counts.own)?;
    if !consumes_children {
        // the walk removes only `lb` elements under a `code`, never an `item`, so the running count
        // equals upstream's `itersiblings('item', preceding=True)`
        let mut items_before = 0usize;
        let mut next = tree.first_child(element);
        while let Some(child) = next {
            next = tree.node(child).next_sibling();
            let child_counts = ItemCounts {
                own: items_before,
                parent: counts.own,
            };
            process_element(tree, child, state, walk, in_cell, in_item, child_counts)?;
            if tree.tag(child) == Some("item") {
                items_before = items_before.saturating_add(1);
            }
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
        counts.parent,
    );
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
        let copy = tree.copy(root, call)?;
        escape_and_convert_math_tree(tree, copy, call, &mut state.visited)?;
        collapse_emphasis(tree, copy, 0, call, &mut state.visited)?;
        merge_adjacent_hi(tree, copy, call, &mut state.visited)?;
        strip_block_whitespace(tree, copy, call, &mut state.visited)?;
        copy
    } else {
        root
    };
    let counts = ItemCounts {
        own: preceding_items(tree, root),
        parent: tree
            .node(root)
            .parent()
            .map_or(0, |parent| preceding_items(tree, parent)),
    };
    process_element(tree, root, &mut state, walk, false, false, counts)?;
    let sanitized = sanitize(&state.out.text, true, false).unwrap_or_default();
    Ok(unescape(&sanitized)?.into_owned())
}
