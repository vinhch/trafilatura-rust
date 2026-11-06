//! Tree cleaning and pruning of upstream `htmlprocessing.py`.
//!
//! Every function takes the tree and the element it works on, and the `Call` (directly or through
//! the `Ctx`) where it loops over nodes.

use std::collections::HashSet;

use crate::cancel::{CHECK_INTERVAL, Call};
use crate::context::Ctx;
use crate::deduplication::duplicate_test;
use crate::dom::{LiveIter, NodeId, Tree};
use crate::error::ExtractError;
use crate::pystr;
use crate::settings::{CUT_EMPTY_ELEMS, Focus, MANUALLY_CLEANED, MANUALLY_STRIPPED};
use crate::utils::{LINK_FARM_RATIO, image_src, textfilter, trim};
use crate::xml::{delete_element, separates_inline};
use crate::xpaths::Selector;
use crate::xpaths::select::descendants;

/// Formatting tags and the `rend` value each becomes, in upstream's order.
// upstream: htmlprocessing.py::REND_TAG_MAPPING
pub const REND_TAG_MAPPING: &[(&str, &str)] = &[
    ("em", "#i"),
    ("i", "#i"),
    ("b", "#b"),
    ("strong", "#b"),
    ("u", "#u"),
    ("kbd", "#t"),
    ("samp", "#t"),
    ("tt", "#t"),
    ("var", "#t"),
    ("sub", "#sub"),
    ("sup", "#sup"),
];

/// The tag each `rend` value of a `hi` element becomes in HTML: `REND_TAG_MAPPING` inverted, where
/// the last tag of a `rend` wins (`#i` gives `i`, `#t` gives `var`).
// upstream: htmlprocessing.py::HTML_TAG_MAPPING
pub const HTML_TAG_MAPPING: &[(&str, &str)] = &[
    ("#i", "i"),
    ("#b", "strong"),
    ("#u", "u"),
    ("#t", "var"),
    ("#sub", "sub"),
    ("#sup", "sup"),
];

/// Containers of images that cleaning keeps when images are on.
// upstream: htmlprocessing.py::PRESERVE_IMG_CLEANING
pub const PRESERVE_IMG_CLEANING: &[&str] = &["figure", "picture", "source"];

/// Substrings that mark text as code.
// upstream: htmlprocessing.py::CODE_INDICATORS
pub const CODE_INDICATORS: &[&str] = &["{", "(\"", "('", "\n    "];

/// `.//*[local-name()="annotation"][@encoding="application/x-tex"]`: the LaTeX source carried by
/// MathML. `local-name()` ignores the namespace, which XHTML pages keep on the subtree.
// upstream: htmlprocessing.py::TEX_ANNOTATION_XPATH
const TEX_ANNOTATION_XPATH: Selector = tex_annotation_xpath;

fn tex_annotation_xpath(
    tree: &Tree,
    context: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    descendants(tree, context, call, |tree, id| {
        &*tree.node(id).name == "annotation"
            && tree.get(id, "encoding") == Some("application/x-tex")
    })
}

/// The elements of `nodes` named one of `tags`, in order; checks `call` about every
/// `CHECK_INTERVAL` nodes it walks, matching or not.
fn collect_tagged(
    tree: &Tree,
    call: &Call<'_>,
    nodes: impl Iterator<Item = NodeId>,
    tags: &[&str],
) -> Result<Vec<NodeId>, ExtractError> {
    let mut found = Vec::new();
    for (index, node) in nodes.enumerate() {
        if index.is_multiple_of(CHECK_INTERVAL) {
            call.check()?;
        }
        if tree.tag(node).is_some_and(|name| tags.contains(&name)) {
            found.push(node);
        }
    }
    Ok(found)
}

/// Removes the first `item` from `list`, like Python's `list.remove`; false when it is not there.
fn remove_first(list: &mut Vec<&str>, item: &str) -> bool {
    match list.iter().position(|&listed| listed == item) {
        Some(index) => {
            list.remove(index);
            true
        }
        None => false,
    }
}

