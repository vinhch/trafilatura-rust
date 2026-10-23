//! The main extraction handlers of upstream `main_extractor.py`.
//!
//! Every handler takes the tree and the element it works on, and the `Ctx` (or the `Call`) where
//! it loops over nodes. A handler returns the element it built or reused, `None` where upstream
//! returns `None`.

use std::collections::{HashMap, HashSet};

use crate::cancel::Call;
use crate::context::Ctx;
use crate::dom::{LiveIter, LocalName, NodeId, Ns, Tree};
use crate::error::{ExtractError, Rejection};
use crate::html_processing::{
    delete_by_link_density, handle_textnode, has_content, link_density_test_tables, process_node,
    prune_unwanted_nodes, tick,
};
use crate::pystr;
use crate::pyurl::urljoin;
use crate::settings::{
    DEDUPE_SCAN_CAP, DEFAULT_MAX_NODES, Extractor, Focus, INLINE_CARRIED, INLINE_CONSUMING,
    INLINE_FORMATTABLE, MIN_DUPLICATE_LENGTH, TAG_CATALOG,
};
use crate::utils::{FORMATTING_PROTECTED, SPACING_PROTECTED, image_src, text_chars_test, trim};
use crate::xml::{delete_element, separates_inline};
use crate::xpaths::select;
use crate::xpaths::{
    BODY_XPATH, COMMENTS_DISCARD_XPATH, COMMENTS_XPATH, DISCARD_IMAGE_ELEMENTS,
    OVERALL_DISCARD_XPATH, PRECISION_DISCARD_XPATH, TEASER_DISCARD_XPATH,
};

/// Cell tags of a table row.
// upstream: main_extractor.py::TABLE_ELEMS
pub const TABLE_ELEMS: &[&str] = &["td", "th"];

/// Meaningful internal attributes to carry onto a rewired sub-element; stray `class`, `style` and
/// `width` are dropped.
// upstream: main_extractor.py::KEEP_ATTRS
pub const KEEP_ATTRS: &[&str] = &["rend", "role", "target", "src", "alt", "title"];

/// Tags `handle_quotes` takes.
// upstream: main_extractor.py::CODES_QUOTES
pub const CODES_QUOTES: &[&str] = &["code", "quote"];

/// Blocks `handle_textelem` rebuilds as new elements, which leaves the source tail behind.
// upstream: main_extractor.py::REBUILT_BLOCKS
pub const REBUILT_BLOCKS: &[&str] = &["code", "quote", "list", "table"];

/// Tags that must not close the extracted body.
// upstream: main_extractor.py::NOT_AT_THE_END
pub const NOT_AT_THE_END: &[&str] = &["head", "ref"];

/// Tags allowed inside a blockquote paragraph: `TAG_CATALOG` plus `ref` and `graphic`.
// upstream: main_extractor.py::_QUOTE_TAGS
pub const QUOTE_TAGS: &[&str] = &[
    "code", "del", "head", "hi", "lb", "list", "p", "quote", "ref", "graphic",
];

/// Largest `colspan` or `rowspan` a cell may carry.
// upstream: main_extractor.py::_MAX_SPAN
pub const MAX_SPAN: usize = 100;

/// Arena size, in nodes, above which table construction fails with `Rejection::TreeTooLarge`: a
/// parse of at most `DEFAULT_MAX_NODES` elements, `extract_content`'s backup copy and the nodes
/// table construction creates all fit.
// deviation: D-9
pub const MAX_OUTPUT_NODES: usize = 4 * DEFAULT_MAX_NODES;

/// Text rendering for the recovery and adjacent dedup checks: plain concatenation, so inline tag
/// boundaries stay joined (`Hyper<b>link</b>ed` is `Hyperlinked`). Both sides of those checks use
/// it, or invented spaces defeat the comparison.
// upstream: main_extractor.py::_elem_text
pub(crate) fn elem_text(tree: &Tree, element: NodeId) -> String {
    trim(&tree.itertext(element).collect::<String>())
}

/// A formatting element whose children must be carried verbatim: `ref`, or `hi` and `del`
/// wrapping inline content.
// upstream: main_extractor.py::_wraps_inline
fn wraps_inline(tree: &Tree, element: NodeId) -> bool {
    tree.child_count(element) > 0
        && (tree.tag(element) == Some("ref")
            || tree
                .children(element)
                .any(|child| in_tags(tree, child, INLINE_CARRIED)))
}

/// Whether `element` is an HTML element named one of `tags`.
fn in_tags(tree: &Tree, element: NodeId, tags: &[&str]) -> bool {
    tree.tag(element).is_some_and(|tag| tags.contains(&tag))
}

