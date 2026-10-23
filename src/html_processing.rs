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
use crate::utils::{
    LINK_FARM_RATIO, image_src, safe_base_url, safe_relative_url, textfilter, trim,
};
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
    let total = tree.text_content(root).chars().count();
    for (index, form) in forms.into_iter().enumerate() {
        if index.is_multiple_of(CHECK_INTERVAL) {
            call.check()?;
        }
        if total != 0 && tree.text_content(form).chars().count() as f64 > total as f64 / 2.0 {
            tree.set_name(form, "div");
        } else {
            delete_element(tree, form, true);
        }
    }
    Ok(())
}

/// Prunes the tree below `root` by discarding the unwanted elements of `MANUALLY_CLEANED` and
/// `MANUALLY_STRIPPED`; returns the root to continue with, which is `root` except when recall
/// restores its copy. Checks `ctx.call` about every `CHECK_INTERVAL` nodes.
// upstream: htmlprocessing.py::tree_cleaning
pub fn tree_cleaning(tree: &mut Tree, root: NodeId, ctx: &Ctx<'_>) -> Result<NodeId, ExtractError> {
    tree_cleaning_with(tree, root, ctx, MANUALLY_CLEANED, MANUALLY_STRIPPED)
}

/// `tree_cleaning` over the given lists in place of `MANUALLY_CLEANED` and `MANUALLY_STRIPPED`,
/// which users mutate upstream. With `images` on, `stripped` is expected to hold `img`; without it
/// nothing is removed from the list (upstream raises `ValueError`, and only an option of phase 3f
/// can lead here).
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
        remove_first(&mut stripping_list, "img");
    }

    // strip targeted elements
    tree.strip_tags(root, &stripping_list);

    // recall: undo the deletions if they remove every paragraph (copy only if none can survive)
    let needs_backup = ctx.options.focus == Focus::Recall
        && tree.find(root, "p").is_some()
        && (cleaning_list.contains(&"p")
            || !any_paragraph_survives(tree, root, &cleaning_list, call)?);
    let backup = needs_backup.then(|| tree.copy(root));
    for expression in &cleaning_list {
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
fn tick(visited: &mut usize, call: &Call<'_>) -> Result<(), ExtractError> {
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
    let old_len = tree.text_content(root).chars().count();
    let removed = outer.iter().fold(0usize, |sum, &element| {
        sum.saturating_add(tree.text_content(element).chars().count())
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
    let text_len = trim(&tree.text_content(element)).chars().count();
    // shortcut
    if let [only] = links[..] {
        let len_threshold = if favor_precision { 10 } else { 100 };
        let link_len = trim(&tree.text_content(only)).chars().count();
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
    let text_len = trim(&tree.text_content(element)).chars().count();
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
                && trim(&tree.text_content(element)).chars().count() < len_threshold)
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
fn has_content(value: Option<&str>) -> bool {
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
        || (ctx.options.dedup && duplicate_test(tree, elem, ctx))
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
        && (textfilter(tree, elem) || (ctx.options.dedup && duplicate_test(tree, elem, ctx)))
    {
        return None;
    }
    Some(elem)
}

/// The keys of `CONVERSIONS`; `convert` holds the function of each.
// upstream: htmlprocessing.py::CONVERSIONS
const CONVERSION_TAGS: &[&str] = &[
    "dl",
    "ol",
    "ul",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "br",
    "hr",
    "blockquote",
    "pre",
    "q",
    "del",
    "s",
    "strike",
    "details",
];

/// Converts `ul`, `ol` and `dl` to `list` and the `li`, `dd` and `dt` below to `item`; `dd` and
/// `dt` keep their kind and the number of their `dt`/`dd` pair in `rend`. Checks `call` about every
/// `CHECK_INTERVAL` nodes.
// upstream: htmlprocessing.py::convert_lists
pub fn convert_lists(tree: &mut Tree, elem: NodeId, call: &Call<'_>) -> Result<(), ExtractError> {
    let kind = tree.node(elem).name.to_string();
    tree.set(elem, "rend", &kind);
    tree.set_name(elem, "list");
    let mut counter = 1usize;
    // a live walk: each item is renamed as the walk passes it
    let tags = ["dd", "dt", "li"];
    let mut walk = LiveIter::new(tree, elem, &tags);
    let mut visited = 0usize;
    while let Some(subelem) = walk.next(tree) {
        tick(&mut visited, call)?;
        // keep track of dd/dt items
        let tag = tree.tag(subelem).unwrap_or_default().to_string();
        if tag == "dd" || tag == "dt" {
            tree.set(subelem, "rend", &format!("{tag}-{counter}"));
            // increment counter after <dd> in description list
            if tag == "dd" {
                counter = counter.saturating_add(1);
            }
        }
        // convert elem tag (needs to happen after the rest)
        tree.set_name(subelem, "item");
    }
    Ok(())
}

/// Converts `blockquote`, `pre` and `q` to `quote`, or to `code` for a `pre` that wraps a single
/// `span`, carries highlight.js spans (their attributes are cleared) or has code-like text. Checks
/// `call` about every `CHECK_INTERVAL` nodes.
// upstream: htmlprocessing.py::convert_quotes
pub fn convert_quotes(tree: &mut Tree, elem: NodeId, call: &Call<'_>) -> Result<(), ExtractError> {
    let is_pre = tree.tag(elem) == Some("pre");
    let mut hljs = Vec::new();
    if is_pre {
        let spans = collect_tagged(tree, call, tree.iterdescendants(elem, &[]), &["span"])?;
        hljs = spans
            .into_iter()
            .filter(|&span| {
                tree.get(span, "class")
                    .is_some_and(|class| class.starts_with("hljs"))
            })
            .collect();
    }
    for &subelem in &hljs {
        tree.clear_attrs(subelem);
    }
    // a pre is code if it wraps a single span, carries highlight.js spans or code-like text
    let is_code = is_pre
        && ((tree.child_count(elem) == 1
            && tree
                .first_child(elem)
                .is_some_and(|child| tree.tag(child) == Some("span")))
            || !hljs.is_empty()
            || {
                let text = tree.node(elem).text.as_deref().unwrap_or("");
                CODE_INDICATORS
                    .iter()
                    .any(|indicator| text.contains(indicator))
            });
    tree.set_name(elem, if is_code { "code" } else { "quote" });
    Ok(())
}

/// Turns a heading into `head`, with its attributes dropped and its old tag in `rend`.
// upstream: htmlprocessing.py::convert_headings
pub fn convert_headings(tree: &mut Tree, elem: NodeId) {
    let kind = tree.node(elem).name.to_string();
    tree.clear_attrs(elem);
    tree.set(elem, "rend", &kind);
    tree.set_name(elem, "head");
}

/// Turns `br` and `hr` into `lb`.
// upstream: htmlprocessing.py::convert_line_breaks
pub fn convert_line_breaks(tree: &mut Tree, elem: NodeId) {
    tree.set_name(elem, "lb");
}

/// Turns `del`, `s` and `strike` into `del` with `rend="overstrike"`.
// upstream: htmlprocessing.py::convert_deletions
pub fn convert_deletions(tree: &mut Tree, elem: NodeId) {
    tree.set_name(elem, "del");
    tree.set(elem, "rend", "overstrike");
}

/// Turns `details` into `div` and the `summary` elements below it into `head`. Checks `call` about
/// every `CHECK_INTERVAL` nodes.
// upstream: htmlprocessing.py::convert_details
pub fn convert_details(tree: &mut Tree, elem: NodeId, call: &Call<'_>) -> Result<(), ExtractError> {
    tree.set_name(elem, "div");
    let tags = ["summary"];
    let mut walk = LiveIter::new(tree, elem, &tags);
    let mut visited = 0usize;
    while let Some(subelem) = walk.next(tree) {
        tick(&mut visited, call)?;
        tree.set_name(subelem, "head");
    }
    Ok(())
}

/// The conversion `CONVERSIONS` holds for the tag of `elem`; one arm per entry of `CONVERSION_TAGS`.
// upstream: htmlprocessing.py::CONVERSIONS
fn convert(tree: &mut Tree, elem: NodeId, call: &Call<'_>) -> Result<(), ExtractError> {
    match tree.tag(elem) {
        Some("dl" | "ol" | "ul") => convert_lists(tree, elem, call),
        Some("h1" | "h2" | "h3" | "h4" | "h5" | "h6") => {
            convert_headings(tree, elem);
            Ok(())
        }
        Some("br" | "hr") => {
            convert_line_breaks(tree, elem);
            Ok(())
        }
        Some("blockquote" | "pre" | "q") => convert_quotes(tree, elem, call),
        Some("del" | "s" | "strike") => {
            convert_deletions(tree, elem);
            Ok(())
        }
        Some("details") => convert_details(tree, elem, call),
        _ => Ok(()),
    }
}

/// Turns a link into `ref` with only a `target` attribute, resolved against `base_url` (empty for
/// none; a target that cannot be resolved stays as it is).
// upstream: htmlprocessing.py::convert_link
pub fn convert_link(tree: &mut Tree, elem: NodeId, base_url: &str) {
    tree.set_name(elem, "ref");
    let target = tree.get(elem, "href").map(str::to_string);
    tree.clear_attrs(elem);
    let Some(mut target) = target.filter(|target| !target.is_empty()) else {
        return;
    };
    // convert relative URLs
    if !base_url.is_empty() {
        let resolved = safe_relative_url(base_url, &target);
        if !resolved.is_empty() {
            target = resolved;
        }
    }
    tree.set(elem, "target", &target);
}

/// Simplifies the markup below `root` and converts the relevant HTML tags to the XML standard of
/// the pipeline; `url` resolves relative links. Checks `ctx.call` about every `CHECK_INTERVAL`
/// nodes.
// upstream: htmlprocessing.py::convert_tags
pub fn convert_tags(
    tree: &mut Tree,
    root: NodeId,
    ctx: &Ctx<'_>,
    url: Option<&str>,
) -> Result<(), ExtractError> {
    let call = ctx.call;
    let options = &ctx.options;
    // upstream edits inside `tree.iter(...)` in each loop below, hence the live walks
    if !options.links {
        // delete links for faster processing
        let containers: &[&str] = if options.tables {
            &["div", "li", "p", "table"]
        } else {
            &["div", "li", "p"]
        };
        // necessary for further detection, the tree root does not count as a container
        let tags = ["a"];
        let mut walk = LiveIter::new(tree, root, &tags);
        let mut visited = 0usize;
        while let Some(elem) = walk.next(tree) {
            tick(&mut visited, call)?;
            if tree
                .iterancestors(elem, containers)
                .next()
                .is_some_and(|container| container != root)
            {
                tree.set_name(elem, "ref");
            }
        }
        // strip the rest
        tree.strip_tags(root, &["a"]);
    } else {
        // get base URL for converting relative URLs
        let base_url = url.map(safe_base_url).unwrap_or_default();
        let tags = ["a", "ref"];
        let mut walk = LiveIter::new(tree, root, &tags);
        let mut visited = 0usize;
        while let Some(elem) = walk.next(tree) {
            tick(&mut visited, call)?;
            convert_link(tree, elem, &base_url);
        }
    }

    // Yoast FAQ blocks: question headers are bold but act as titles (#471)
    let tags = ["strong"];
    let mut walk = LiveIter::new(tree, root, &tags);
    let mut visited = 0usize;
    while let Some(elem) = walk.next(tree) {
        tick(&mut visited, call)?;
        if tree
            .get(elem, "class")
            .unwrap_or("")
            .contains("schema-faq-question")
        {
            tree.clear_attrs(elem);
            tree.set(elem, "rend", "h3");
            tree.set_name(elem, "head");
        }
    }

    // an empty sup/sub carries nothing to raise or lower, and process_node() would later
    // hand it the following text as its own, so the marker would wrap the wrong words.
    // The tail is kept in every focus mode, unlike the pruning of CUT_EMPTY_ELEMS.
    let tags = ["sub", "sup"];
    let mut walk = LiveIter::new(tree, root, &tags);
    let mut visited = 0usize;
    while let Some(elem) = walk.next(tree) {
        tick(&mut visited, call)?;
        if !has_content(tree.node(elem).text.as_deref()) && tree.child_count(elem) == 0 {
            delete_element(tree, elem, true);
        }
    }

    let rend_tags: Vec<&str> = REND_TAG_MAPPING.iter().map(|&(tag, _)| tag).collect();
    if options.formatting {
        let mut walk = LiveIter::new(tree, root, &rend_tags);
        let mut visited = 0usize;
        while let Some(elem) = walk.next(tree) {
            tick(&mut visited, call)?;
            let rend = REND_TAG_MAPPING
                .iter()
                .find(|&&(tag, _)| tree.tag(elem) == Some(tag))
                .map_or("", |&(_, rend)| rend);
            tree.clear_attrs(elem);
            tree.set(elem, "rend", rend);
            tree.set_name(elem, "hi");
        }
    } else {
        tree.strip_tags(root, &rend_tags);
    }

    // iterate over all concerned elements
    let mut walk = LiveIter::new(tree, root, CONVERSION_TAGS);
    let mut visited = 0usize;
    while let Some(elem) = walk.next(tree) {
        tick(&mut visited, call)?;
        convert(tree, elem, call)?;
    }

    // images
    if options.images {
        let tags = ["img"];
        let mut walk = LiveIter::new(tree, root, &tags);
        let mut visited = 0usize;
        while let Some(elem) = walk.next(tree) {
            tick(&mut visited, call)?;
            tree.set_name(elem, "graphic");
        }
        // move images out of links to prevent duplication/loss
        if options.links {
            // `list(tree.iter("ref"))`: collected before the moves
            let refs = collect_tagged(tree, call, tree.iter(root, &[]), &["ref"])?;
            let mut visited = 0usize;
            for link in refs {
                tick(&mut visited, call)?;
                let graphics = collect_tagged(tree, call, tree.iter(link, &[]), &["graphic"])?;
                // iterate in reverse so add_next preserves original order
                for &graphic in graphics.iter().rev() {
                    tree.add_next(link, graphic);
                }
                // remove ref if it only contained images (no text),
                // its leftover children (picture, source) would
                // otherwise trigger justext fallback and lose images
                if !graphics.is_empty() && pystr::strip(&tree.text_content(link)).is_empty() {
                    delete_element(tree, link, true);
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
