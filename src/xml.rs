//! Tree helpers of upstream `xml.py`: removing and merging elements, the empty-element pass and the
//! element text of the Markdown output. The TEI checks of that file are not ported here.

use crate::cancel::{CHECK_INTERVAL, Call};
use crate::dom::{LiveIter, NodeId, Tree};
use crate::error::ExtractError;
use crate::html_processing::tick;
use crate::pystr::strip;
use crate::settings::{INLINE_CARRIED, INLINE_CONSUMING, INLINE_FORMATTABLE};
use crate::utils::text_chars_test;

use self::markdown::{code_fence, escape_block_start, escape_inline_specials, md_code, md_wrap};

/// The `hi` renditions rendered as Markdown, with their markers.
// upstream: xml.py::HI_FORMATTING
pub(crate) const HI_FORMATTING: [(&str, &str); 3] = [("#b", "**"), ("#i", "*"), ("#t", "`")];

/// The `hi` renditions with no Markdown syntax, rendered as inline HTML tags.
// upstream: xml.py::HI_HTML_TAGS
pub(crate) const HI_HTML_TAGS: [(&str, &str); 3] = [("#sup", "sup"), ("#sub", "sub"), ("#u", "u")];

/// The heading levels a `head` `rend` may name after its first character.
// upstream: xml.py::HEADING_LEVELS
pub(crate) const HEADING_LEVELS: [char; 6] = ['1', '2', '3', '4', '5', '6'];

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
/// the text goes to the previous sibling's tail or the parent's text, joined by a space. Only the
/// element's own text and tail are kept, its children are lost. Does nothing without a parent.
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
            let lookup = |pairs: &[(&'static str, &'static str)]| {
                pairs
                    .iter()
                    .find(|(key, _)| *key == rend)
                    .map(|(_, value)| *value)
            };
            match (lookup(&HI_FORMATTING), lookup(&HI_HTML_TAGS)) {
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

mod markdown;
pub use markdown::md_link;

#[cfg(test)]
mod tests;