/// Turns MathML into its LaTeX source, in the tail of the `math` element, so formulas survive the
/// deletion of `math` with its subtree. The annotation wins over `alttext`; an element with no
/// source is left alone. Checks `call` about every `CHECK_INTERVAL` nodes.
// upstream: htmlprocessing.py::recover_math
pub fn recover_math(tree: &mut Tree, root: NodeId, call: &Call<'_>) -> Result<(), ExtractError> {
    // the loop edits tails only, so the collected list is what a live walk would visit
    for element in collect_tagged(tree, call, tree.iter(root, &[]), &["math"])? {
        let annotation = TEX_ANNOTATION_XPATH(tree, element, call)?;
        let latex = match annotation.first() {
            Some(&first) => trim(tree.node(first).text.as_deref().unwrap_or("")),
            None => trim(tree.get(element, "alttext").unwrap_or("")),
        };
        if latex.is_empty() {
            continue;
        }
        // delimiters the Markdown converter understands
        let (opening, closing) = if tree.get(element, "display") == Some("block") {
            ("\\[", "\\]")
        } else {
            ("\\(", "\\)")
        };
        let node = tree.node_mut(element);
        let tail = node.tail.take().unwrap_or_default();
        node.tail = Some(format!("{opening}{latex}{closing}{tail}"));
    }
    Ok(())
}

/// Deletes the `form` elements below `root`, except those holding more than half of the text of
/// the tree: a layout wrapper (ASP.NET WebForms puts the whole page in one form) becomes a `div`.
/// Checks `call` about every `CHECK_INTERVAL` nodes.
// upstream: htmlprocessing.py::_handle_forms
fn handle_forms(tree: &mut Tree, root: NodeId, call: &Call<'_>) -> Result<(), ExtractError> {
    let forms = collect_tagged(tree, call, tree.iter(root, &[]), &["form"])?;
    if forms.is_empty() {
        return Ok(());
    }
    // measured once, after the other elements are gone, so script and head text cannot skew it
    // upstream counts the untrimmed text_content, so char_len, not trimmed_len
    let total = char_len(tree, root);
    for (index, form) in forms.into_iter().enumerate() {
        if index.is_multiple_of(CHECK_INTERVAL) {
            call.check()?;
        }
        if total != 0 && char_len(tree, form) as f64 > total as f64 / 2.0 {
            tree.set_name(form, "div");
        } else {
            delete_element(tree, form, true);
        }
    }
    Ok(())
}

/// Prunes the tree below `root` by discarding the unwanted elements of `MANUALLY_CLEANED` and
/// `MANUALLY_STRIPPED`; returns the root to continue with, which is `root` except when recall
/// restores its copy. Checks `ctx.call` about every `CHECK_INTERVAL` nodes of the walks it makes
/// itself and of the elements a live walk matches, and once per cleaning expression whose tag the
/// tree holds; `strip_tags`, `copy` and the start of a live walk take no `Call`.
// upstream: htmlprocessing.py::tree_cleaning
pub fn tree_cleaning(tree: &mut Tree, root: NodeId, ctx: &Ctx<'_>) -> Result<NodeId, ExtractError> {
    tree_cleaning_with(tree, root, ctx, MANUALLY_CLEANED, MANUALLY_STRIPPED)
}

