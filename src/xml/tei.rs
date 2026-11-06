//! The XML-TEI output of upstream `xml.py`: the TEI tree written from `doc` (`write_teitree`, with the
//! header of `write_fullheader`), and the conformity pass `check_tei` that renames headings, merges
//! unknown tags, moves tails and wraps the blocks of a `div`. The walks follow the tree's links and
//! check `Call` about every `CHECK_INTERVAL` nodes; none recurses, so no stack grows with the depth.
//! The TEI DTD check is not ported (D-27).

use std::collections::HashSet;

use super::control::clean_attributes;
use super::merge_with_parent;
use crate::cancel::Call;
use crate::dom::{LiveIter, LocalName, NodeId, Tree};
use crate::error::ExtractError;
use crate::html_processing::tick;
use crate::pystr::strip;
use crate::settings::Document;

/// The upstream version, from `Cargo.toml` through `build.rs`.
// upstream: xml.py::PKG_VERSION
pub const PKG_VERSION: &str = env!("TRAFILATURA_UPSTREAM_VERSION");

/// The elements a TEI body may hold; `check_tei` merges any other element into its parent.
// upstream: xml.py::TEI_VALID_TAGS
const TEI_VALID_TAGS: [&str; 17] = [
    "ab", "body", "cell", "code", "del", "div", "graphic", "head", "hi", "item", "lb", "list", "p",
    "quote", "ref", "row", "table",
];

/// The attributes a TEI element may keep; `check_tei` removes the others.
// upstream: xml.py::TEI_VALID_ATTRS
const TEI_VALID_ATTRS: [&str; 5] = ["rend", "rendition", "role", "target", "type"];

/// The elements whose tail `check_tei` moves into the body.
// upstream: xml.py::TEI_REMOVE_TAIL
const TEI_REMOVE_TAIL: [&str; 2] = ["ab", "p"];

/// The block elements that `_wrap_unwanted_siblings_of_div` gathers into a new `div`.
// upstream: xml.py::TEI_DIV_SIBLINGS
const TEI_DIV_SIBLINGS: [&str; 5] = ["p", "list", "table", "quote", "ab"];

/// The metadata the TEI header reads. The fields are borrowed one by one, so `write_teitree` writes
/// the header into the document's tree while it reads the metadata beside it.
#[derive(Clone, Copy, Debug, Default)]
pub struct HeaderFields<'a> {
    /// Page title.
    pub title: Option<&'a str>,
    /// Author.
    pub author: Option<&'a str>,
    /// URL of the page.
    pub url: Option<&'a str>,
    /// Host name of the URL.
    pub hostname: Option<&'a str>,
    /// Description.
    pub description: Option<&'a str>,
    /// Name of the site.
    pub sitename: Option<&'a str>,
    /// Publication date.
    pub date: Option<&'a str>,
    /// Categories.
    pub categories: Option<&'a [String]>,
    /// Tags.
    pub tags: Option<&'a [String]>,
    /// Content fingerprint.
    pub fingerprint: Option<&'a str>,
    /// Record id given by the caller.
    pub id: Option<&'a str>,
    /// License.
    pub license: Option<&'a str>,
    /// Date of the file.
    pub filedate: Option<&'a str>,
}

/// The TEI document of `doc`: `write_teitree`, then `check_tei` with the URL of `doc`.
// upstream: xml.py::build_tei_output
pub fn build_tei_output(doc: &mut Document, call: &Call<'_>) -> Result<NodeId, ExtractError> {
    let teidoc = write_teitree(doc, call)?;
    check_tei(&mut doc.tree, teidoc, doc.url.as_deref(), call)
}