/// `" ".join(filter(None, (first, second)))`: the non-empty pieces joined by a space.
fn join_nonempty(first: Option<&str>, second: Option<&str>) -> String {
    [first, second]
        .into_iter()
        .flatten()
        .filter(|piece| !piece.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Renames every element `walk` yields to `done`, as upstream's `child.tag = "done"` loops do.
fn rename_done(
    tree: &mut Tree,
    mut walk: LiveIter<'_>,
    call: &Call<'_>,
) -> Result<(), ExtractError> {
    let mut visited = 0usize;
    while let Some(node) = walk.next(tree) {
        tick(&mut visited, call)?;
        tree.set_name(node, "done");
    }
    Ok(())
}

/// Processes a head element (title).
// upstream: main_extractor.py::handle_titles
pub fn handle_titles(
    tree: &mut Tree,
    element: NodeId,
    ctx: &Ctx<'_>,
) -> Result<Option<NodeId>, ExtractError> {
    let title = if tree.child_count(element) == 0 {
        process_node(tree, element, ctx)
    } else {
        let title = tree.copy(element);
        // children already consumed by an earlier pass over the same tree
        tree.strip_elements(title, &["done"], true);
        rename_done(tree, LiveIter::descendants(tree, element, &[]), ctx.call)?;
        Some(title)
    };
    Ok(title.filter(|&title| is_text_element(tree, Some(title))))
}

/// Processes a formatting element (`b`, `i` etc. converted to `hi`) found outside of paragraphs.
// upstream: main_extractor.py::handle_formatting
pub fn handle_formatting(tree: &mut Tree, element: NodeId, ctx: &Ctx<'_>) -> Option<NodeId> {
    let formatting = process_node(tree, element, ctx)?;
    let parent = tree.node(element).parent();
    if parent.is_some_and(|parent| in_tags(tree, parent, FORMATTING_PROTECTED)) {
        return Some(formatting);
    }
    // repair orphan elements
    let wrapper = tree.new_element("p");
    tree.append(wrapper, formatting);
    Some(wrapper)
}

/// Copies the `rend` attribute from an existing element to a new one.
// upstream: main_extractor.py::update_elem_rendition
fn update_elem_rendition(tree: &mut Tree, elem: NodeId, new_elem: NodeId) {
    if let Some(rend) = tree.get(elem, "rend").filter(|rend| !rend.is_empty()) {
        let rend = rend.to_string();
        tree.set(new_elem, "rend", &rend);
    }
}

/// Whether the element contains text.
// upstream: main_extractor.py::is_text_element
fn is_text_element(tree: &Tree, elem: Option<NodeId>) -> bool {
    elem.is_some_and(|elem| text_chars_test(Some(&tree.itertext(elem).collect::<String>())))
}

/// Carries the meaningful internal attributes over to a rewired element.
// upstream: main_extractor.py::_copy_attrs
fn copy_attrs(tree: &mut Tree, source: NodeId, target: NodeId) {
    let kept: Vec<_> = tree
        .node(source)
        .attrs
        .iter()
        .filter(|(key, _)| KEEP_ATTRS.contains(&&**key))
        .cloned()
        .collect();
    for (key, value) in kept {
        tree.set(target, &key, &value);
    }
}

/// Creates a new sub-element of `orig`, optionally carrying its inline children
/// (`INLINE_CARRIED`).
// upstream: main_extractor.py::define_newelem
fn define_newelem(
    tree: &mut Tree,
    processed: Option<NodeId>,
    orig: NodeId,
    keep_children: bool,
    call: &Call<'_>,
) -> Result<(), ExtractError> {
    let Some(processed) = processed else {
        return Ok(());
    };
    let child = tree.new_element_like(processed);
    tree.append(orig, child);
    let node = tree.node(processed);
    let (text, tail) = (node.text.clone(), node.tail.clone());
    let node = tree.node_mut(child);
    node.text = text;
    node.tail = tail;
    copy_attrs(tree, processed, child);
    if keep_children {
        // upstream walks the children of `processed`, which this body never edits
        let subs: Vec<NodeId> = tree.children(processed).collect();
        let mut visited = 0usize;
        for sub in subs {
            tick(&mut visited, call)?;
            if in_tags(tree, sub, INLINE_CARRIED) || tree.tag(sub) == Some("lb") {
                define_newelem(tree, Some(sub), child, true, call)?;
                // mark only the carried subtree done; non-carried siblings (e.g. a nested `p`)
                // stay processable
                rename_done(tree, LiveIter::new(tree, sub, &[]), call)?;
            }
        }
    }
    Ok(())
}

/// Iterates through an element's descendants and rewires them under `new_child_elem`.
// upstream: main_extractor.py::process_nested_elements
pub fn process_nested_elements(
    tree: &mut Tree,
    child: NodeId,
    new_child_elem: NodeId,
    ctx: &Ctx<'_>,
) -> Result<(), ExtractError> {
    tree.node_mut(new_child_elem).text = tree.node(child).text.clone();
    let mut walk = LiveIter::descendants(tree, child, &[]);
    let mut visited = 0usize;
    while let Some(subelem) = walk.next(tree) {
        tick(&mut visited, ctx.call)?;
        let tag = tree.tag(subelem);
        if tag == Some("list") {
            let processed = handle_lists(tree, subelem, ctx)?;
            append_block(tree, new_child_elem, processed, subelem);
        } else if tag == Some("p") && tree.child_count(subelem) > 0 {
            let processed = handle_paragraphs(tree, subelem, QUOTE_TAGS, ctx)?;
            append_block(tree, new_child_elem, processed, subelem);
        } else if tag == Some("graphic") {
            let processed = handle_image(tree, subelem, Some(&ctx.options))?;
            append_block(tree, new_child_elem, processed, subelem);
        } else if in_tags(tree, subelem, INLINE_CARRIED) {
            define_newelem(tree, Some(subelem), new_child_elem, true, ctx.call)?;
        } else {
            if let Some(last) = tree.last_child(subelem)
                && text_chars_test(tree.node(subelem).tail.as_deref())
            {
                // the tail follows the children, which are appended after this element
                let tail = tree.node_mut(subelem).tail.take().unwrap_or_default();
                let last_tail = tree.node(last).tail.as_deref().unwrap_or("");
                let merged = format!("{last_tail}{tail}");
                tree.node_mut(last).tail = Some(merged);
            }
            let processed = handle_textnode(tree, subelem, ctx, false, false);
            define_newelem(tree, processed, new_child_elem, false, ctx.call)?;
        }
        tree.set_name(subelem, "done");
    }
    Ok(())
}

/// Appends a processed block with the source tail, or keeps that tail as text after the parent's
/// content.
// upstream: main_extractor.py::_append_block
fn append_block(tree: &mut Tree, parent: NodeId, processed: Option<NodeId>, source: NodeId) {
    let tail = tree.node(source).tail.clone();
    if let Some(processed) = processed {
        tree.node_mut(processed).tail = tail;
        tree.append(parent, processed);
        return;
    }
    let Some(tail) = tail.filter(|tail| text_chars_test(Some(tail))) else {
        return;
    };
    let last = tree.last_child(parent);
    let before = match last {
        None => tree.node(parent).text.clone(),
        Some(last) => tree.node(last).tail.clone(),
    }
    .unwrap_or_default();
    // a dropped block still separates the text around it, a dropped image does not
    let separate = tree.tag(source) != Some("graphic")
        && (!before.is_empty() || last.is_some())
        && !before.chars().next_back().is_some_and(pystr::is_space)
        && !tail.chars().next().is_some_and(pystr::is_space);
    let joined = if separate {
        format!("{before} {tail}")
    } else {
        format!("{before}{tail}")
    };
    match last {
        None => tree.node_mut(parent).text = Some(joined),
        Some(last) => tree.node_mut(last).tail = Some(joined),
    }
}

/// Processes list elements including their descendants.
// upstream: main_extractor.py::handle_lists
pub fn handle_lists(
    tree: &mut Tree,
    element: NodeId,
    ctx: &Ctx<'_>,
) -> Result<Option<NodeId>, ExtractError> {
    let processed_element = tree.new_element_like(element);

    if text_chars_test(tree.node(element).text.as_deref()) {
        let new_child_elem = tree.new_element("item");
        tree.append(processed_element, new_child_elem);
        tree.node_mut(new_child_elem).text = tree.node(element).text.clone();
    }

    let mut walk = LiveIter::descendants(tree, element, &["item"]);
    let mut visited = 0usize;
    while let Some(child) = walk.next(tree) {
        tick(&mut visited, ctx.call)?;
        // items of a nested list are processed by the recursive call and renamed "done",
        // but the walk fetches the first of them before that happens: skip it here
        if tree.tag(child) == Some("done") {
            continue;
        }
        let new_child_elem = tree.new_element("item");
        if tree.child_count(child) == 0 {
            if let Some(processed_child) = process_node(tree, child, ctx) {
                let node = tree.node(processed_child);
                let text = join_nonempty(node.text.as_deref(), node.tail.as_deref());
                tree.node_mut(new_child_elem).text = Some(text);
            }
        } else {
            process_nested_elements(tree, child, new_child_elem, ctx)?;
            let child_tail = tree.node(child).tail.clone();
            if let Some(last) = tree.last_child(new_child_elem)
                && text_chars_test(child_tail.as_deref())
            {
                let child_tail = child_tail.unwrap_or_default();
                let merged = match tree.node(last).tail.as_deref() {
                    Some(last_tail) if text_chars_test(Some(last_tail)) => {
                        format!("{last_tail} {child_tail}")
                    }
                    _ => child_tail,
                };
                tree.node_mut(last).tail = Some(merged);
            }
        }
        if is_text_element(tree, Some(new_child_elem))
            || tree.iter(new_child_elem, &["graphic"]).next().is_some()
        {
            update_elem_rendition(tree, child, new_child_elem);
            tree.append(processed_element, new_child_elem);
        }
        tree.set_name(child, "done");
    }
    tree.set_name(element, "done");
    if is_text_element(tree, Some(processed_element)) {
        update_elem_rendition(tree, element, processed_element);
        return Ok(Some(processed_element));
    }
    Ok(None)
}

/// Whether the element is a code element by common structural markers.
// upstream: main_extractor.py::is_code_block_element
fn is_code_block_element(tree: &Tree, element: NodeId) -> bool {
    // pip
    if has_content(tree.get(element, "lang")) || tree.tag(element) == Some("code") {
        return true;
    }
    // GitHub
    let parent = tree.node(element).parent();
    if parent.is_some_and(|parent| {
        tree.get(parent, "class")
            .is_some_and(|class| class.contains("highlight"))
    }) {
        return true;
    }
    // highlightjs
    let is_blank = |text: Option<&str>| pystr::strip(text.unwrap_or("")).is_empty();
    let code = tree
        .children(element)
        .find(|&child| tree.tag(child) == Some("code"));
    code.is_some_and(|code| {
        tree.child_count(element) == 1
            && is_blank(tree.node(element).text.as_deref())
            && is_blank(tree.node(code).tail.as_deref())
    })
}

/// Turns `element` into a properly tagged code block; the source and its descendants become
/// `done`.
// upstream: main_extractor.py::handle_code_blocks
pub fn handle_code_blocks(
    tree: &mut Tree,
    element: NodeId,
    call: &Call<'_>,
) -> Result<NodeId, ExtractError> {
    let processed = tree.copy(element);
    rename_done(tree, LiveIter::new(tree, element, &[]), call)?;
    tree.set_name(processed, "code");
    Ok(processed)
}

/// Processes quote elements.
// upstream: main_extractor.py::handle_quotes
pub fn handle_quotes(
    tree: &mut Tree,
    element: NodeId,
    ctx: &Ctx<'_>,
) -> Result<Option<NodeId>, ExtractError> {
    if is_code_block_element(tree, element) {
        return handle_code_blocks(tree, element, ctx.call).map(Some);
    }

    let processed_element = tree.new_element_like(element);
    process_nested_elements(tree, element, processed_element, ctx)?;
    if is_text_element(tree, Some(processed_element)) {
        // avoid double/nested tags
        tree.strip_tags(processed_element, &["quote"]);
        return Ok(Some(processed_element));
    }
    Ok(None)
}

/// Handles diverse or unknown elements in the scope of relevant tags.
// upstream: main_extractor.py::handle_other_elements
pub fn handle_other_elements(
    tree: &mut Tree,
    element: NodeId,
    potential_tags: &[&str],
    ctx: &Ctx<'_>,
) -> Result<Option<NodeId>, ExtractError> {
    let is_div = tree.tag(element) == Some("div");
    // handle w3schools code
    if is_div
        && tree
            .get(element, "class")
            .is_some_and(|class| class.contains("w3-code"))
    {
        return handle_code_blocks(tree, element, ctx.call).map(Some);
    }

    if !is_div || !potential_tags.contains(&"div") {
        return Ok(None);
    }

    let Some(processed) = handle_textnode(tree, element, ctx, false, true) else {
        return Ok(None);
    };
    if !text_chars_test(tree.node(processed).text.as_deref()) {
        return Ok(None);
    }
    tree.clear_attrs(processed);
    tree.set_name(processed, "p");
    Ok(Some(processed))
}

/// Processes a paragraph along with its children, trims and cleans the content.
// upstream: main_extractor.py::handle_paragraphs
pub fn handle_paragraphs(
    tree: &mut Tree,
    element: NodeId,
    potential_tags: &[&str],
    ctx: &Ctx<'_>,
) -> Result<Option<NodeId>, ExtractError> {
    if tree.child_count(element) == 0 {
        return Ok(process_node(tree, element, ctx));
    }

    let processed_element = tree.new_element_like(element);
    // unexpected children, foreign ones included: keep their text in place
    tree.strip_tags_where(element, |tree, node| {
        !tree
            .tag(node)
            .is_some_and(|tag| tag == "done" || potential_tags.contains(&tag))
    });
    let mut walk = LiveIter::new(tree, element, &[]);
    let mut visited = 0usize;
    while let Some(child) = walk.next(tree) {
        tick(&mut visited, ctx.call)?;
        if let Some(processed_child) = handle_textnode(tree, child, ctx, false, true) {
            if tree.tag(processed_child) == Some("p") {
                let text = join_nonempty(
                    tree.node(processed_element).text.as_deref(),
                    tree.node(processed_child).text.as_deref(),
                );
                tree.node_mut(processed_element).text = Some(text);
                tree.set_name(child, "done");
                continue;
            }
            let mut newsub = tree.new_element_like(child);
            if in_tags(tree, processed_child, INLINE_CONSUMING) {
                // carry inline children verbatim (ref/hi/del wrapping formatting)
                if wraps_inline(tree, processed_child) {
                    define_newelem(
                        tree,
                        Some(processed_child),
                        processed_element,
                        true,
                        ctx.call,
                    )?;
                    tree.set_name(child, "done");
                    continue;
                }
                let items: Vec<NodeId> = tree.children(processed_child).collect();
                for &item in &items {
                    tick(&mut visited, ctx.call)?;
                    let node = tree.node(item);
                    if tree.tag(item) == Some("lb") && has_content(node.tail.as_deref()) {
                        let tail = node
                            .tail
                            .as_deref()
                            .unwrap_or("")
                            .trim_start_matches(pystr::is_space);
                        tree.node_mut(item).tail = Some(format!(" {tail}"));
                    } else if text_chars_test(node.text.as_deref()) {
                        let text = format!(" {}", node.text.as_deref().unwrap_or(""));
                        tree.node_mut(item).text = Some(text);
                    }
                }
                let direct: HashSet<(LocalName, Ns)> = items
                    .iter()
                    .map(|&item| (tree.node(item).name.clone(), tree.node(item).ns))
                    .collect();
                tree.strip_tags_where(processed_child, |tree, node| {
                    let node = tree.node(node);
                    direct.contains(&(node.name.clone(), node.ns))
                });
                copy_attrs(tree, child, newsub);
            }
            let node = tree.node(processed_child);
            let (text, tail) = (node.text.clone(), node.tail.clone());
            let node = tree.node_mut(newsub);
            node.text = text;
            node.tail = tail;

            if tree.tag(processed_child) == Some("graphic")
                && let Some(image) = handle_image(tree, processed_child, Some(&ctx.options))?
            {
                newsub = image;
            }
            tree.append(processed_element, newsub);
        }
        tree.set_name(child, "done");
    }
    if let Some(last) = tree.last_child(processed_element) {
        if tree.tag(last) == Some("lb") && tree.node(last).tail.is_none() {
            delete_element(tree, last, true);
        }
        return Ok(Some(processed_element));
    }
    // text_chars_test, not truthiness: layout wrappers (clearfix/spacer divs, cleared-out ad
    // slots) leave indentation-only text
    Ok(text_chars_test(tree.node(processed_element).text.as_deref()).then_some(processed_element))
}

/// Determines the cell element type and mints a new element; fails with `Rejection::TreeTooLarge`
/// once the arena holds more than `max_len` nodes.
// upstream: main_extractor.py::define_cell_type
fn define_cell_type(
    tree: &mut Tree,
    is_header: bool,
    max_len: usize,
) -> Result<NodeId, ExtractError> {
    // deviation: D-9
    if tree.len() > max_len {
        return Err(ExtractError::Rejected(Rejection::TreeTooLarge));
    }
    let cell = tree.new_element("cell");
    if is_header {
        tree.set(cell, "role", "head");
    }
    Ok(cell)
}

/// A `row` under construction with its cell count; `Tree::child_count` walks the children, which
/// would make a row of n cells cost n squared.
struct Row {
    id: NodeId,
    len: usize,
}

impl Row {
    fn new(tree: &mut Tree) -> Self {
        Self {
            id: tree.new_element("row"),
            len: 0,
        }
    }

    fn push(&mut self, tree: &mut Tree, cell: NodeId) {
        tree.append(self.id, cell);
        self.len = self.len.saturating_add(1);
    }
}

/// Parses the `colspan` or `rowspan` of a cell: 1 by default and for a value that is not a decimal
/// number, at most `MAX_SPAN`. More than `MAX_STR_DIGITS` digits is `Rejection::InvalidValue`,
/// where upstream's `int()` raises `ValueError`.
// upstream: main_extractor.py::_span
pub fn span(tree: &Tree, cell: NodeId, attr: &str) -> Result<usize, ExtractError> {
    let value = tree.get(cell, attr).unwrap_or("1");
    // isdecimal, not isdigit: int() rejects the superscripts isdigit() admits
    if !pystr::is_decimal(value) {
        return Ok(1);
    }
    if value.chars().count() > pystr::MAX_STR_DIGITS {
        return Err(ExtractError::Rejected(Rejection::InvalidValue));
    }
    // capped at every step: exact below `MAX_SPAN`, `MAX_SPAN` above it
    Ok(value
        .chars()
        .filter_map(pystr::decimal_value)
        .fold(0usize, |acc, digit| {
            let digit = usize::try_from(digit).unwrap_or(0);
            acc.saturating_mul(10).saturating_add(digit).min(MAX_SPAN)
        }))
}

/// Inserts empty placeholder cells for the columns a rowspan still occupies at the current
/// position of the row.
// upstream: main_extractor.py::_flush_rowspan_phantoms
fn flush_rowspan_phantoms(
    tree: &mut Tree,
    rowspan_map: &mut HashMap<usize, usize>,
    newrow: &mut Row,
    max_len: usize,
) -> Result<(), ExtractError> {
    while let Some(remaining) = rowspan_map.get_mut(&newrow.len) {
        let col = newrow.len;
        *remaining = remaining.saturating_sub(1);
        if *remaining == 0 {
            rowspan_map.remove(&col);
        }
        let phantom = define_cell_type(tree, false, max_len)?;
        newrow.push(tree, phantom);
    }
    Ok(())
}

/// Closes a row: trailing rowspan placeholders, padding to `max_cols`, and the row is kept only
/// when a cell holds something.
// upstream: main_extractor.py::_finalize_row
fn finalize_row(
    tree: &mut Tree,
    newtable: NodeId,
    newrow: &mut Row,
    rowspan_map: &mut HashMap<usize, usize>,
    max_cols: usize,
    max_len: usize,
) -> Result<(), ExtractError> {
    flush_rowspan_phantoms(tree, rowspan_map, newrow, max_len)?;
    let filled = tree.children(newrow.id).any(|cell| {
        has_content(tree.node(cell).text.as_deref()) || tree.first_child(cell).is_some()
    });
    // padding cells are empty, so they never change `filled`; a row that is dropped is not padded
    if !filled {
        return Ok(());
    }
    while newrow.len < max_cols {
        let padding = define_cell_type(tree, false, max_len)?;
        newrow.push(tree, padding);
    }
    tree.append(newtable, newrow.id);
    Ok(())
}

/// Extracts the content of a source `td` or `th` into the new `cell`, rewiring inline and block
/// children.
// upstream: main_extractor.py::_fill_cell
fn fill_cell(
    tree: &mut Tree,
    new_child_elem: NodeId,
    cell: NodeId,
    nested_elems: &HashSet<NodeId>,
    ptags_with_div: &[&str],
    ctx: &Ctx<'_>,
) -> Result<(), ExtractError> {
    if tree.first_child(cell).is_none() {
        if let Some(processed_cell) = process_node(tree, cell, ctx) {
            let node = tree.node(processed_cell);
            let (text, tail) = (node.text.clone(), node.tail.clone());
            let node = tree.node_mut(new_child_elem);
            node.text = text;
            node.tail = tail;
        }
        return Ok(());
    }
    let node = tree.node(cell);
    let (text, tail) = (node.text.clone(), node.tail.clone());
    let node = tree.node_mut(new_child_elem);
    node.text = text;
    node.tail = tail;
    // renamed before the inner walk so that handle_formatting wraps orphan spans in a `p`
    tree.set_name(cell, "done");
    let mut walk = LiveIter::descendants(tree, cell, &[]);
    let mut visited = 0usize;
    while let Some(child) = walk.next(tree) {
        tick(&mut visited, ctx.call)?;
        if tree.tag(child) == Some("done") {
            continue;
        }
        if nested_elems.contains(&child) {
            // nested tables are left to the main loop, their tail moves to the cell holding them;
            // a nested table always has an enclosing table, so a missing one cannot happen and
            // counts as not nested
            if tree.tag(child) == Some("table")
                && tree
                    .iterancestors(child, &["table"])
                    .next()
                    .is_none_or(|outer| !nested_elems.contains(&outer))
            {
                append_block(tree, new_child_elem, None, child);
                tree.node_mut(child).tail = None;
            }
            continue;
        }
        if separates_inline(tree, child) && !text_chars_test(tree.node(child).tail.as_deref()) {
            let linebreak = tree.new_element("lb");
            tree.append(new_child_elem, linebreak);
            tree.set_name(child, "done");
            continue;
        }
        let processed_subchild = if in_tags(tree, child, TABLE_ELEMS) {
            // stray cell from malformed HTML
            tree.set_name(child, "cell");
            handle_textnode(tree, child, ctx, true, true)
        } else if in_tags(tree, child, INLINE_CONSUMING) {
            let processed = handle_textnode(tree, child, ctx, true, true);
            // handle_textnode drops inline wrappers (ref/hi/del) with children but no direct text
            // (`<ref><hi>link text</hi></ref>`): carry the subtree directly instead
            if processed.is_none() && tree.first_child(child).is_some() {
                Some(child)
            } else {
                processed
            }
        } else if tree.tag(child) == Some("list") {
            let processed_list = handle_lists(tree, child, ctx)?;
            // lists in cells only in recall mode: keeping them otherwise is noise
            let kept = processed_list.filter(|_| ctx.options.focus == Focus::Recall);
            append_block(tree, new_child_elem, kept, child);
            continue;
        } else {
            handle_textelem(tree, child, ptags_with_div, ctx)?
        };
        define_newelem(tree, processed_subchild, new_child_elem, true, ctx.call)?;
        tree.set_name(child, "done");
    }
    Ok(())
}

/// Processes a single table element.
// upstream: main_extractor.py::handle_table
pub fn handle_table(
    tree: &mut Tree,
    table_elem: NodeId,
    potential_tags: &[&str],
    ctx: &Ctx<'_>,
) -> Result<Option<NodeId>, ExtractError> {
    handle_table_limited(tree, table_elem, potential_tags, ctx, MAX_OUTPUT_NODES)
}

/// `handle_table` with the arena size limit as a parameter, so tests can use a small one.
fn handle_table_limited(
    tree: &mut Tree,
    table_elem: NodeId,
    potential_tags: &[&str],
    ctx: &Ctx<'_>,
    max_len: usize,
) -> Result<Option<NodeId>, ExtractError> {
    let newtable = tree.new_element("table");
    let ptags_with_div: Vec<&str> = potential_tags.iter().copied().chain(["div"]).collect();

    tree.strip_tags(table_elem, &["thead", "tbody", "tfoot"]);

    // Everything inside a nested table: skipped in the cell walk without a `done` mark, so the
    // main loop can call handle_table on each nested table separately. A table inside one already
    // collected is covered by it.
    let mut nested_elems: HashSet<NodeId> = HashSet::new();
    let mut visited = 0usize;
    for nested_table in tree.iterdescendants(table_elem, &["table"]) {
        tick(&mut visited, ctx.call)?;
        if nested_elems.contains(&nested_table) {
            continue;
        }
        for node in tree.iter(nested_table, &[]) {
            tick(&mut visited, ctx.call)?;
            nested_elems.insert(node);
        }
    }

    // Direct children only, nested tables are left to the main loop. Orphan cells join the
    // current row but do not count for max_cols.
    let mut rows: Vec<Vec<NodeId>> = vec![Vec::new()];
    let mut captions: Vec<String> = Vec::new();
    let mut max_cols = 0usize;
    let children: Vec<NodeId> = tree.children(table_elem).collect();
    for elem in children {
        tick(&mut visited, ctx.call)?;
        if tree.tag(elem) == Some("tr") {
            let cells: Vec<NodeId> = tree
                .children(elem)
                .filter(|&cell| in_tags(tree, cell, TABLE_ELEMS))
                .collect();
            let mut width = 0usize;
            for &cell in &cells {
                tick(&mut visited, ctx.call)?;
                width = width.saturating_add(span(tree, cell, "colspan")?);
            }
            max_cols = max_cols.max(width);
            rows.push(cells);
        } else if in_tags(tree, elem, TABLE_ELEMS) {
            if let Some(row) = rows.last_mut() {
                row.push(elem);
            }
            continue;
        } else if tree.tag(elem) == Some("caption") {
            let pieces: Vec<&str> = tree.itertext_lxml(elem).collect();
            captions.push(pystr::strip(&pieces.join(" ")).to_string());
            // text consumed above, not to be emitted again, images are left to the main loop
            let mut walk = LiveIter::descendants(tree, elem, &[]);
            while let Some(sub) = walk.next(tree) {
                tick(&mut visited, ctx.call)?;
                if tree.tag(sub) == Some("graphic") {
                    tree.node_mut(sub).tail = None;
                } else {
                    tree.set_name(sub, "done");
                }
            }
        } else if tree.tag(elem) == Some("table") {
            continue;
        }
        tree.set_name(elem, "done");
    }
    let max_cols = max_cols.min(MAX_SPAN);

    for caption_text in captions.into_iter().filter(|caption| !caption.is_empty()) {
        let caption_cell = define_cell_type(tree, true, max_len)?;
        tree.node_mut(caption_cell).text = Some(caption_text);
        let mut caption_row = Row::new(tree);
        caption_row.push(tree, caption_cell);
        finalize_row(
            tree,
            newtable,
            &mut caption_row,
            &mut HashMap::new(),
            max_cols,
            max_len,
        )?;
    }

    let mut header_row_emitted = false;
    // column index to the rows still spanned from a rowspan cell
    let mut rowspan_map: HashMap<usize, usize> = HashMap::new();
    for cells in rows {
        tick(&mut visited, ctx.call)?;
        let mut newrow = Row::new(tree);
        let mut row_has_th = false;
        for cell in cells {
            tick(&mut visited, ctx.call)?;
            let is_header = tree.tag(cell) == Some("th") && !header_row_emitted;
            row_has_th = row_has_th || is_header;
            flush_rowspan_phantoms(tree, &mut rowspan_map, &mut newrow, max_len)?;
            let new_child_elem = define_cell_type(tree, is_header, max_len)?;
            let colspan = span(tree, cell, "colspan")?;
            // mark all spanned columns as occupied for the following rows
            let rowspan = span(tree, cell, "rowspan")?;
            if rowspan > 1 {
                for col in newrow.len..newrow.len.saturating_add(colspan) {
                    rowspan_map.insert(col, rowspan.saturating_sub(1));
                }
            }
            fill_cell(
                tree,
                new_child_elem,
                cell,
                &nested_elems,
                &ptags_with_div,
                ctx,
            )?;
            // keep empty cells so column positions stay aligned
            newrow.push(tree, new_child_elem);
            // inline colspan: pad with empty cells so the following rows align
            for _ in 1..colspan {
                let padding = define_cell_type(tree, is_header, max_len)?;
                newrow.push(tree, padding);
            }
            tree.set_name(cell, "done");
        }
        finalize_row(
            tree,
            newtable,
            &mut newrow,
            &mut rowspan_map,
            max_cols,
            max_len,
        )?;
        header_row_emitted = header_row_emitted || row_has_th;
    }
    Ok(tree.first_child(newtable).is_some().then_some(newtable))
}

/// Processes an image element and its relevant attributes. Without `options.url` a leading `//`
/// becomes `http://`; with it the source is joined to the page URL, which fails with
/// `Rejection::InvalidValue` where upstream's `urljoin` raises `ValueError`.
// upstream: main_extractor.py::handle_image
pub fn handle_image(
    tree: &mut Tree,
    element: NodeId,
    options: Option<&Extractor>,
) -> Result<Option<NodeId>, ExtractError> {
    let Some(link) = image_src(tree, element) else {
        return Ok(None);
    };
    let link = if link.starts_with("http") {
        link.to_string()
    } else if let Some(url) = options.and_then(|options| options.url.as_deref()) {
        urljoin(url, link).map_err(|_| ExtractError::Rejected(Rejection::InvalidValue))?
    } else {
        match link.strip_prefix("//") {
            Some(rest) => format!("http://{rest}"),
            None => link.to_string(),
        }
    };

    let alt = tree.get(element, "alt").map(str::to_string);
    let title = tree.get(element, "title").map(str::to_string);
    let tail = tree.node(element).tail.clone();
    let processed = tree.new_element_like(element);
    tree.set(processed, "src", &link);
    if let Some(alt) = alt.filter(|alt| !alt.is_empty()) {
        tree.set(processed, "alt", &alt);
    }
    if let Some(title) = title.filter(|title| !title.is_empty()) {
        tree.set(processed, "title", &title);
    }
    tree.node_mut(processed).tail = tail;
    Ok(Some(processed))
}

/// Processes a text element and determines how to deal with its content.
// upstream: main_extractor.py::handle_textelem
pub fn handle_textelem(
    tree: &mut Tree,
    element: NodeId,
    potential_tags: &[&str],
    ctx: &Ctx<'_>,
) -> Result<Option<NodeId>, ExtractError> {
    if tree.tag(element) == Some("list") {
        handle_lists(tree, element, ctx)
    } else if in_tags(tree, element, CODES_QUOTES) {
        handle_quotes(tree, element, ctx)
    } else if tree.tag(element) == Some("head") {
        handle_titles(tree, element, ctx)
    } else if tree.tag(element) == Some("p") {
        handle_paragraphs(tree, element, potential_tags, ctx)
    } else if tree.tag(element) == Some("lb") {
        if !text_chars_test(tree.node(element).tail.as_deref()) {
            return Ok(None);
        }
        let Some(this_element) = process_node(tree, element, ctx) else {
            return Ok(None);
        };
        let new_element = tree.new_element("p");
        tree.node_mut(new_element).text = tree.node(this_element).tail.clone();
        Ok(Some(new_element))
    } else if in_tags(tree, element, INLINE_CONSUMING) {
        Ok(handle_formatting(tree, element, ctx))
    } else if tree.tag(element) == Some("table") && potential_tags.contains(&"table") {
        handle_table(tree, element, potential_tags, ctx)
    } else if tree.tag(element) == Some("graphic") && potential_tags.contains(&"graphic") {
        handle_image(tree, element, Some(&ctx.options))
    } else {
        handle_other_elements(tree, element, potential_tags, ctx)
    }
}

/// Rule-based deletion of targeted document sections below `root`, in place; `keep_teasers`
/// spares the teaser-class blocks, which some sites wrap around real content.
// upstream: main_extractor.py::prune_unwanted_sections
pub fn prune_unwanted_sections(
    tree: &mut Tree,
    root: NodeId,
    potential_tags: &[&str],
    ctx: &Ctx<'_>,
    keep_teasers: bool,
) -> Result<(), ExtractError> {
    let call = ctx.call;
    let focus = ctx.options.focus;
    let favor_precision = focus == Focus::Precision;
    prune_unwanted_nodes(tree, root, OVERALL_DISCARD_XPATH, true, call)?;
    if !potential_tags.contains(&"graphic") {
        prune_unwanted_nodes(tree, root, DISCARD_IMAGE_ELEMENTS, false, call)?;
    }
    if focus != Focus::Recall {
        if !keep_teasers {
            prune_unwanted_nodes(tree, root, TEASER_DISCARD_XPATH, false, call)?;
        }
        if favor_precision {
            prune_unwanted_nodes(tree, root, PRECISION_DISCARD_XPATH, false, call)?;
        }
    }
    for _ in 0..2 {
        delete_by_link_density(tree, root, "div", true, favor_precision, call)?;
        delete_by_link_density(tree, root, "list", false, favor_precision, call)?;
        delete_by_link_density(tree, root, "p", false, favor_precision, call)?;
    }
    if potential_tags.contains(&"table") || favor_precision {
        // collected before deleting: a table removed mid-walk would take its nested tables with it
        let mut visited = 0usize;
        let mut boilerplate_tables = Vec::new();
        for table in tree.iter(root, &["table"]) {
            tick(&mut visited, call)?;
            if link_density_test_tables(tree, table, call)? {
                boilerplate_tables.push(table);
            }
        }
        for table in boilerplate_tables {
            tick(&mut visited, call)?;
            delete_element(tree, table, false);
        }
    }
    if favor_precision {
        let mut visited = 0usize;
        while let Some(last) = tree.last_child(root)
            && tree.tag(last) == Some("head")
        {
            tick(&mut visited, call)?;
            delete_element(tree, last, false);
        }
        delete_by_link_density(tree, root, "head", false, true, call)?;
        delete_by_link_density(tree, root, "quote", false, true, call)?;
    }
    // after the link density tests, which need the refs
    let stripped: &[&str] = if potential_tags.contains(&"ref") {
        &["span"]
    } else {
        &["span", "ref"]
    };
    tree.strip_tags(root, stripped);
    Ok(())
}

/// The `src` of every `graphic` at or below `node`; `None` for one without it.
fn graphic_sources<'t>(
    tree: &'t Tree,
    node: NodeId,
    call: &Call<'_>,
) -> Result<HashSet<Option<&'t str>>, ExtractError> {
    let mut visited = 0usize;
    let mut sources = HashSet::new();
    for graphic in tree.iter(node, &["graphic"]) {
        tick(&mut visited, call)?;
        sources.insert(tree.get(graphic, "src"));
    }
    Ok(sources)
}