/// `tree_cleaning` over the given lists in place of `MANUALLY_CLEANED` and `MANUALLY_STRIPPED`,
/// which users mutate upstream. With `images` on, `stripped` is expected to hold `img`; without it
/// nothing is removed from the list and nothing is raised, where upstream's `ValueError` discards the
/// document (deviation D-8).
// upstream: htmlprocessing.py::tree_cleaning
pub fn tree_cleaning_with(
    tree: &mut Tree,
    root: NodeId,
    ctx: &Ctx<'_>,
    cleaned: &[&str],
    stripped: &[&str],
) -> Result<NodeId, ExtractError> {
    let call = ctx.call;
    let mut root = root;
    // salvage formulas before <math> is discarded along with its subtree
    recover_math(tree, root, call)?;
    let mut cleaning_list = cleaned.to_vec();
    let mut stripping_list = stripped.to_vec();
    // forms are handled below, once the rest of the noise is gone; a list without "form" keeps them
    let clean_forms = remove_first(&mut cleaning_list, "form");
    if !ctx.options.tables {
        cleaning_list.extend(["table", "td", "th", "tr"]);
    } else {
        // figures holding a table (#301) and ARIA layout tables (role=presentation/none); the
        // renaming goes through a live walk, as upstream edits inside `tree.iter`
        let tags = ["figure", "table"];
        let mut walk = LiveIter::new(tree, root, &tags);
        let mut visited = 0usize;
        while let Some(element) = walk.next(tree) {
            if visited.is_multiple_of(CHECK_INTERVAL) {
                call.check()?;
            }
            visited = visited.saturating_add(1);
            let reclassify = if tree.tag(element) == Some("figure") {
                tree.find(element, "table").is_some()
            } else {
                matches!(tree.get(element, "role"), Some("presentation" | "none"))
            };
            if reclassify {
                tree.set_name(element, "div");
            }
        }
    }
    if ctx.options.images {
        // many websites have <img> inside <figure> or <picture> or <source> tag
        cleaning_list.retain(|tag| !PRESERVE_IMG_CLEANING.contains(tag));
        // deviation: D-8
        remove_first(&mut stripping_list, "img");
    }

    // strip targeted elements
    tree.strip_tags(root, &stripping_list);

    // recall: undo the deletions if they remove every paragraph (copy only if none can survive)
    let needs_backup = ctx.options.focus == Focus::Recall
        && tree.find(root, "p").is_some()
        && (cleaning_list.contains(&"p")
            || !any_paragraph_survives(tree, root, &cleaning_list, call)?);
    let backup = if needs_backup {
        Some(tree.copy(root, call)?)
    } else {
        None
    };
    // the loop only deletes, so a tag absent now stays absent; `tree.tag` matches the HTML namespace
    // only, as `LiveIter` does
    let mut present = HashSet::new();
    for (index, node) in tree.iter(root, &[]).enumerate() {
        if index.is_multiple_of(CHECK_INTERVAL) {
            call.check()?;
        }
        if let Some(name) = tree.tag(node)
            && let Some(&entry) = cleaning_list.iter().find(|&&entry| entry == name)
        {
            present.insert(entry);
        }
    }
    for expression in cleaning_list
        .iter()
        .filter(|&expression| present.contains(expression))
    {
        // a live walk: upstream deletes inside `tree.iter(expression)`, and lxml's walk skips what
        // follows a deleted subtree
        call.check()?;
        let tags = [*expression];
        let mut walk = LiveIter::new(tree, root, &tags);
        let mut visited = 0usize;
        while let Some(element) = walk.next(tree) {
            if visited.is_multiple_of(CHECK_INTERVAL) {
                call.check()?;
            }
            visited = visited.saturating_add(1);
            delete_element(tree, element, true);
        }
    }
    if let Some(copy) = backup
        && tree.find(root, "p").is_none()
    {
        root = copy;
    }

    // the readers from here on would see a script or style without its text; `bare_extraction`
    // then runs again on a full parse, so the output stays upstream's
    if tree.holds_dropped_text(root, call)? {
        tree.needs_script_text = true;
    }

    if clean_forms {
        handle_forms(tree, root, call)?;
    }

    prune_html(tree, root, ctx.options.focus, call)?;
    Ok(root)
}