/// The TEI root of `doc`: the header, then a `text` holding the body (`entry`) and the comments
/// (`comments`) as `div`s. Both bodies are moved from the document's tree into the new one.
///
/// Checks `call` through `clean_attributes`.
// upstream: xml.py::write_teitree
pub fn write_teitree(doc: &mut Document, call: &Call<'_>) -> Result<NodeId, ExtractError> {
    let teidoc = doc.tree.new_element("TEI");
    doc.tree.set(teidoc, "xmlns", "http://www.tei-c.org/ns/1.0");
    let fields = HeaderFields {
        title: doc.title.as_deref(),
        author: doc.author.as_deref(),
        url: doc.url.as_deref(),
        hostname: doc.hostname.as_deref(),
        description: doc.description.as_deref(),
        sitename: doc.sitename.as_deref(),
        date: doc.date.as_deref(),
        categories: doc.categories.as_deref(),
        tags: doc.tags.as_deref(),
        fingerprint: doc.fingerprint.as_deref(),
        id: doc.id.as_deref(),
        license: doc.license.as_deref(),
        filedate: doc.filedate.as_deref(),
    };
    write_fullheader(&mut doc.tree, teidoc, &fields);
    let text_elem = sub_element(&mut doc.tree, teidoc, "text", &[], None);
    let text_body = sub_element(&mut doc.tree, text_elem, "body", &[], None);
    for (body, kind) in [(doc.body, "entry"), (doc.comments_body, "comments")] {
        clean_attributes(&mut doc.tree, body, call)?;
        doc.tree.set_name(body, "div");
        doc.tree.set(body, "type", kind);
        doc.tree.append(text_body, body);
    }
    Ok(teidoc)
}

/// The `teiHeader` of `fields`, appended to `teidoc`. A `None` field leaves its element without
/// text.
// upstream: xml.py::write_fullheader
pub fn write_fullheader(tree: &mut Tree, teidoc: NodeId, fields: &HeaderFields<'_>) -> NodeId {
    let header = sub_element(tree, teidoc, "teiHeader", &[], None);
    let filedesc = sub_element(tree, header, "fileDesc", &[], None);
    let titlestmt = sub_element(tree, filedesc, "titleStmt", &[], None);
    sub_element(tree, titlestmt, "title", &[("type", "main")], fields.title);
    if let Some(author) = truthy(fields.author) {
        sub_element(tree, titlestmt, "author", &[], Some(author));
    }

    let publicationstmt_a = sub_element(tree, filedesc, "publicationStmt", &[], None);
    let publisher_string = define_publisher_string(fields);
    // license, if applicable
    if let Some(license) = truthy(fields.license) {
        sub_element(
            tree,
            publicationstmt_a,
            "publisher",
            &[],
            Some(publisher_string.as_str()),
        );
        let availability = sub_element(tree, publicationstmt_a, "availability", &[], None);
        sub_element(tree, availability, "p", &[], Some(license));
    } else {
        // an empty paragraph for conformity
        sub_element(tree, publicationstmt_a, "p", &[], None);
    }

    let notesstmt = sub_element(tree, filedesc, "notesStmt", &[], None);
    if let Some(id) = truthy(fields.id) {
        sub_element(tree, notesstmt, "note", &[("type", "id")], Some(id));
    }
    sub_element(
        tree,
        notesstmt,
        "note",
        &[("type", "fingerprint")],
        fields.fingerprint,
    );

    let sourcedesc = sub_element(tree, filedesc, "sourceDesc", &[], None);
    let source_bibl = sub_element(tree, sourcedesc, "bibl", &[], None);
    let sigle = join_truthy(&[fields.sitename, fields.date], ", ");
    let bibl_text = join_truthy(&[fields.title, Some(sigle.as_str())], ", ");
    tree.node_mut(source_bibl).text = Some(bibl_text);
    sub_element(
        tree,
        sourcedesc,
        "bibl",
        &[("type", "sigle")],
        Some(sigle.as_str()),
    );

    let biblfull = sub_element(tree, sourcedesc, "biblFull", &[], None);
    let bib_titlestmt = sub_element(tree, biblfull, "titleStmt", &[], None);
    sub_element(
        tree,
        bib_titlestmt,
        "title",
        &[("type", "main")],
        fields.title,
    );
    if let Some(author) = truthy(fields.author) {
        sub_element(tree, bib_titlestmt, "author", &[], Some(author));
    }
    let publicationstmt = sub_element(tree, biblfull, "publicationStmt", &[], None);
    sub_element(
        tree,
        publicationstmt,
        "publisher",
        &[],
        Some(publisher_string.as_str()),
    );
    if let Some(url) = truthy(fields.url) {
        sub_element(
            tree,
            publicationstmt,
            "ptr",
            &[("type", "URL"), ("target", url)],
            None,
        );
    }
    sub_element(tree, publicationstmt, "date", &[], fields.date);

    let profiledesc = sub_element(tree, header, "profileDesc", &[], None);
    let abstract_elem = sub_element(tree, profiledesc, "abstract", &[], None);
    sub_element(tree, abstract_elem, "p", &[], fields.description);

    let categories = fields.categories.filter(|list| !list.is_empty());
    let tags = fields.tags.filter(|list| !list.is_empty());
    if categories.is_some() || tags.is_some() {
        let textclass = sub_element(tree, profiledesc, "textClass", &[], None);
        let keywords = sub_element(tree, textclass, "keywords", &[], None);
        if let Some(categories) = categories {
            let joined = categories.join(",");
            sub_element(
                tree,
                keywords,
                "term",
                &[("type", "categories")],
                Some(joined.as_str()),
            );
        }
        if let Some(tags) = tags {
            let joined = tags.join(",");
            sub_element(
                tree,
                keywords,
                "term",
                &[("type", "tags")],
                Some(joined.as_str()),
            );
        }
    }

    let creation = sub_element(tree, profiledesc, "creation", &[], None);
    sub_element(
        tree,
        creation,
        "date",
        &[("type", "download")],
        fields.filedate,
    );

    let encodingdesc = sub_element(tree, header, "encodingDesc", &[], None);
    let appinfo = sub_element(tree, encodingdesc, "appInfo", &[], None);
    let application = sub_element(
        tree,
        appinfo,
        "application",
        &[("version", PKG_VERSION), ("ident", "Trafilatura")],
        None,
    );
    sub_element(tree, application, "label", &[], Some("Trafilatura"));
    sub_element(
        tree,
        application,
        "ptr",
        &[("target", "https://github.com/adbar/trafilatura")],
        None,
    );
    header
}