/// Looks for the wild elements the main pass left, outside the determined frame and throughout
/// the document, and appends the ones whose text `result_body` does not hold yet. `potential_tags`
/// defaults to `TAG_CATALOG`; `consumed` holds elements the main pass already took.
///
/// Do not widen the search expression without benchmarking the full suite: recovered text raises
/// the extracted length, which can suppress the stronger rescues that run after this one.
// upstream: main_extractor.py::recover_wild_text
pub fn recover_wild_text(
    tree: &mut Tree,
    root: NodeId,
    result_body: NodeId,
    ctx: &Ctx<'_>,
    potential_tags: Option<&[&str]>,
    consumed: Option<&HashSet<NodeId>>,
) -> Result<(), ExtractError> {
    let call = ctx.call;
    // a copy: the recall branch adds tags, which must not leak back to the caller
    let mut potential_tags: Vec<&str> = potential_tags.unwrap_or(TAG_CATALOG).to_vec();
    let recall = ctx.options.focus == Focus::Recall;
    if recall {
        for tag in ["div", "lb"] {
            if !potential_tags.contains(&tag) {
                potential_tags.push(tag);
            }
        }
    }
    // blockquote, pre and q are already renamed to quote and code by convert_tags
    // deviation: D-1, keep_teasers is options.fast, which is true here
    prune_unwanted_sections(tree, root, &potential_tags, ctx, true)?;
    // `.//code|.//p|.//quote|.//table|.//div[contains(@class, 'w3-code')]`, plus
    // `|.//div|.//lb|.//list` in recall: one walk in document order
    let subelems = select::descendants(tree, root, call, |tree, node| match tree.tag(node) {
        Some("code" | "p" | "quote" | "table") => true,
        Some("div") => recall || select::attr(tree, node, "class").contains("w3-code"),
        Some("lb" | "list") => recall,
        _ => false,
    })?;
    // dedup against the snapshot taken before this pass: exact match, or a length-gated
    // substring (a paragraph folded into its list container)
    let mut visited = 0usize;
    let mut elem_texts: Vec<String> = Vec::new();
    for element in tree.children(result_body) {
        tick(&mut visited, call)?;
        elem_texts.push(elem_text(tree, element));
    }
    // newline-joined (trimmed element text has no newline), so no substring match spans two
    // elements; `existing_len` counts code points
    let mut existing = String::new();
    let mut existing_len = 0usize;
    for text in elem_texts.iter().filter(|text| !text.is_empty()) {
        if !existing.is_empty() {
            existing.push('\n');
            existing_len = existing_len.saturating_add(1);
        }
        existing.push_str(text);
        existing_len = existing_len.saturating_add(text.chars().count());
    }
    let mut existing_elems: HashSet<String> = elem_texts.into_iter().collect();
    // elements emitted or deduped whole: their descendants are covered
    let mut handled: HashSet<NodeId> = HashSet::new();
    for subelem in subelems {
        tick(&mut visited, call)?;
        if consumed.is_some_and(|consumed| consumed.contains(&subelem))
            || tree
                .iterancestors(subelem, &[])
                .any(|ancestor| handled.contains(&ancestor))
        {
            continue;
        }
        let Some(processed) = handle_textelem(tree, subelem, &potential_tags, ctx)? else {
            continue;
        };
        if processed == subelem {
            handled.insert(subelem);
        }
        let text = elem_text(tree, processed);
        // image-only blocks have no text to compare: check their sources against the live body
        if text.is_empty() {
            let images = graphic_sources(tree, processed, call)?;
            if !images.is_empty() && images.is_subset(&graphic_sources(tree, result_body, call)?) {
                continue;
            }
        }
        // past the cap the substring scan is skipped and `existing` stops growing
        let under_cap = existing_len <= DEDUPE_SCAN_CAP;
        // a copy carrying its source tail is a fragment of a text run: no length gate
        let tail = tree.node(processed).tail.as_deref();
        let fragment = if text_chars_test(tail) {
            trim(&format!("{text}{}", tail.unwrap_or_default()))
        } else {
            String::new()
        };
        if !text.is_empty()
            && (existing_elems.contains(&text)
                || (under_cap
                    && ((text.chars().count() > MIN_DUPLICATE_LENGTH && existing.contains(&text))
                        || (!fragment.is_empty() && existing.contains(&fragment)))))
        {
            continue;
        }
        tree.append(result_body, processed);
        if under_cap {
            let added = if fragment.is_empty() {
                &text
            } else {
                &fragment
            };
            existing.push('\n');
            existing.push_str(added);
            existing_len = existing_len
                .saturating_add(1)
                .saturating_add(added.chars().count());
        }
        existing_elems.insert(text);
    }
    Ok(())
}

