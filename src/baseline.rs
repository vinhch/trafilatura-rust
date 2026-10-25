//! The baseline extraction of upstream `baseline.py`: text from embedded JSON, article tags,
//! paragraphs, teasers or the whole page body, and `html2txt`.
//!
//! A function that takes the `Tree` works on its arena: a result element is a detached node of
//! it. `walk_json` and the `@type` search are loops over the flat `pyjson` document, so a deeply
//! nested script cannot overflow the stack (addendum, Recursion and cancellation).

use std::sync::LazyLock;

use regex::Regex;

use crate::cancel::Call;
use crate::dom::{LocalName, NodeId, Ns, ParseLimits, Tree, parse_fragment};
use crate::error::{ExtractError, Rejection, caught};
use crate::html_processing::tick;
use crate::pyhtml::unescape;
use crate::pyjson::{Json, JsonRef, Kind, loads};
use crate::pystr::{self, PY_SPACE_CLASS, ignore_case_i};
use crate::settings::{BASIC_CLEAN_XPATH, DEDUPE_SCAN_CAP, MAX_FILE_SIZE, MIN_DUPLICATE_LENGTH};
use crate::utils::{HtmlInput, as_list, load_html, remove_control_characters, trim};
use crate::xml::delete_element;

/// The element names `HTML_MARKUP` takes as real markup, as a regex alternation.
// upstream: baseline.py::_HTML_TAG_NAMES
pub const HTML_TAG_NAMES: &str = "a|abbr|address|article|aside|b|blockquote|body|br|caption|cite|code|dd|del|div|dl|dt|\
em|figcaption|figure|footer|h[1-6]|head|header|hr|html|i|img|ins|kbd|li|main|mark|nav|\
ol|p|pre|q|quote|s|section|small|span|strong|sub|summary|sup|table|tbody|td|tfoot|th|\
thead|time|title|tr|u|ul";

// Detects a closing, bare, self-closing or attribute-bearing tag (an `=` in it); must not fire on
// comparison prose. `re.IGNORECASE` also folds U+0130 and U+0131 to `i` and `\s` is Python's
// (`ignore_case_i`, `PY_SPACE_CLASS`).
// upstream: baseline.py::_HTML_MARKUP
static HTML_MARKUP: LazyLock<Option<Regex>> = LazyLock::new(|| {
    let tags = ignore_case_i(HTML_TAG_NAMES);
    Regex::new(&format!(
        r"(?i)</({tags})>|<({tags})([{PY_SPACE_CLASS}][^<>]*=[^<>]*)?/?>"
    ))
    .ok()
});

/// Schema.org text properties usable as page content.
// upstream: baseline.py::_JSON_TEXT_KEYS
pub const JSON_TEXT_KEYS: [&str; 2] = ["articleBody", "reviewBody"];

/// Types whose description carries the page content (the teaser tier).
// upstream: baseline.py::_DESCRIPTION_TYPES
pub const DESCRIPTION_TYPES: [&str; 2] = ["Product", "VideoObject"];

/// The cheap script pre-filter: bare strings match property names, quoted ones match `@type`
/// values. A script is parsed only when its text holds one of them.
// upstream: baseline.py::_JSON_HOOKS (and `_JSON_HOOKS_RE`, a plain substring test here)
pub const JSON_HOOKS: [&str; 7] = [
    "articleBody",
    "reviewBody",
    "recipeInstructions",
    "acceptedAnswer",
    "\"Product\"",
    "\"VideoObject\"",
    "\"HowTo\"",
];

/// A strategy must accumulate more than this many characters to be accepted, and a single
/// `article` must carry more than this to count as content.
// upstream: baseline.py::_MIN_CONTENT_LENGTH
pub const MIN_CONTENT_LENGTH: usize = 100;