/// The publisher of the TEI header: `sitename (hostname)` when both are set, else whichever is set,
/// else `N/A`.
// upstream: xml.py::_define_publisher_string
fn define_publisher_string(fields: &HeaderFields<'_>) -> String {
    match (truthy(fields.hostname), truthy(fields.sitename)) {
        (Some(hostname), Some(sitename)) => format!("{} ({hostname})", strip(sitename)),
        (hostname, sitename) => hostname.or(sitename).unwrap_or("N/A").to_owned(),
    }
}

/// Makes `xmldoc` conform to TEI and returns it: converts each `head` to `ab` (or to an `ab` with
/// `lb` line breaks when it has children), moves the `p` a `head` holds out of its `p`, turns the
/// `lb` of a `div` into a `p` when it has a tail, merges each element that is not a TEI element into
/// its parent, moves tails of `ab` and `p` into the body, wraps the blocks of a `div`, and removes
/// the attributes TEI does not allow. `_url` is upstream's URL for its log lines, which the crate
/// does not write.
///
/// Checks `call` about every `CHECK_INTERVAL` nodes.
// upstream: xml.py::check_tei
pub fn check_tei(
    tree: &mut Tree,
    xmldoc: NodeId,
    _url: Option<&str>,
    call: &Call<'_>,
) -> Result<NodeId, ExtractError> {
    let mut visited = 0;
    // a live walk: a head moved by one conversion is still visited, as lxml's `iter` does
    let mut heads = LiveIter::new(tree, xmldoc, &["head"]);
    while let Some(head) = heads.next(tree) {
        tick(&mut visited, call)?;
        convert_head(tree, head, call)?;
    }

    let bodies = text_bodies(tree, xmldoc, call)?;
    // convert <lb/> when child of <div> to <p>
    for lb in div_line_breaks(tree, &bodies, call)? {
        tick(&mut visited, call)?;
        if truthy_tail(tree, lb).is_some_and(|tail| !strip(tail).is_empty()) {
            let node = tree.node_mut(lb);
            node.text = node.tail.take();
            tree.set_name(lb, "p");
        }
    }

    // a snapshot, as upstream's `findall`: an element merged away is still handled, inside its
    // detached subtree, which changes no output
    for elem in union_below(tree, &bodies, &[], call)? {
        tick(&mut visited, call)?;
        let kind = tree.tag(elem).map(|tag| {
            (
                TEI_VALID_TAGS.contains(&tag),
                TEI_REMOVE_TAIL.contains(&tag),
                tag == "div",
            )
        });
        let Some((true, remove_tail, is_div)) = kind else {
            merge_with_parent(tree, elem, false, call)?;
            continue;
        };
        if remove_tail {
            handle_unwanted_tails(tree, elem);
        } else if is_div {
            handle_text_content_of_div_nodes(tree, elem);
            wrap_unwanted_siblings_of_div(tree, elem, call)?;
        }
        drop_invalid_attributes(tree, elem);
    }
    Ok(xmldoc)
}

