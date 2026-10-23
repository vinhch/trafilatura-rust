//! The main extraction handlers of upstream `main_extractor.py`.
//!
//! Every handler takes the tree and the element it works on, and the `Ctx` (or the `Call`) where
//! it loops over nodes. A handler returns the element it built or reused, `None` where upstream
//! returns `None`.

use std::collections::HashSet;

use crate::cancel::Call;
use crate::context::Ctx;
use crate::dom::{LiveIter, LocalName, NodeId, Ns, Tree};
use crate::error::{ExtractError, Rejection};
use crate::html_processing::{handle_textnode, has_content, process_node, tick};
use crate::pystr;
use crate::pyurl::urljoin;
use crate::settings::{Extractor, INLINE_CARRIED, INLINE_CONSUMING};
use crate::utils::{FORMATTING_PROTECTED, image_src, text_chars_test, trim};
use crate::xml::delete_element;

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
        for sub in subs {
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
    // no children
    if tree.child_count(element) == 0 {
        return Ok(process_node(tree, element, ctx));
    }

    // children
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
            // handle formatting
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
                // check depth and clean
                let items: Vec<NodeId> = tree.children(processed_child).collect();
                for &item in &items {
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
    // finish
    if let Some(last) = tree.last_child(processed_element) {
        // clean trailing lb-elements
        if tree.tag(last) == Some("lb") && tree.node(last).tail.is_none() {
            delete_element(tree, last, true);
        }
        return Ok(Some(processed_element));
    }
    // text_chars_test, not truthiness: layout wrappers (clearfix/spacer divs, cleared-out ad
    // slots) leave indentation-only text
    Ok(text_chars_test(tree.node(processed_element).text.as_deref()).then_some(processed_element))
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

#[cfg(test)]
mod tests;