/// Block-level elements: their boundaries separate text runs.
// upstream: baseline.py::_BLOCK_ELEMS
pub const BLOCK_ELEMS: [&str; 34] = [
    "address",
    "article",
    "aside",
    "blockquote",
    "br",
    "dd",
    "div",
    "dl",
    "dt",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "summary",
    "table",
    "td",
    "th",
    "tr",
    "ul",
];

/// The tags the paragraph strategy scrapes; an element below one of them is part of it.
const SCRAPED_TAGS: [&str; 6] = ["blockquote", "code", "p", "pre", "q", "quote"];

/// The deepest `walk_json` level: a dict on this level ends the walk of its script after its own
/// texts and teaser. Python's recursion limit cuts the walk at a depth that depends on the
/// caller's stack (996 under a top-level `baseline`, 992 under `extract`); this is the `extract`
/// value.
// deviation: D-10
const WALK_JSON_MAX_LEVEL: usize = 992;

/// The deepest object (counting the `@type` value as 1) of which Python's `str` still builds the
/// text; one level deeper it raises `RecursionError`.
// deviation: D-10
const TYPE_MAX_DEPTH: usize = 9996;

/// Whether `raw` holds markup: a closing, bare, self-closing or attribute-bearing tag.
fn has_markup(raw: &str) -> bool {
    HTML_MARKUP
        .as_ref()
        .is_some_and(|markup| markup.is_match(raw))
}

/// Removes a few section types from the document.
// upstream: baseline.py::basic_cleaning
pub fn basic_cleaning(tree: &mut Tree, root: NodeId, call: &Call<'_>) -> Result<(), ExtractError> {
    let mut visited = 0;
    for element in BASIC_CLEAN_XPATH(tree, root, call)? {
        tick(&mut visited, call)?;
        delete_element(tree, element, true);
    }
    Ok(())
}

/// The elements of `nodes` named one of `tags`, in order; checks `call` for every node walked.
fn collect_tagged(
    tree: &Tree,
    call: &Call<'_>,
    nodes: impl Iterator<Item = NodeId>,
    tags: &[&str],
) -> Result<Vec<NodeId>, ExtractError> {
    let mut found = Vec::new();
    let mut visited = 0;
    for node in nodes {
        tick(&mut visited, call)?;
        if tree.tag(node).is_some_and(|name| tags.contains(&name)) {
            found.push(node);
        }
    }
    Ok(found)
}

/// Whether the text of a `@type` value holds a description type, `None` where Python's `str`
/// of it raises `RecursionError`. The whole value is read either way, as `str` builds all of it
/// before the test: a string anywhere inside, key or element, counts.
// deviation: D-10
fn type_has_description_type(
    json: &Json,
    value: JsonRef,
    call: &Call<'_>,
) -> Result<Option<bool>, ExtractError> {
    let holds = |text: &str| DESCRIPTION_TYPES.iter().any(|kind| text.contains(kind));
    let mut found = false;
    let mut visited = 0;
    let mut pending = vec![(value, 1_usize)];
    while let Some((node, depth)) = pending.pop() {
        tick(&mut visited, call)?;
        if depth > TYPE_MAX_DEPTH {
            return Ok(None);
        }
        match json.kind(node) {
            Kind::Str => found |= json.as_str(node).is_some_and(holds),
            Kind::List => pending.extend(json.items(node).map(|item| (item, depth + 1))),
            Kind::Object => {
                for (key, entry) in json.entries(node) {
                    found |= holds(key);
                    pending.push((entry, depth + 1));
                }
            }
            Kind::Null | Kind::Bool | Kind::Number => {}
        }
    }
    Ok(Some(found))
}