/// The `body` elements that `.//text/body` selects below `root`: a `body` whose parent is a `text`
/// element under `root`, in document order.
///
/// Checks `call` about every `CHECK_INTERVAL` nodes.
pub(super) fn text_bodies(
    tree: &Tree,
    root: NodeId,
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    let mut visited = 0;
    let mut bodies = Vec::new();
    for node in tree.iterdescendants(root, &[]) {
        tick(&mut visited, call)?;
        let selected = tree.tag(node) == Some("body")
            && tree
                .node(node)
                .parent()
                .is_some_and(|text| tree.tag(text) == Some("text") && tree.is_ancestor(root, text));
        if selected {
            bodies.push(node);
        }
    }
    Ok(bodies)
}

/// The `lb` elements that `body//div/lb` selects under `bodies`: an `lb` whose parent is a `div`.
///
/// Checks `call` about every `CHECK_INTERVAL` nodes.
pub(super) fn div_line_breaks(
    tree: &Tree,
    bodies: &[NodeId],
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    let mut visited = 0;
    let mut found = Vec::new();
    for lb in union_below(tree, bodies, &["lb"], call)? {
        tick(&mut visited, call)?;
        if tree
            .node(lb)
            .parent()
            .is_some_and(|parent| tree.tag(parent) == Some("div"))
        {
            found.push(lb);
        }
    }
    Ok(found)
}

/// The elements named in `tags` (every element when empty) below each of `bodies`, in document
/// order and without repeats: nested bodies would reach an element twice.
///
/// Checks `call` about every `CHECK_INTERVAL` nodes.
pub(super) fn union_below(
    tree: &Tree,
    bodies: &[NodeId],
    tags: &[&str],
    call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    let mut visited = 0;
    let mut found = Vec::new();
    for &body in bodies {
        for node in tree.iterdescendants(body, &[]) {
            tick(&mut visited, call)?;
            if tags.is_empty() || tree.tag(node).is_some_and(|tag| tags.contains(&tag)) {
                found.push(node);
            }
        }
    }
    if bodies.len() < 2 {
        return Ok(found);
    }
    // deviation: D-28
    let mut seen = HashSet::with_capacity(found.len());
    Ok(found.into_iter().filter(|&id| seen.insert(id)).collect())
}

/// Converts one `head` for `check_tei`: its tag becomes `ab` with `type="header"`. A head with
/// children is replaced by a converted copy; a head under a `p` then moves one level up.
// upstream: xml.py::check_tei (the `head` loop)
fn convert_head(tree: &mut Tree, head: NodeId, call: &Call<'_>) -> Result<(), ExtractError> {
    tree.set_name(head, "ab");
    tree.set(head, "type", "header");
    let Some(parent) = tree.node(head).parent() else {
        return Ok(());
    };
    let mut elem = head;
    if tree.first_child(head).is_some() {
        let new_elem = tei_handle_complex_head(tree, head, call)?;
        tree.add_previous(head, new_elem);
        tree.remove(head);
        elem = new_elem;
    }
    if tree.tag(parent) == Some("p") {
        move_element_one_level_up(tree, elem, call)?;
    }
    Ok(())
}

/// The text of `id`'s tail, or `None` when the tail is absent or empty.
fn truthy_tail(tree: &Tree, id: NodeId) -> Option<&str> {
    truthy(tree.node(id).tail.as_deref())
}

/// Removes the attributes of `element` that TEI does not allow.
// upstream: xml.py::check_tei (the attribute loop)
fn drop_invalid_attributes(tree: &mut Tree, element: NodeId) {
    let invalid: Vec<LocalName> = tree
        .node(element)
        .attrs
        .iter()
        .map(|(name, _)| name.clone())
        .filter(|name| !TEI_VALID_ATTRS.contains(&&**name))
        .collect();
    for name in invalid {
        tree.remove_attr(element, &name);
    }
}