/// `" ".join(element.itertext()).strip()`: lxml's empty text nodes count as pieces, so two adjacent
/// separators can leave a double space.
fn joined_text(tree: &Tree, element: NodeId) -> String {
    let pieces: Vec<&str> = tree.itertext_lxml(element).collect();
    pystr::strip(&pieces.join(" ")).to_string()
}

/// `subtree.xpath("//p//text()")`: whether the text nodes of the paragraphs of the whole document
/// exist at all (an empty one counts), and their joined length in code points. A paragraph inside
/// another is read once, with the outer one.
fn paragraph_text(
    tree: &Tree,
    document: NodeId,
    call: &Call<'_>,
) -> Result<(bool, usize), ExtractError> {
    let mut visited = 0usize;
    let mut found = false;
    let mut length = 0usize;
    for paragraph in tree.iter(document, &["p"]) {
        tick(&mut visited, call)?;
        if tree.iterancestors(paragraph, &["p"]).next().is_some() {
            continue;
        }
        for piece in tree.itertext_lxml(paragraph) {
            tick(&mut visited, call)?;
            found = true;
            length = length.saturating_add(piece.chars().count());
        }
    }
    Ok((found, length))
}

/// Finds the main content below `root` with the `BODY_XPATH` expressions and returns the body it
/// builds, its text and the tags it allowed.
// upstream: main_extractor.py::_extract
fn extract_body(
    tree: &mut Tree,
    root: NodeId,
    ctx: &Ctx<'_>,
) -> Result<(NodeId, String, Vec<&'static str>), ExtractError> {
    let call = ctx.call;
    let options = &ctx.options;
    let mut potential_tags: Vec<&'static str> = TAG_CATALOG.to_vec();
    if options.tables {
        potential_tags.extend(["table", "td", "th", "tr"]);
    }
    if options.images {
        potential_tags.push("graphic");
    }
    if options.links {
        potential_tags.push("ref");
    }
    let result_body = tree.new_element("body");
    // `//` from any node reads the document the node is in, and the frame may be detached from it
    // by the time it is read
    let document = tree.root_of(root);
    let factor = if options.focus == Focus::Precision {
        1
    } else {
        3
    };
    for expr in BODY_XPATH {
        let Some(subtree) = expr(tree, root, call)?.into_iter().next() else {
            continue;
        };
        prune_unwanted_sections(tree, subtree, &potential_tags, ctx, false)?;
        if tree.child_count(subtree) == 0 {
            continue;
        }
        let (has_text, text_len) = paragraph_text(tree, document, call)?;
        if (!has_text || text_len < options.min_extracted_size.saturating_mul(factor))
            && !potential_tags.contains(&"div")
        {
            potential_tags.push("div");
        }

        let mut subelems = select::descendants(tree, subtree, call, |_, _| true)?;
        // a single text run (only lb or inline elements): process the frame itself. A foreign
        // element (svg, math) is neither: on the html5 tree lxml names it `{namespace}name`
        // (D-2), and the port compares by namespace and name.
        let (mut only_lb, mut only_inline) = (!subelems.is_empty(), true);
        let mut visited = 0usize;
        for &subelem in &subelems {
            tick(&mut visited, call)?;
            let tag = tree.tag(subelem);
            only_lb &= tag == Some("lb");
            only_inline &= tag.is_some_and(|tag| tag == "lb" || INLINE_FORMATTABLE.contains(&tag));
        }
        let single_run = only_lb
            || (tree.tag(subtree) == Some("div") && potential_tags.contains(&"div") && only_inline);
        if single_run {
            subelems = vec![subtree];
        }
        // the frame's own text before its first child, kept if the frame is accepted
        let start = tree.child_count(result_body);
        let lead = if !single_run && text_chars_test(tree.node(subtree).text.as_deref()) {
            let lead = tree.new_element("p");
            tree.node_mut(lead).text = tree.node(subtree).text.clone();
            process_node(tree, lead, ctx)
        } else {
            None
        };
        for elem in subelems {
            tick(&mut visited, call)?;
            // a text-bearing div moves into result_body as a whole, and its descendants are still
            // in `subelems`: revisiting one would emit its text twice
            if tree.root_of(elem) == result_body {
                continue;
            }
            // handlers may rename the element to "done", so read these first
            let rebuilt = in_tags(tree, elem, REBUILT_BLOCKS);
            let tail = tree.node(elem).tail.clone();
            let processed = handle_textelem(tree, elem, &potential_tags, ctx)?;
            if let Some(processed) = processed {
                tree.append(result_body, processed);
                // a copy leaves its source behind for a later, wider subtree
                if tree.root_of(elem) != result_body {
                    tree.set_name(elem, "done");
                }
            }
            // text right after a rebuilt block is a paragraph of its own, like an lb tail, unless
            // the handler already carried it over (a code block copied as a whole)
            if rebuilt
                && text_chars_test(tail.as_deref())
                && processed.is_none_or(|processed| tree.node(processed).tail.is_none())
            {
                let tail_elem = tree.new_element("p");
                tree.node_mut(tail_elem).text = tail;
                if process_node(tree, tail_elem, ctx).is_some() {
                    tree.append(result_body, tail_elem);
                }
            }
        }
        while let Some(last) = tree.last_child(result_body)
            && in_tags(tree, last, NOT_AT_THE_END)
        {
            tick(&mut visited, call)?;
            delete_element(tree, last, false);
        }
        // exit once there is real content, not just a lone image
        let content = tree
            .children(result_body)
            .filter(|&child| tree.tag(child) != Some("graphic"))
            .take(2)
            .count();
        if content > 1 {
            if let Some(lead) = lead {
                tree.insert(result_body, start, lead);
            }
            break;
        }
    }
    let temp_text = joined_text(tree, result_body);
    Ok((result_body, temp_text, potential_tags))
}