/// Whether a `p` has no ancestor in `cleaning_list` below `root` (`root` itself counts as none):
/// `any(next(p.iterancestors(cleaning_list), tree) is tree for p in tree.iter("p"))`.
fn any_paragraph_survives(
    tree: &Tree,
    root: NodeId,
    cleaning_list: &[&str],
    call: &Call<'_>,
) -> Result<bool, ExtractError> {
    for (index, node) in tree.iter(root, &[]).enumerate() {
        if index.is_multiple_of(CHECK_INTERVAL) {
            call.check()?;
        }
        if tree.tag(node) != Some("p") {
            continue;
        }
        // lxml matches no ancestor against an empty list; an empty `tags` here would match them all
        let cleaned_ancestor = if cleaning_list.is_empty() {
            None
        } else {
            tree.iterancestors(node, cleaning_list).next()
        };
        if cleaned_ancestor.is_none_or(|ancestor| ancestor == root) {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Deletes the empty elements of `CUT_EMPTY_ELEMS` below `root` to save space and processing time;
/// their tails stay, except for the `Precision` focus. Checks `call` about every `CHECK_INTERVAL`
/// nodes.
// upstream: htmlprocessing.py::prune_html
pub fn prune_html(
    tree: &mut Tree,
    root: NodeId,
    focus: Focus,
    call: &Call<'_>,
) -> Result<(), ExtractError> {
    let tails = focus != Focus::Precision;
    let candidates = collect_tagged(tree, call, tree.iterdescendants(root, &[]), CUT_EMPTY_ELEMS)?;
    let empty = candidates
        .into_iter()
        .filter(|&element| tree.node(element).text.is_none() && tree.first_child(element).is_none())
        .collect::<Vec<_>>();
    for element in empty {
        delete_element(tree, element, tails);
    }
    Ok(())
}

/// Checks `call` when `visited` is a multiple of `CHECK_INTERVAL`, then counts the node.
pub(crate) fn tick(visited: &mut usize, call: &Call<'_>) -> Result<(), ExtractError> {
    if visited.is_multiple_of(CHECK_INTERVAL) {
        call.check()?;
    }
    *visited = visited.saturating_add(1);
    Ok(())
}

/// Prunes the tree below `root` by deleting what each selector of `nodelist` matches; the text
/// after a deleted element stays. With `with_backup`, nothing is deleted when the outer matches
/// would take all but a seventh of the text (the guard against over-pruning). Checks `call` about
/// every `CHECK_INTERVAL` nodes.
// upstream: htmlprocessing.py::prune_unwanted_nodes
pub fn prune_unwanted_nodes(
    tree: &mut Tree,
    root: NodeId,
    nodelist: &[Selector],
    with_backup: bool,
    call: &Call<'_>,
) -> Result<(), ExtractError> {
    if !with_backup {
        for select in nodelist {
            // each result is complete before the first deletion, as an XPath result is
            for subtree in select(tree, root, call)? {
                delete_element(tree, subtree, true);
            }
        }
        return Ok(());
    }
    // `dict.fromkeys`: matches in first-seen order, each once
    let mut matches = Vec::new();
    let mut matched = HashSet::new();
    for select in nodelist {
        for element in select(tree, root, call)? {
            if matched.insert(element) {
                matches.push(element);
            }
        }
    }
    let mut visited = 0usize;
    let mut outer = Vec::new();
    for &element in &matches {
        tick(&mut visited, call)?;
        if !tree
            .iterancestors(element, &[])
            .any(|ancestor| matched.contains(&ancestor))
        {
            outer.push(element);
        }
    }
    let old_len = char_len(tree, root);
    let removed = outer.iter().fold(0usize, |sum, &element| {
        sum.saturating_add(char_len(tree, element))
    });
    // todo: adjust for recall and precision settings
    if old_len as f64 - removed as f64 > old_len as f64 / 7.0 {
        for element in outer {
            delete_element(tree, element, true);
        }
    }
    Ok(())
}

/// Heuristics on the text of `links`, the empty ones dropped: the summed length of their trimmed
/// text, their count, how many are shorter than 10 characters, and the texts. Lengths are code
/// points. Checks `call` about every `CHECK_INTERVAL` links.
// upstream: htmlprocessing.py::collect_link_info
pub fn collect_link_info(
    tree: &Tree,
    links: &[NodeId],
    call: &Call<'_>,
) -> Result<(usize, usize, usize, Vec<String>), ExtractError> {
    let mut visited = 0usize;
    let mut texts = Vec::new();
    for &link in links {
        tick(&mut visited, call)?;
        let text = trim(&tree.text_content(link));
        if !text.is_empty() {
            texts.push(text);
        }
    }
    let lengths: Vec<usize> = texts.iter().map(|text| text.chars().count()).collect();
    // longer strings impact recall in favor of precision
    let short = lengths.iter().filter(|&&length| length < 10).count();
    let total = lengths
        .iter()
        .fold(0usize, |sum, &length| sum.saturating_add(length));
    Ok((total, texts.len(), short, texts))
}

/// Whether every link of `links` stands alone in its own paragraph: a document listing, as against
/// a farm of links running together. Checks `call` about every `CHECK_INTERVAL` nodes.
// upstream: htmlprocessing.py::is_paragraph_listing
pub fn is_paragraph_listing(
    tree: &Tree,
    links: &[NodeId],
    call: &Call<'_>,
) -> Result<bool, ExtractError> {
    let mut visited = 0usize;
    for &link in links {
        tick(&mut visited, call)?;
        let Some(parent) = tree.node(link).parent() else {
            return Ok(false);
        };
        if tree.tag(parent) != Some("p") {
            return Ok(false);
        }
        // `len(parent.findall(".//ref")) > 1`: stops at the second one
        let mut refs = 0usize;
        for node in tree.iterdescendants(parent, &[]) {
            tick(&mut visited, call)?;
            if tree.tag(node) == Some("ref") {
                refs = refs.saturating_add(1);
                if refs > 1 {
                    return Ok(false);
                }
            }
        }
    }
    Ok(true)
}

/// The `ref` descendants of `element`, in document order.
fn find_refs(tree: &Tree, element: NodeId, call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    collect_tagged(tree, call, tree.iterdescendants(element, &[]), &["ref"])
}

/// Characters in `trim(&tree.text_content(id))`, counted without building either string.
fn trimmed_len(tree: &Tree, id: NodeId) -> usize {
    let mut len = 0usize;
    // `text_content` joins the pieces with nothing between them, so a word can span several
    let mut in_word = false;
    for c in tree.itertext(id).flat_map(str::chars) {
        if pystr::is_space(c) {
            in_word = false;
            continue;
        }
        // a word is at least one character, so `len > 0` means an earlier word needs a separator
        if !in_word && len > 0 {
            len = len.saturating_add(1);
        }
        in_word = true;
        len = len.saturating_add(1);
    }
    len
}

/// Characters in `tree.text_content(id)`, counted without building the string.
pub fn char_len(tree: &Tree, id: NodeId) -> usize {
    tree.itertext(id).fold(0usize, |sum, piece| {
        sum.saturating_add(piece.chars().count())
    })
}

/// Whether `element` is rich in links (probably boilerplate), and whether it is a short one with
/// links. Checks `call` about every `CHECK_INTERVAL` nodes.
// upstream: htmlprocessing.py::link_density_test
pub fn link_density_test(
    tree: &Tree,
    element: NodeId,
    favor_precision: bool,
    call: &Call<'_>,
) -> Result<(bool, bool), ExtractError> {
    let links = find_refs(tree, element, call)?;
    if links.is_empty() {
        return Ok((false, false));
    }
    // preserve image containers
    let graphics = collect_tagged(tree, call, tree.iterdescendants(element, &[]), &["graphic"])?;
    if !graphics.is_empty() {
        return Ok((false, false));
    }
    // same count as len(trim(text_content)), without building the strings
    let text_len = trimmed_len(tree, element);
    // shortcut
    if let [only] = links[..] {
        let len_threshold = if favor_precision { 10 } else { 100 };
        let link_len = trimmed_len(tree, only);
        if link_len > len_threshold && link_len as f64 > text_len as f64 * 0.9 {
            return Ok((true, false));
        }
    }
    let has_next = tree.node(element).next_sibling().is_some();
    let limit_len = match (tree.tag(element) == Some("p"), has_next) {
        (true, false) => 60,
        (true, true) => 30,
        (false, false) => 300,
        (false, true) => 100,
    };
    if text_len < limit_len {
        let (link_len, elem_num, short_elems, _) = collect_link_info(tree, &links, call)?;
        if elem_num == 0 {
            return Ok((true, false));
        }
        let dense = link_len as f64 > text_len as f64 * 0.8
            || (elem_num > 1 && short_elems as f64 / elem_num as f64 > 0.8);
        return Ok((dense, true));
    }
    // large near-total-link farms ("latest news" sidebars) at or above limit_len, which the size
    // gate above never tests; more than 4 links: a handful of long sentence-links is editorial
    if links.len() > 4 {
        let (link_len, elem_num, _, _) = collect_link_info(tree, &links, call)?;
        // an average link length of 100 or more is a catalog or listing (one link per card), kept;
        // a product too large for usize is larger than any link_len
        let below_catalog_average = 100usize
            .checked_mul(elem_num)
            .is_none_or(|limit| link_len < limit);
        if link_len as f64 > text_len as f64 * LINK_FARM_RATIO
            && below_catalog_average
            && !is_paragraph_listing(tree, &links, call)?
        {
            return Ok((true, false));
        }
    }
    Ok((false, false))
}

/// Whether the table `element` is rich in links (probably boilerplate); a table of fewer than 200
/// characters, or whose links carry no text, never is. Checks `call` about every `CHECK_INTERVAL`
/// nodes.
// upstream: htmlprocessing.py::link_density_test_tables
pub fn link_density_test_tables(
    tree: &Tree,
    element: NodeId,
    call: &Call<'_>,
) -> Result<bool, ExtractError> {
    let links = find_refs(tree, element, call)?;
    if links.is_empty() {
        return Ok(false);
    }
    // same count as len(trim(text_content)), without building the strings
    let text_len = trimmed_len(tree, element);
    if text_len < 200 {
        return Ok(false);
    }
    let (link_len, _, _, _) = collect_link_info(tree, &links, call)?;
    Ok(if text_len < 1000 {
        link_len as f64 > 0.8 * text_len as f64
    } else {
        link_len as f64 > 0.5 * text_len as f64
    })
}

/// Deletes the `tagname` elements of `subtree` (the root included) that are rich in links, with
/// their text but not their tail; with `backtracking`, also the short ones with links that have
/// enough children. Checks `call` about every `CHECK_INTERVAL` nodes.
// upstream: htmlprocessing.py::delete_by_link_density
pub fn delete_by_link_density(
    tree: &mut Tree,
    subtree: NodeId,
    tagname: &str,
    backtracking: bool,
    favor_precision: bool,
    call: &Call<'_>,
) -> Result<(), ExtractError> {
    let len_threshold = if favor_precision { 200 } else { 100 };
    let depth_threshold = if favor_precision { 1 } else { 3 };
    // the walk edits nothing: upstream collects the deletions and applies them afterwards
    let candidates = collect_tagged(tree, call, tree.iter(subtree, &[]), &[tagname])?;
    let mut deletions = Vec::new();
    for element in candidates {
        let (dense, short_with_links) = link_density_test(tree, element, favor_precision, call)?;
        if dense
            || (backtracking
                && short_with_links
                && tree.child_count(element) >= depth_threshold
                // same count as len(trim(text_content)), without building the strings
                && trimmed_len(tree, element) < len_threshold)
        {
            // a paragraph that holds the content of a list item is kept: the link density of the
            // whole list is checked separately, and removing it here would leave the item empty
            let parent = tree.node(element).parent();
            if tagname == "p"
                && parent
                    .and_then(|parent| tree.tag(parent))
                    .is_some_and(|name| matches!(name, "item" | "td" | "th"))
            {
                continue;
            }
            deletions.push(element);
        }
    }
    // `dict.fromkeys`: each element once
    let mut deleted = HashSet::new();
    let mut visited = 0usize;
    for element in deletions {
        tick(&mut visited, call)?;
        if deleted.insert(element) {
            delete_element(tree, element, true);
        }
    }
    Ok(())
}

/// Python's `trim(value) or None`: `None` stays `None`, an empty trim becomes `None`.
fn trim_or_none(value: Option<&str>) -> Option<String> {
    value.map(trim).filter(|trimmed| !trimmed.is_empty())
}

/// Python truthiness of an lxml text or tail: neither `None` nor `""`.
pub(crate) fn has_content(value: Option<&str>) -> bool {
    value.is_some_and(|text| !text.is_empty())
}

/// Converts, formats and probes a potential text element; `None` when it is not worth keeping.
// upstream: htmlprocessing.py::handle_textnode
pub fn handle_textnode(
    tree: &mut Tree,
    elem: NodeId,
    ctx: &Ctx<'_>,
    comments_fix: bool,
    preserve_spaces: bool,
) -> Option<NodeId> {
    let tag = tree.tag(elem);
    if tag == Some("graphic") && image_src(tree, elem).is_some() {
        return Some(elem);
    }
    let is_lb = tag == Some("lb");
    let node = tree.node(elem);
    if tag == Some("done")
        || (tree.child_count(elem) == 0
            && !has_content(node.text.as_deref())
            && !has_content(node.tail.as_deref())
            && (comments_fix || !separates_inline(tree, elem)))
    {
        return None;
    }

    // lb bypass
    if !comments_fix && is_lb {
        if !preserve_spaces {
            let tail = trim_or_none(tree.node(elem).tail.as_deref());
            tree.node_mut(elem).tail = tail;
        }
        // no textfilter/dedup for lb, unlike process_node: the two probes are not interchangeable
        return Some(elem);
    }

    if !has_content(tree.node(elem).text.as_deref()) && tree.child_count(elem) == 0 {
        // try the tail
        let node = tree.node_mut(elem);
        node.text = node.tail.take();
        node.tail = Some(String::new());
        // handle differently for br/lb
        if comments_fix && is_lb {
            tree.set_name(elem, "p");
        }
    }

    // trim
    if !preserve_spaces {
        let node = tree.node(elem);
        let text = trim_or_none(node.text.as_deref());
        let tail = if has_content(node.tail.as_deref()) {
            trim_or_none(node.tail.as_deref())
        } else {
            node.tail.clone()
        };
        let node = tree.node_mut(elem);
        node.text = text;
        node.tail = tail;
    }

    // filter content
    if (!has_content(tree.node(elem).text.as_deref()) && textfilter(tree, elem))
        || duplicate_test(tree, elem, ctx)
    {
        return None;
    }
    Some(elem)
}

/// Converts, formats and probes a potential text element (light format).
// upstream: htmlprocessing.py::process_node
pub fn process_node(tree: &mut Tree, elem: NodeId, ctx: &Ctx<'_>) -> Option<NodeId> {
    let node = tree.node(elem);
    if tree.tag(elem) == Some("done")
        || (tree.child_count(elem) == 0
            && !has_content(node.text.as_deref())
            && !has_content(node.tail.as_deref()))
    {
        return None;
    }

    // trim
    let node = tree.node(elem);
    let text = trim_or_none(node.text.as_deref());
    let tail = trim_or_none(node.tail.as_deref());
    let node = tree.node_mut(elem);
    node.text = text;
    node.tail = tail;

    // adapt content string
    if tree.tag(elem) != Some("lb")
        && !has_content(tree.node(elem).text.as_deref())
        && has_content(tree.node(elem).tail.as_deref())
        && tree.child_count(elem) == 0
    {
        let node = tree.node_mut(elem);
        node.text = node.tail.take();
    }

    // content checks
    let node = tree.node(elem);
    if (has_content(node.text.as_deref()) || has_content(node.tail.as_deref()))
        && (textfilter(tree, elem) || duplicate_test(tree, elem, ctx))
    {
        return None;
    }
    Some(elem)
}

mod convert;

#[cfg(test)]
use convert::CONVERSION_TAGS;
pub use convert::{build_html_output, convert_tags};
#[cfg(any(test, feature = "__internals"))]
pub use convert::{
    convert_deletions, convert_details, convert_headings, convert_line_breaks, convert_link,
    convert_lists, convert_quotes, convert_to_html,
};

#[cfg(test)]
mod cases;
#[cfg(test)]
mod tests;
