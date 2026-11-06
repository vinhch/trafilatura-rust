//! The part of upstream `xml.py` that serves the plain-text and Markdown output: removing and merging
//! elements, the empty-element pass, the element text and `xml_to_text`, plus `meta_items`. The XML,
//! TEI, JSON and CSV builders and the TEI checks are stubs until phase 6.

use std::borrow::Cow;
use std::collections::HashMap;

use crate::cancel::{CHECK_INTERVAL, Call};
use crate::dom::{LiveIter, LocalName, NodeId, Ns, Tree};
use crate::error::ExtractError;
use crate::html_processing::tick;
use crate::pystr::{lstrip, rstrip, strip};
use crate::settings::{
    Document, Extractor, Field, INLINE_CARRIED, INLINE_CONSUMING, INLINE_FORMATTABLE,
};
use crate::utils::text_chars_test;

use self::markdown::{
    code_fence, escape_and_convert_math, escape_block_start, escape_inline_specials, md_code,
    md_wrap,
};

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
            match slot {
                Some(tail) if !tail.is_empty() => {
                    tail.push(' ');
                    tail.push_str(&full_text);
                }
                _ => *slot = Some(full_text),
            }
        }
        None => {
            let slot = &mut tree.node_mut(parent).text;
            match slot {
                Some(text) => {
                    text.push(' ');
                    text.push_str(&full_text);
                }
                None => *slot = Some(full_text),
            }
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
    let mut rows = HashMap::new();
    for (index, element) in nodes.into_iter().rev().enumerate() {
        if index.is_multiple_of(CHECK_INTERVAL) {
            call.check()?;
        }
        let node = tree.node(element);
        if tree.first_child(element).is_none()
            && !text_chars_test(node.text.as_deref())
            && !text_chars_test(node.tail.as_deref())
            && !keeps_empty(tree, element, &mut rows)
        {
            delete_element(tree, element, false);
        }
    }
    Ok(())
}

/// Whether an empty element must stay. `rows` holds the row decisions of the pass that
/// `is_padding_cell` reads.
// upstream: xml.py::keeps_empty
pub(crate) fn keeps_empty(tree: &Tree, element: NodeId, rows: &mut HashMap<NodeId, bool>) -> bool {
    let Some(parent) = tree.node(element).parent() else {
        return true;
    };
    tree.tag(element) == Some("graphic")
        || tree.tag(parent) == Some("code")
        || separates_inline(tree, element)
        || is_padding_cell(tree, element, rows)
}

/// Whether the element is an empty cell keeping the following values of its row in their columns
/// (upstream issue #633). `rows` maps each row to whether it has text, computed on first use from
/// its text pieces: one of them has a non-space character. Deleting an empty cell never changes
/// that, so a pass that deletes empty elements can keep one map for all of them.
// upstream: xml.py::is_padding_cell
pub(crate) fn is_padding_cell(
    tree: &Tree,
    element: NodeId,
    rows: &mut HashMap<NodeId, bool>,
) -> bool {
    if tree.tag(element) != Some("cell") {
        return false;
    }
    let Some(row) = tree.node(element).parent() else {
        return false;
    };
    *rows
        .entry(row)
        .or_insert_with(|| tree.itertext(row).any(|piece| text_chars_test(Some(piece))))
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
    if let Some(text) = tree.node(element).text.as_deref()
        && let Cow::Owned(converted) = escape_and_convert_math(text)
    {
        tree.node_mut(element).text = Some(converted);
    }
    let mut next = tree.first_child(element);
    while let Some(child) = next {
        next = tree.node(child).next_sibling();
        escape_and_convert_math_tree(tree, child, call, visited)?;
        if let Some(tail) = tree.node(child).tail.as_deref()
            && let Cow::Owned(converted) = escape_and_convert_math(tail)
        {
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
/// Recursive, bounded like `process_element`. Ticks `visited` per element and per adjacent pair.
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
    // Every pair is judged on the original nodes before anything is removed: a removed node keeps
    // its tail, and the condition reads the tail of the current element.
    let mut mergeable = Vec::with_capacity(children.len().saturating_sub(1));
    for (&current, &next) in children.iter().zip(children.iter().skip(1)) {
        tick(visited, call)?;
        let rend = tree.get(current, "rend");
        mergeable.push(
            tree.tag(current) == Some("hi")
                && tree.tag(next) == Some("hi")
                && rend == tree.get(next, "rend")
                && rend.is_some_and(|rend| MERGEABLE_RENDS.contains(&rend))
                && tree.first_child(current).is_none()
                && tree.first_child(next).is_none()
                && strip(tree.node(current).tail.as_deref().unwrap_or("")).is_empty(),
        );
    }
    // A run of mergeable pairs collapses into its first element, built once so the cost stays
    // linear in the run length.
    let mut start = 0;
    while start < children.len() {
        let mut end = start;
        while mergeable.get(end) == Some(&true) {
            end = end.saturating_add(1);
        }
        if let Some(run) = children.get(start..=end).filter(|run| run.len() > 1) {
            merge_run(tree, run);
        }
        start = end.saturating_add(1);
    }
    Ok(())
}

/// Joins the `hi` elements of `run` into its first one: the text and tail of each element but the
/// last, then the text of the last, whose tail becomes the tail of the first. The rest are removed.
fn merge_run(tree: &mut Tree, run: &[NodeId]) {
    let (Some(&first), Some(&last)) = (run.first(), run.last()) else {
        return;
    };
    let mut joined = String::new();
    for &node in run {
        joined.extend(tree.node(node).text.as_deref());
        if node != last {
            joined.extend(tree.node(node).tail.as_deref());
        }
    }
    let last_tail = tree.node_mut(last).tail.take();
    let node = tree.node_mut(first);
    node.text = Some(joined);
    node.tail = last_tail;
    for &node in run.iter().skip(1) {
        tree.remove(node);
    }
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

/// The metadata fields the XML and HTML outputs write, in output order.
// upstream: xml.py::META_ATTRIBUTES
pub const META_ATTRIBUTES: &[&str] = &[
    "sitename",
    "title",
    "author",
    "date",
    "url",
    "hostname",
    "description",
    "categories",
    "tags",
    "license",
    "id",
    "fingerprint",
    "language",
];

/// The non-empty metadata fields of `doc` in `META_ATTRIBUTES` order, lists joined with `;`.
// upstream: xml.py::meta_items
pub fn meta_items(doc: &Document) -> Vec<(&'static str, String)> {
    META_ATTRIBUTES
        .iter()
        .filter_map(|&name| {
            let value = match doc.field(name)? {
                Field::Text(text) => text.to_owned(),
                Field::List(items) => items.join(";"),
            };
            Some((name, value))
        })
        .collect()
}

/// The XML or XML-TEI string of `doc`. Empty until phase 6.
// upstream: xml.py::control_xml_output
pub fn control_xml_output(_doc: &mut Document, _options: &Extractor) -> String {
    // todo(phase 6): xml
    String::new()
}

/// The CSV line of `doc`. Empty until phase 6.
// upstream: xml.py::xmltocsv
pub fn xml_to_csv(_doc: &Document, _include_formatting: bool) -> String {
    // todo(phase 6): xml
    String::new()
}

/// The JSON string of `doc`. Empty until phase 6.
// upstream: xml.py::build_json_output
pub fn build_json_output(_doc: &Document, _with_metadata: bool) -> String {
    // todo(phase 6): xml
    String::new()
}

mod markdown;
mod to_text;
pub use markdown::md_link;
pub use to_text::xml_to_text;

#[cfg(test)]
mod tests;
