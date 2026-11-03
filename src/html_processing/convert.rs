//! The tag conversions of upstream `htmlprocessing.py`: lists, quotes, code, headings, links and the HTML output.

use std::borrow::Cow;

use crate::cancel::Call;
use crate::context::Ctx;
use crate::dom::{LiveIter, NodeId, Tree};
use crate::error::{ExtractError, Rejection};
use crate::pystr;
use crate::utils::{safe_base_url, safe_relative_url};
use crate::xml::delete_element;

use super::{
    CODE_INDICATORS, HTML_TAG_MAPPING, REND_TAG_MAPPING, collect_tagged, has_content, tick,
};

/// The keys of `CONVERSIONS`; `convert` holds the function of each.
// upstream: htmlprocessing.py::CONVERSIONS
pub(super) const CONVERSION_TAGS: &[&str] = &[
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
/// `CHECK_INTERVAL` matched elements; the live walk itself takes no `Call`.
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
/// every `CHECK_INTERVAL` matched elements; the live walk itself takes no `Call`.
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
/// the pipeline; `url` resolves relative links. Checks `ctx.call` before each walk and about every
/// `CHECK_INTERVAL` matched elements of a walk; the live walks themselves take no `Call`, so one
/// walk runs unchecked, bounded by `max_nodes`.
// upstream: htmlprocessing.py::convert_tags
pub fn convert_tags(
    tree: &mut Tree,
    root: NodeId,
    ctx: &Ctx<'_>,
    url: Option<&str>,
) -> Result<(), ExtractError> {
    let call = ctx.call;
    let options = &ctx.options;
    call.check()?;
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
    call.check()?;
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
    call.check()?;
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
    call.check()?;
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
    call.check()?;
    let mut walk = LiveIter::new(tree, root, CONVERSION_TAGS);
    let mut visited = 0usize;
    while let Some(elem) = walk.next(tree) {
        tick(&mut visited, call)?;
        convert(tree, elem, call)?;
    }

    // images
    if options.images {
        call.check()?;
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

/// The HTML tag `element` becomes, `None` for an element that stays as it is. Fails with
/// `InvalidValue` where upstream raises: a `hi` whose `rend` is not in `HTML_TAG_MAPPING`
/// (`KeyError`, an empty `rend` included: the `#i` default is for an absent attribute) and a `head`
/// level that Python `int` rejects (`ValueError`).
// upstream: htmlprocessing.py::HTML_CONVERSIONS
fn html_conversion(
    tree: &Tree,
    element: NodeId,
) -> Result<Option<Cow<'static, str>>, ExtractError> {
    let invalid = || ExtractError::Rejected(Rejection::InvalidValue);
    let Some(tag) = tree.tag(element) else {
        return Ok(None);
    };
    Ok(Some(Cow::Borrowed(match tag {
        "list" => "ul",
        "item" => "li",
        "code" => "pre",
        "quote" => "blockquote",
        "lb" => "br",
        "graphic" => "img",
        "ref" => "a",
        "row" => "tr",
        "cell" if tree.get(element, "role") == Some("head") => "th",
        "cell" => "td",
        "head" => {
            // `rend[1:]`: whatever follows the first character, `h` in practice
            let mut rend = tree.get(element, "rend").unwrap_or("h3").chars();
            rend.next();
            return Ok(Some(Cow::Owned(format!(
                "h{}",
                pystr::int_str(rend.as_str())?
            ))));
        }
        "hi" => {
            let rend = tree.get(element, "rend").unwrap_or("#i");
            HTML_TAG_MAPPING
                .iter()
                .find(|(key, _)| *key == rend)
                .map(|(_, html_tag)| *html_tag)
                .ok_or_else(invalid)?
        }
        _ => return Ok(None),
    })))
}

/// Converts the XML elements below `body` to HTML in place (`list` to `ul`, `hi` to its formatting
/// tag, `ref` to `a` with its `target` as `href`...), renames `body` itself to `body` whatever its
/// tag, and returns a new `html` element with it appended. A converted element loses its
/// attributes, except `a` (which gets `href`) and `img` (which keeps all); an element that is not
/// converted keeps its own. Checks `call` about every `CHECK_INTERVAL` elements.
// upstream: htmlprocessing.py::convert_to_html
pub fn convert_to_html(
    tree: &mut Tree,
    body: NodeId,
    call: &Call<'_>,
) -> Result<NodeId, ExtractError> {
    let mut walk = LiveIter::new(tree, body, &[]);
    let mut visited = 0usize;
    while let Some(element) = walk.next(tree) {
        tick(&mut visited, call)?;
        let Some(name) = html_conversion(tree, element)? else {
            continue;
        };
        tree.set_name(element, &name);
        match &*name {
            "a" => {
                let target = tree.remove_attr(element, "target");
                tree.set(element, "href", target.as_deref().unwrap_or(""));
            }
            "img" => {}
            _ => tree.clear_attrs(element),
        }
    }
    tree.set_name(body, "body");
    let html = tree.new_element("html");
    tree.append(html, body);
    Ok(html)
}

/// Converts the tree below `body` to HTML and returns it indented, without the line break after
/// the root. With `meta`, a `head` first in `html` holds one `meta` element per `(name, content)`
/// pair, and is empty for an empty slice. Fails with `InvalidValue` where [`convert_to_html`] does;
/// checks `call` before it serializes.
// upstream: htmlprocessing.py::build_html_output
pub fn build_html_output(
    tree: &mut Tree,
    body: NodeId,
    meta: Option<&[(&str, &str)]>,
    call: &Call<'_>,
) -> Result<String, ExtractError> {
    let html = convert_to_html(tree, body, call)?;
    if let Some(items) = meta {
        let head = tree.new_element("head");
        for (name, content) in items {
            let item = tree.new_element("meta");
            tree.set(item, "name", name);
            tree.set(item, "content", content);
            tree.append(head, item);
        }
        tree.insert(html, 0, head);
    }
    call.check()?;
    Ok(pystr::strip(&tree.tostring(html, true, false)).to_owned())
}
