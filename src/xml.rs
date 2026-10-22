//! Tree helpers of upstream `xml.py`: removing elements and the empty-element pass. The output
//! formats and the TEI checks of that file are not ported here.

use crate::cancel::{CHECK_INTERVAL, Call};
use crate::dom::{NodeId, Tree};
use crate::error::ExtractError;
use crate::settings::INLINE_CARRIED;
use crate::utils::text_chars_test;

/// Removes the element with its children and text; with `keep_tail`, its tail is joined to the
/// previous sibling's tail or to the parent's text. Does nothing without a parent.
// upstream: xml.py::delete_element
pub(crate) fn delete_element(tree: &mut Tree, element: NodeId, keep_tail: bool) {
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

/// Whether the element is a line break before inline content.
// upstream: xml.py::separates_inline
pub(crate) fn separates_inline(tree: &Tree, element: NodeId) -> bool {
    &*tree.node(element).name == "lb"
        && tree
            .node(element)
            .next_sibling()
            .is_some_and(|next| INLINE_CARRIED.contains(&&*tree.node(next).name))
}

/// Removes, bottom-up, the elements without children, text and tail that `keeps_empty` does not
/// protect, from `tree`'s subtree at `root`.
///
/// Checks `call` about every `CHECK_INTERVAL` elements.
// upstream: xml.py::remove_empty_elements
pub(crate) fn remove_empty_elements(
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
    &*tree.node(element).name == "graphic"
        || &*tree.node(parent).name == "code"
        || separates_inline(tree, element)
        || is_padding_cell(tree, element)
}

/// Whether the element is an empty cell keeping the following values of its row in their columns
/// (upstream issue #633).
// upstream: xml.py::is_padding_cell
pub(crate) fn is_padding_cell(tree: &Tree, element: NodeId) -> bool {
    &*tree.node(element).name == "cell"
        && tree
            .node(element)
            .parent()
            .is_some_and(|parent| text_chars_test(Some(&tree.itertext(parent).collect::<String>())))
}

#[cfg(test)]
mod tests;