/// Finds the main content of a page using a set of XPath expressions, then extracts relevant
/// elements, strips them of unwanted subparts and converts them. Returns the body and the text
/// taken before the final cleanup; the tree is edited in place.
// upstream: main_extractor.py::extract_content
pub fn extract_content(
    tree: &mut Tree,
    cleaned_root: NodeId,
    ctx: &Ctx<'_>,
) -> Result<(NodeId, String), ExtractError> {
    let call = ctx.call;
    call.check()?;
    let backup = tree.copy(cleaned_root);
    // source order before the main pass moves elements, the backup keeps it
    let mut visited = 0usize;
    let mut order = Vec::new();
    for node in tree.iter(cleaned_root, &[]) {
        tick(&mut visited, call)?;
        order.push(node);
    }

    let (result_body, mut temp_text, potential_tags) = extract_body(tree, cleaned_root, ctx)?;

    if tree.child_count(result_body) == 0
        || temp_text.chars().count() < ctx.options.min_extracted_size
    {
        // copies of what the main pass consumed are not recovered again. Both walks cover the same
        // tree, taken before anything moved, so the pairs line up.
        let mut consumed = HashSet::new();
        let twins: Vec<NodeId> = tree.iter(backup, &[]).collect();
        // upstream zips with strict=True; Tree::copy is contiguous in document order
        debug_assert_eq!(order.len(), twins.len());
        for (&elem, &twin) in order.iter().zip(&twins) {
            tick(&mut visited, call)?;
            if tree.tag(elem) == Some("done") {
                consumed.insert(twin);
            }
        }
        recover_wild_text(
            tree,
            backup,
            result_body,
            ctx,
            Some(potential_tags.as_slice()),
            Some(&consumed),
        )?;
        temp_text = joined_text(tree, result_body);
    }
    // drop substantial elements repeating the previous one (overlapping-candidate or recovery
    // artifact); length-gated so short genuine repeats stay for the dedup and the tree-size guards
    let mut previous: Option<String> = None;
    let children: Vec<NodeId> = tree.children(result_body).collect();
    for child in children {
        tick(&mut visited, call)?;
        let current = elem_text(tree, child);
        if !current.is_empty()
            && previous.as_deref() == Some(current.as_str())
            && current.chars().count() > MIN_DUPLICATE_LENGTH
        {
            delete_element(tree, child, false);
        } else {
            previous = Some(current);
        }
    }
    tree.strip_elements(result_body, &["done"], true);
    tree.strip_tags(result_body, &["div"]);
    // stripping the divs merges their indentation into the preceding lb's tail, so an lb that only
    // separated pruned blocks ends up carrying a run of whitespace. Inside code that whitespace is
    // the indentation itself.
    let mut linebreaks = Vec::new();
    for linebreak in tree.iter(result_body, &["lb"]) {
        tick(&mut visited, call)?;
        linebreaks.push(linebreak);
    }
    for linebreak in linebreaks {
        tick(&mut visited, call)?;
        let tail = tree.node(linebreak).tail.as_deref();
        if has_content(tail)
            && !text_chars_test(tail)
            && tree
                .iterancestors(linebreak, SPACING_PROTECTED)
                .next()
                .is_none()
        {
            tree.node_mut(linebreak).tail = None;
        }
    }
    Ok((result_body, temp_text))
}