/// The texts one dict of a JSON-LD script contributes, in upstream's order. False when the
/// `@type` search hit Python's recursion limit, which ends the walk of the script.
// upstream: baseline.py::_walk_json
fn collect_item(
    json: &Json,
    item: JsonRef,
    bodies: &mut Vec<String>,
    teasers: &mut Vec<String>,
    call: &Call<'_>,
) -> Result<bool, ExtractError> {
    let string_of = |value: Option<JsonRef>| value.and_then(|value| json.as_str(value));
    for key in JSON_TEXT_KEYS {
        if let Some(text) = string_of(json.get(item, key)).filter(|text| !text.is_empty()) {
            bodies.push(text.to_string());
        }
    }
    // recipe/how-to instructions: a string, strings, or step objects carrying "text", possibly
    // one itemListElement level down (HowToDirection)
    for key in ["recipeInstructions", "step"] {
        for step in as_list(json, json.get(item, key)) {
            match json.kind(step) {
                Kind::Str => bodies.push(json.as_str(step).unwrap_or_default().to_string()),
                Kind::Object => {
                    let nested = as_list(json, json.get(step, "itemListElement"));
                    for sub in std::iter::once(step).chain(nested) {
                        if json.kind(sub) == Kind::Object
                            && let Some(text) = string_of(json.get(sub, "text"))
                        {
                            bodies.push(text.to_string());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    // FAQ answers
    if let Some(answer) = json
        .get(item, "acceptedAnswer")
        .filter(|&answer| json.kind(answer) == Kind::Object)
        && let Some(text) = string_of(json.get(answer, "text"))
    {
        bodies.push(text.to_string());
    }
    let is_description_type = match json.get(item, "@type") {
        Some(value) => match type_has_description_type(json, value, call)? {
            Some(found) => found,
            None => return Ok(false),
        },
        None => false,
    };
    if is_description_type && let Some(text) = string_of(json.get(item, "description")) {
        teasers.push(text.to_string());
    }
    Ok(true)
}

/// A step of `walk_json`: read a value as a list of dicts, or read one dict. `usize` is the
/// level of the recursive frame upstream would be in.
enum Task {
    Walk(JsonRef, usize),
    Item(JsonRef, usize),
}

/// Collects schema.org text content from parsed JSON-LD (list-wrapped and `@graph`-nested nodes
/// included) into `bodies`; teasers (Product/VideoObject descriptions) go to `teasers`.
///
/// An explicit stack gives upstream's order: a dict's own texts, then its `@graph`, then its
/// `mainEntity`, each list in order. The walk stops where Python's recursion limit would
/// (D-10), keeping what it has collected.
// upstream: baseline.py::_walk_json
// deviation: D-10
fn walk_json(
    json: &Json,
    node: Option<JsonRef>,
    bodies: &mut Vec<String>,
    teasers: &mut Vec<String>,
    call: &Call<'_>,
) -> Result<(), ExtractError> {
    let mut pending: Vec<Task> = node.map(|node| Task::Walk(node, 1)).into_iter().collect();
    let mut visited = 0;
    while let Some(task) = pending.pop() {
        match task {
            Task::Walk(node, level) => {
                let dicts = as_list(json, Some(node))
                    .into_iter()
                    .filter(|&item| json.kind(item) == Kind::Object);
                let dicts: Vec<JsonRef> = dicts.collect();
                pending.extend(dicts.into_iter().rev().map(|item| Task::Item(item, level)));
            }
            Task::Item(item, level) => {
                tick(&mut visited, call)?;
                if !collect_item(json, item, bodies, teasers, call)? {
                    return Ok(());
                }
                // upstream enters the next frame for an absent container too
                if level >= WALK_JSON_MAX_LEVEL {
                    return Ok(());
                }
                // the stack pops `@graph` first
                for container in ["mainEntity", "@graph"] {
                    if let Some(value) = json.get(item, container) {
                        pending.push(Task::Walk(value, level + 1));
                    }
                }
            }
        }
    }
    Ok(())
}

/// Extracts post HTML from the JSON a Discourse forum preloads into an attribute (the page body
/// is empty). A `posts` value that is not iterable is upstream's `TypeError`.
// upstream: baseline.py::_discourse_texts
fn discourse_texts(
    tree: &Tree,
    root: NodeId,
    call: &Call<'_>,
) -> Result<Vec<String>, ExtractError> {
    let Some(node) = tree
        .iterdescendants(root, &["div"])
        .find(|&div| tree.get(div, "id") == Some("data-preloaded"))
    else {
        return Ok(Vec::new());
    };
    let attribute = tree.get(node, "data-preloaded").unwrap_or_default();
    let Some(preloaded) = loads(attribute, true, call)? else {
        return Ok(Vec::new());
    };
    let top = preloaded.root();
    if preloaded.kind(top) != Kind::Object {
        return Ok(Vec::new());
    }
    let mut texts = Vec::new();
    let mut visited = 0;
    for (key, value) in preloaded.entries(top) {
        if !key.starts_with("topic_") {
            continue;
        }
        let Some(topic) = preloaded
            .as_str(value)
            .map(|raw| loads(raw, true, call))
            .transpose()?
            .flatten()
        else {
            continue;
        };
        let Some(posts) = topic
            .get(topic.root(), "post_stream")
            .and_then(|stream| topic.get(stream, "posts"))
        else {
            continue;
        };
        match topic.kind(posts) {
            Kind::List => {
                for post in topic.items(posts) {
                    tick(&mut visited, call)?;
                    if topic.kind(post) == Kind::Object
                        && let Some(cooked) = topic
                            .get(post, "cooked")
                            .and_then(|cooked| topic.as_str(cooked))
                    {
                        texts.push(cooked.to_string());
                    }
                }
            }
            // iterated silently: a string's characters and an object's keys are no dicts
            Kind::Str | Kind::Object => {}
            Kind::Null | Kind::Bool | Kind::Number => {
                return Err(ExtractError::Rejected(Rejection::InvalidValue));
            }
        }
    }
    Ok(texts)
}

/// Derives clean text from an embedded-JSON value which may carry (escaped) HTML markup.
///
/// A fragment parse that fails with the node limit falls back to `trim(raw)`, as upstream's
/// `except` does for any parse failure; a cancellation propagates.
// upstream: baseline.py::_render_text
// deviation: D-5
fn render_text(raw: &str, limits: &ParseLimits, call: &Call<'_>) -> Result<String, ExtractError> {
    // some sites HTML-escape the content ("&lt;p&gt;…"); unescape so markup is parsed, not
    // leaked. Control characters go after the unescape (`&#1;` makes one).
    let raw = unescape(raw)?;
    let raw = remove_control_characters(&raw);
    if has_markup(&raw)
        && let Some(mut fragment) = caught(parse_fragment(&raw, limits, call))?
    {
        let root = fragment.root();
        return block_text(&mut fragment, root, call);
    }
    Ok(trim(&raw))
}

/// `render_text` of each raw value, in order.
fn render_all(
    raws: &[String],
    limits: &ParseLimits,
    call: &Call<'_>,
) -> Result<Vec<String>, ExtractError> {
    raws.iter()
        .map(|raw| render_text(raw, limits, call))
        .collect()
}

/// Wraps one paragraph per text in a fresh `body` element, optionally dropping repeated content,
/// and returns the element with the paragraphs joined by newlines. A text of `MIN_DUPLICATE_LENGTH`
/// characters or fewer is kept even if it recurs, and the substring scan stops once the
/// accumulated text exceeds `scan_cap` characters. Checks `call` for every text: the scan is
/// quadratic (texts x `scan_cap`).
// upstream: baseline.py::_build_body
pub fn build_body(
    tree: &mut Tree,
    texts: Vec<String>,
    dedupe: bool,
    scan_cap: usize,
    call: &Call<'_>,
) -> Result<(NodeId, String), ExtractError> {
    let postbody = tree.new_element("body");
    let mut temp_text = String::new();
    let mut temp_len = 0_usize;
    for text in texts {
        call.check()?;
        // strip control chars (element inputs skip load_html's cleaning)
        let text = remove_control_characters(&text);
        if text.is_empty() {
            continue;
        }
        // newline-joined (trimmed text has none), so no match spans two paragraphs
        let text_len = text.chars().count();
        if dedupe
            && text_len > MIN_DUPLICATE_LENGTH
            && temp_len <= scan_cap
            && temp_text.contains(&*text)
        {
            continue;
        }
        let paragraph = tree.new_element("p");
        tree.node_mut(paragraph).text = Some(text.to_string());
        tree.append(postbody, paragraph);
        if !temp_text.is_empty() {
            temp_text.push('\n');
            temp_len = temp_len.saturating_add(1);
        }
        temp_text.push_str(&text);
        temp_len = temp_len.saturating_add(text_len);
    }
    Ok((postbody, temp_text))
}

/// A body element, its text and the text's length in characters.
type Found = (NodeId, String, usize);

/// Builds a body from the texts and accepts it if it carries enough content.
// upstream: baseline.py::_attempt
fn attempt(
    tree: &mut Tree,
    texts: Vec<String>,
    dedupe: bool,
    call: &Call<'_>,
) -> Result<Option<Found>, ExtractError> {
    let (postbody, temp_text) = build_body(tree, texts, dedupe, DEDUPE_SCAN_CAP, call)?;
    let len = temp_text.chars().count();
    Ok((len > MIN_CONTENT_LENGTH).then_some((postbody, temp_text, len)))
}

/// Gathers the raw text content embedded as JSON: full-text bodies and teaser descriptions. The
/// values may carry markup; `render_text` renders them at use time. A script that is not JSON
/// is skipped.
// upstream: baseline.py::_collect_json_content
fn collect_json_content(
    tree: &Tree,
    root: NodeId,
    call: &Call<'_>,
) -> Result<(Vec<String>, Vec<String>), ExtractError> {
    let mut bodies = Vec::new();
    let mut teasers = Vec::new();
    let mut visited = 0;
    for element in tree.iterdescendants(root, &["script"]) {
        tick(&mut visited, call)?;
        if tree.get(element, "type") != Some("application/ld+json") {
            continue;
        }
        let Some(text) = tree
            .node(element)
            .text
            .as_deref()
            .filter(|text| JSON_HOOKS.iter().any(|hook| text.contains(hook)))
        else {
            continue;
        };
        // strict=False: real pages carry raw newlines/tabs inside JSON strings
        let Some(json) = loads(text, false, call)? else {
            continue;
        };
        walk_json(&json, Some(json.root()), &mut bodies, &mut teasers, call)?;
    }
    // Discourse forums render posts client-side but embed them as JSON in an attribute
    bodies.extend(discourse_texts(tree, root, call)?);
    Ok((bodies, teasers))
}

/// The strategies of `baseline` on the tree below `root`, which they modify.
fn baseline_in(
    tree: &mut Tree,
    root: NodeId,
    limits: &ParseLimits,
    call: &Call<'_>,
) -> Result<Found, ExtractError> {
    // scrape from embedded JSON: full-text properties first, teaser descriptions kept for later.
    // dedupe: pages often embed the same JSON-LD block twice (theme + SEO plugin)
    let (json_bodies, json_teasers) = collect_json_content(tree, root, call)?;
    let bodies = render_all(&json_bodies, limits, call)?;
    if let Some(found) = attempt(tree, bodies, true, call)? {
        return Ok(found);
    }

    basic_cleaning(tree, root, call)?;

    // article tags: a dominant one relegates much smaller siblings to noise (related teasers),
    // similar-sized ones are all content (forum posts). Nested articles are counted in their
    // ancestor
    let articles = collect_tagged(tree, call, tree.iterdescendants(root, &[]), &["article"])?;
    let mut article_texts = Vec::new();
    for article in articles {
        if tree.iterancestors(article, &["article"]).next().is_some() {
            continue;
        }
        let text = block_text(tree, article, call)?;
        let len = text.chars().count();
        if len > MIN_CONTENT_LENGTH {
            article_texts.push((text, len));
        }
    }
    let longest = article_texts.iter().map(|&(_, len)| len).max();
    if let Some(longest) = longest {
        // len >= longest / 5; never empty: the longest article passes both its own length gate
        // and the cutoff
        let kept = article_texts
            .into_iter()
            .filter(|&(_, len)| len.saturating_mul(5) >= longest)
            .map(|(text, _)| text)
            .collect();
        if let Some(found) = attempt(tree, kept, false, call)? {
            return Ok(found);
        }
    }

    // scrape from text paragraphs, dropping repeats: a nested element (e.g. <p> in
    // <blockquote>) duplicates part of its container's text, collected first in document order.
    // Skip any element whose ancestor is already in the scraped set (<p><code>, <blockquote><p>)
    let scraped = collect_tagged(tree, call, tree.iter(root, &[]), &SCRAPED_TAGS)?;
    let mut paragraphs = Vec::new();
    for element in scraped {
        if tree.iterancestors(element, &SCRAPED_TAGS).next().is_none() {
            paragraphs.push(block_text(tree, element, call)?);
        }
    }
    if let Some(found) = attempt(tree, paragraphs, true, call)? {
        return Ok(found);
    }

    // teaser tier: Product/VideoObject descriptions, once every full-text source came up empty.
    // A short summary must not shadow a longer body dump: it wins only if the dump is shorter
    let teasers = render_all(&json_teasers, limits, call)?;
    let teaser = attempt(tree, teasers, true, call)?;

    // default strategy: clean the tree and take everything
    let postbody = tree.new_element("body");
    if let Some(body) = tree.find(root, "body") {
        let paragraph = tree.new_element("p");
        tree.append(postbody, paragraph);
        let mut pieces = Vec::new();
        let mut visited = 0;
        for piece in tree.itertext(body) {
            tick(&mut visited, call)?;
            let piece = trim(piece);
            if !piece.is_empty() {
                pieces.push(piece);
            }
        }
        let text = remove_control_characters(&pieces.join("\n")).into_owned();
        let len = text.chars().count();
        tree.node_mut(paragraph).text = Some(text.clone());
        if teaser
            .as_ref()
            .is_none_or(|&(_, _, teaser_len)| len >= teaser_len)
        {
            return Ok((postbody, text, len));
        }
    }
    Ok(teaser.unwrap_or((postbody, String::new(), 0)))
}

/// A parsed document, `None` when the load gives upstream's `None` (an empty tree); every other
/// rejection and a cancellation propagate.
fn load(
    input: HtmlInput<'_>,
    limits: &ParseLimits,
    call: &Call<'_>,
) -> Result<Option<Tree>, ExtractError> {
    match load_html(input, MAX_FILE_SIZE, limits, call) {
        Ok(tree) => Ok(Some(tree)),
        Err(ExtractError::Rejected(Rejection::EmptyTree)) => Ok(None),
        Err(error) => Err(error),
    }
}

/// Uses a baseline extraction targeting content in embedded JSON or text elements: tries a series
/// of sources and takes the first that yields enough text (JSON-LD properties and Discourse
/// posts, article tags, paragraphs, teaser descriptions, the raw text of the page body).
///
/// Returns the tree, a `body` element in it holding the extracted paragraphs, the text and its
/// length in characters. A document that does not load (an empty tree) gives an empty `body`,
/// `""` and 0.
// upstream: baseline.py::baseline
pub fn baseline(
    input: HtmlInput<'_>,
    limits: &ParseLimits,
    call: &Call<'_>,
) -> Result<(Tree, NodeId, String, usize), ExtractError> {
    let Some(mut tree) = load(input, limits, call)? else {
        let tree = Tree::new(LocalName::from("body"), Ns::Html, Vec::new());
        let body = tree.root();
        return Ok((tree, body, String::new(), 0));
    };
    let root = tree.root();
    let (body, text, len) = baseline_in(&mut tree, root, limits, call)?;
    Ok((tree, body, text, len))
}

/// `baseline` on an element of `tree`: the element is copied first (the baseline modifies its
/// tree), the caller's tree is left as it is. The `body` element joins `tree` detached.
// upstream: baseline.py::baseline
pub fn baseline_tree(
    tree: &mut Tree,
    root: NodeId,
    limits: &ParseLimits,
    call: &Call<'_>,
) -> Result<(NodeId, String, usize), ExtractError> {
    let copy = tree.copy(root);
    baseline_in(tree, copy, limits, call)
}

/// Runs basic html2txt on a document: the text of its `body`, with `clean` removing potentially
/// undesirable elements. An empty tree gives `""`, as does a parsed document without a `body` (a
/// feed, say, under libxml2); html5ever always builds a `body`, so that branch does not run for
/// the input of this function (D-2).
// upstream: baseline.py::html2txt
pub fn html2txt(
    input: HtmlInput<'_>,
    clean: bool,
    limits: &ParseLimits,
    call: &Call<'_>,
) -> Result<String, ExtractError> {
    let Some(mut tree) = load(input, limits, call)? else {
        return Ok(String::new());
    };
    let root = tree.root();
    let Some(body) = tree.find(root, "body") else {
        return Ok(String::new());
    };
    html2txt_body(&mut tree, body, clean, call)
}

/// `html2txt` on an element of `tree`: the element is copied first, the caller's tree is left as
/// it is. An element without a `body` is itself the content.
// upstream: baseline.py::html2txt
pub fn html2txt_tree(
    tree: &mut Tree,
    root: NodeId,
    clean: bool,
    call: &Call<'_>,
) -> Result<String, ExtractError> {
    let copy = tree.copy(root);
    let body = tree.find(copy, "body").unwrap_or(copy);
    html2txt_body(tree, body, clean, call)
}

fn html2txt_body(
    tree: &mut Tree,
    body: NodeId,
    clean: bool,
    call: &Call<'_>,
) -> Result<String, ExtractError> {
    if clean {
        basic_cleaning(tree, body, call)?;
    }
    spaced_text(tree, body, call)
}

/// The text of `element` with word boundaries kept at block tags; `element` is left as it is.
// upstream: baseline.py::block_text
pub fn block_text(
    tree: &mut Tree,
    element: NodeId,
    call: &Call<'_>,
) -> Result<String, ExtractError> {
    let copy = tree.copy(element);
    spaced_text(tree, copy, call)
}

/// ` text` for a block's text or tail: a space in front, a lone space for none.
fn spaced(text: Option<&str>) -> String {
    match text {
        Some(text) if !text.is_empty() => format!(" {}", remove_control_characters(text)),
        _ => " ".to_string(),
    }
}

/// Collects the text, spacing block boundaries in place so adjacent runs don't stick (minified
/// pages).
// upstream: baseline.py::_spaced_text
fn spaced_text(tree: &mut Tree, element: NodeId, call: &Call<'_>) -> Result<String, ExtractError> {
    let blocks = collect_tagged(tree, call, tree.iter(element, &[]), &BLOCK_ELEMS)?;
    for block in blocks {
        let node = tree.node(block);
        let (text, tail) = (spaced(node.text.as_deref()), spaced(node.tail.as_deref()));
        let node = tree.node_mut(block);
        node.text = Some(text);
        node.tail = Some(tail);
    }
    let text = tree.text_content(element);
    Ok(pystr::split_whitespace(&text).collect::<Vec<_>>().join(" "))
}

#[cfg(test)]
mod tests;
