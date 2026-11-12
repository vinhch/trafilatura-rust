//! The body, comment and section handlers of upstream `main_extractor.py`.

use std::collections::HashSet;

use crate::cancel::Call;
use crate::context::Ctx;
use crate::dom::{NodeId, Tree};
use crate::error::ExtractError;
use crate::html_processing::{
    delete_by_link_density, handle_textnode, has_content, link_density_test_tables, process_node,
    prune_unwanted_nodes, tick,
};
use crate::pystr;
use crate::settings::{
    DEDUPE_SCAN_CAP, Focus, INLINE_FORMATTABLE, MIN_DUPLICATE_LENGTH, TAG_CATALOG,
};
use crate::utils::{SPACING_PROTECTED, text_chars_test, trim};
use crate::xml::delete_element;
use crate::xpaths::select;
use crate::xpaths::{
    BODY_XPATH, COMMENTS_DISCARD_XPATH, COMMENTS_XPATH, DISCARD_IMAGE_ELEMENTS,
    OVERALL_DISCARD_XPATH, PRECISION_DISCARD_XPATH, TEASER_DISCARD_XPATH,
};

use super::{NOT_AT_THE_END, REBUILT_BLOCKS, elem_text, handle_textelem, in_tags};

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
pub(super) fn extract_body(
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
        // element (svg, math) is neither: the name lists decide, not the namespace.
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
    let backup = tree.copy(cleaned_root, call)?;
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