/// Tries to extract the comments out of the potential sections of the page; the section the
/// comments came from is deleted from the tree.
// upstream: main_extractor.py::extract_comments
pub fn extract_comments(
    tree: &mut Tree,
    root: NodeId,
    ctx: &Ctx<'_>,
) -> Result<(NodeId, String), ExtractError> {
    let call = ctx.call;
    let comments_body = tree.new_element("body");
    // TAG_CATALOG, not div: trouble with <div class="comment-author meta">
    for expr in COMMENTS_XPATH {
        let Some(subtree) = expr(tree, root, call)?.into_iter().next() else {
            continue;
        };
        prune_unwanted_nodes(tree, subtree, COMMENTS_DISCARD_XPATH, false, call)?;
        tree.strip_tags(subtree, &["ref", "span"]);
        // collected first: an element moved into the body leaves its place in the section
        let mut visited = 0usize;
        for elem in select::descendants(tree, subtree, call, |_, _| true)? {
            tick(&mut visited, call)?;
            if !in_tags(tree, elem, TAG_CATALOG) {
                continue;
            }
            if let Some(processed) = handle_textnode(tree, elem, ctx, true, false) {
                tree.clear_attrs(processed);
                tree.append(comments_body, processed);
            }
        }
        if tree.child_count(comments_body) > 0 {
            delete_element(tree, subtree, false);
            break;
        }
    }
    let text = joined_text(tree, comments_body);
    Ok((comments_body, text))
}

#[cfg(test)]
mod tests;