/// Wraps the text of a `div` in a `p`: loose text before the first child joins a leading `p` or
/// becomes a new one, and the same for the tail after the last child.
// upstream: xml.py::_handle_text_content_of_div_nodes
fn handle_text_content_of_div_nodes(tree: &mut Tree, element: NodeId) {
    if let Some(text) = truthy(tree.node(element).text.as_deref())
        .filter(|text| !strip(text).is_empty())
        .map(str::to_owned)
    {
        match tree.first_child(element) {
            Some(first) if tree.tag(first) == Some("p") => {
                let joined = format!(
                    "{text} {}",
                    tree.node(first).text.as_deref().unwrap_or_default()
                );
                tree.node_mut(first).text = Some(strip(&joined).to_owned());
            }
            _ => {
                let new_child = tree.new_element("p");
                tree.node_mut(new_child).text = Some(text);
                tree.insert(element, 0, new_child);
            }
        }
        tree.node_mut(element).text = None;
    }

    if let Some(tail) = truthy(tree.node(element).tail.as_deref())
        .filter(|tail| !strip(tail).is_empty())
        .map(str::to_owned)
    {
        match tree.last_child(element) {
            Some(last) if tree.tag(last) == Some("p") => {
                let joined = format!(
                    "{} {tail}",
                    tree.node(last).text.as_deref().unwrap_or_default()
                );
                tree.node_mut(last).text = Some(strip(&joined).to_owned());
            }
            _ => {
                let new_child = tree.new_element("p");
                tree.node_mut(new_child).text = Some(tail);
                tree.append(element, new_child);
            }
        }
        tree.node_mut(element).tail = None;
    }
}

/// The `ab` that replaces a `head` with children: its stripped text, each `p` child turned into an
/// `lb` with the `p`'s text as the tail (or into the text when nothing precedes it), and the other
/// children moved in. The head's stripped tail becomes the new element's tail.
///
/// Checks `call` about every `CHECK_INTERVAL` children.
// upstream: xml.py::_tei_handle_complex_head
fn tei_handle_complex_head(
    tree: &mut Tree,
    element: NodeId,
    call: &Call<'_>,
) -> Result<NodeId, ExtractError> {
    let new_element = tree.new_element("ab");
    let attrs: Vec<(LocalName, _)> = tree.node(element).attrs.clone();
    for (name, value) in &attrs {
        tree.set(new_element, name, value);
    }
    tree.node_mut(new_element).text = tree
        .node(element)
        .text
        .as_deref()
        .map(|text| strip(text).to_owned());

    let children: Vec<NodeId> = tree.children(element).collect();
    let mut visited = 0;
    for child in children {
        tick(&mut visited, call)?;
        if tree.tag(child) != Some("p") {
            tree.append(new_element, child);
            continue;
        }
        let child_text = tree.node(child).text.clone();
        let has_content = tree.child_count(new_element) > 0
            || truthy(tree.node(new_element).text.as_deref()).is_some();
        if !has_content {
            tree.node_mut(new_element).text = child_text;
            continue;
        }
        // add <lb> if the element has no children or the last tail contains text
        let needs_lb = match tree.last_child(new_element) {
            None => true,
            Some(last) => truthy_tail(tree, last).is_some(),
        };
        if needs_lb {
            let lb = tree.new_element("lb");
            tree.append(new_element, lb);
        }
        if let Some(last) = tree.last_child(new_element) {
            tree.node_mut(last).tail = child_text;
        }
    }

    let tail = tree
        .node(element)
        .tail
        .as_deref()
        .map(|tail| strip(tail).to_owned())
        .filter(|tail| !tail.is_empty());
    if let Some(tail) = tail {
        tree.node_mut(new_element).tail = Some(tail);
    }
    Ok(new_element)
}

/// Wraps each run of block siblings after a `div` in a new `div`. A run ends at another `div` or at
/// an element that is not a block, so the order of the elements is kept.
///
/// Checks `call` about every `CHECK_INTERVAL` siblings.
// upstream: xml.py::_wrap_unwanted_siblings_of_div
pub fn wrap_unwanted_siblings_of_div(
    tree: &mut Tree,
    div_element: NodeId,
    call: &Call<'_>,
) -> Result<(), ExtractError> {
    let mut runs: Vec<Vec<NodeId>> = vec![Vec::new()];
    let mut visited = 0;
    let mut sibling = tree.node(div_element).next_sibling();
    while let Some(current) = sibling {
        tick(&mut visited, call)?;
        sibling = tree.node(current).next_sibling();
        match tree.tag(current) {
            Some("div") => break,
            Some(tag) if TEI_DIV_SIBLINGS.contains(&tag) => {
                if let Some(run) = runs.last_mut() {
                    run.push(current);
                }
            }
            _ => {
                if runs.last().is_some_and(|run| !run.is_empty()) {
                    runs.push(Vec::new());
                }
            }
        }
    }
    for run in runs {
        let Some(&first) = run.first() else {
            continue;
        };
        let wrapper = tree.new_element("div");
        tree.add_previous(first, wrapper);
        for child in run {
            tick(&mut visited, call)?;
            tree.append(wrapper, child);
        }
    }
    Ok(())
}

/// Moves `element` out of its `p` parent, one level up, when the grandparent exists: the siblings
/// after it go into a new `p` after it, and the text around it becomes that `p`'s text and tail.
/// The emptied parent is removed when it has no text left.
///
/// Checks `call` about every `CHECK_INTERVAL` following siblings.
// upstream: xml.py::_move_element_one_level_up
pub fn move_element_one_level_up(
    tree: &mut Tree,
    element: NodeId,
    call: &Call<'_>,
) -> Result<(), ExtractError> {
    let Some(parent) = tree
        .node(element)
        .parent()
        .filter(|&parent| tree.node(parent).parent().is_some())
    else {
        return Ok(());
    };

    let new_elem = tree.new_element("p");
    let mut following = Vec::new();
    let mut visited = 0;
    let mut sibling = tree.node(element).next_sibling();
    while let Some(current) = sibling {
        tick(&mut visited, call)?;
        following.push(current);
        sibling = tree.node(current).next_sibling();
    }
    for current in following {
        tick(&mut visited, call)?;
        tree.append(new_elem, current);
    }

    tree.add_next(parent, element);

    if let Some(tail) = stripped_tail(tree.node(element).tail.as_deref()).filter(|t| !t.is_empty())
    {
        tree.node_mut(new_elem).text = Some(tail);
        tree.node_mut(element).tail = None;
    }
    if let Some(tail) = stripped_tail(tree.node(parent).tail.as_deref()).filter(|t| !t.is_empty()) {
        tree.node_mut(new_elem).tail = Some(tail);
        tree.node_mut(parent).tail = None;
    }

    let has_content = tree.child_count(new_elem) > 0
        || truthy(tree.node(new_elem).text.as_deref()).is_some()
        || truthy(tree.node(new_elem).tail.as_deref()).is_some();
    if has_content {
        tree.add_next(element, new_elem);
    }

    if tree.child_count(parent) == 0 && truthy(tree.node(parent).text.as_deref()).is_none() {
        tree.remove(parent);
    }
    Ok(())
}

/// Handles the tail of `element` for an `ab` or `p`: the tail is stripped, and a `p` takes it into
/// its text, while another element gets a new `p` after it holding the tail. The tail is then gone.
// upstream: xml.py::_handle_unwanted_tails
pub fn handle_unwanted_tails(tree: &mut Tree, element: NodeId) {
    let stripped = stripped_tail(tree.node(element).tail.as_deref());
    tree.node_mut(element).tail = stripped.clone();
    let Some(tail) = stripped.filter(|tail| !tail.is_empty()) else {
        return;
    };
    if tree.tag(element) == Some("p") {
        let text = truthy(tree.node(element).text.as_deref()).map(str::to_owned);
        tree.node_mut(element).text = Some(match text {
            Some(text) => format!("{text} {tail}"),
            None => tail,
        });
    } else {
        let sibling = tree.new_element("p");
        tree.node_mut(sibling).text = Some(tail);
        tree.add_next(element, sibling);
    }
    tree.node_mut(element).tail = None;
}

/// The stripped tail, `None` when the tail is absent or empty. A tail of only whitespace stays as
/// `Some("")`, as upstream keeps the empty string it strips to.
fn stripped_tail(tail: Option<&str>) -> Option<String> {
    truthy(tail).map(|tail| strip(tail).to_owned())
}

/// The value, unless it is absent or empty.
fn truthy(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.is_empty())
}

/// The non-empty values of `parts` joined with `separator`, as `", ".join(filter(None, parts))`.
fn join_truthy(parts: &[Option<&str>], separator: &str) -> String {
    parts
        .iter()
        .filter_map(|part| truthy(*part))
        .collect::<Vec<_>>()
        .join(separator)
}

/// Appends an element `name` to `parent` with `attrs` in order and `text` as its text, and returns
/// it. `None` leaves the element without text.
fn sub_element(
    tree: &mut Tree,
    parent: NodeId,
    name: &str,
    attrs: &[(&str, &str)],
    text: Option<&str>,
) -> NodeId {
    let id = tree.new_element(name);
    for (key, value) in attrs {
        tree.set(id, key, value);
    }
    tree.node_mut(id).text = text.map(str::to_owned);
    tree.append(parent, id);
    id
}
