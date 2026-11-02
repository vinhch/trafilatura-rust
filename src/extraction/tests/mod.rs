mod caller_tree;
mod escalate;
mod formats;
mod forum;
mod rejections;
mod salvage;
mod sequence;

use std::collections::HashSet;

use super::*;
use crate::cancel::CancelToken;
use crate::dom::{LocalName, Ns};
use crate::error::Rejection;
use crate::settings::{Config, MAX_FILE_SIZE};
use crate::utils::{HtmlInput, load_html};

fn options(format: Format, with_metadata: bool) -> Extractor {
    let mut options = Extractor::new(
        Config::default(),
        format,
        false,
        false,
        None,
        false,
        false,
        HashSet::new(),
        HashSet::new(),
    );
    options.with_metadata = with_metadata;
    options
}

/// The options of a cascade run: `Format::Txt`, balanced unless a flag says otherwise.
fn cascade_options(precision: bool, recall: bool, comments: bool) -> Extractor {
    let mut options = Extractor::new(
        Config::default(),
        Format::Txt,
        precision,
        recall,
        None,
        false,
        false,
        HashSet::new(),
        HashSet::new(),
    );
    options.comments = comments;
    options
}

fn load(html: &str) -> Tree {
    load_html(
        HtmlInput::Str(html),
        MAX_FILE_SIZE,
        &ParseLimits::default(),
        &Call::default(),
    )
    .unwrap()
}

fn element(tree: &mut Tree, name: &str, text: Option<&str>, tail: Option<&str>) -> NodeId {
    let id = tree.new_element(name);
    tree.node_mut(id).text = text.map(str::to_owned);
    tree.node_mut(id).tail = tail.map(str::to_owned);
    id
}

/// `n` sentences about `topic`, each ending in a space.
fn prose(topic: &str, n: usize) -> String {
    (0..n)
        .map(|i| {
            format!(
                "{topic} sentence number {i} says something quite different about its subject. "
            )
        })
        .collect()
}

fn full_page(body: &str, head: &str) -> String {
    format!("<html><head><title>T</title>{head}</head><body>{body}</body></html>")
}

// ---------------------------------------------------------------------------- bare_extraction

const COMMENT_DIV: &str =
    "<div id=\"comments\"><p>One comment here</p><p>Second reply here</p></div>";
const COMMENT_BODY: &str = "<body><p>One comment here</p><p>Second reply here</p></body>";

fn main_page(extra: &str) -> String {
    full_page(
        &format!("<article><p>{}</p></article>{extra}", prose("Main", 5)),
        "",
    )
}

fn main_text() -> String {
    prose("Main", 5).trim().to_owned()
}

fn bare_options(format: Format, comments: bool) -> Extractor {
    let mut options = options(format, false);
    options.comments = comments;
    options
}

fn bare(source: Source<'_>, options: &Extractor) -> Result<Document, ExtractError> {
    let call = Call::default();
    let ctx = Ctx {
        options: Cow::Borrowed(options),
        call: &call,
    };
    bare_extraction(source, &ctx, &ParseLimits::default())
}

fn bare_page(html: &str, options: &Extractor) -> Result<Document, ExtractError> {
    bare(Source::Input(HtmlInput::Str(html)), options)
}

fn internal_page(
    html: &str,
    record_id: Option<&str>,
    options: &Extractor,
) -> Result<Document, ExtractError> {
    let call = Call::default();
    let ctx = Ctx {
        options: Cow::Borrowed(options),
        call: &call,
    };
    internal_extraction(
        Source::Input(HtmlInput::Str(html)),
        record_id,
        &ctx,
        &ParseLimits::default(),
    )
}

fn body_xml(doc: &Document) -> String {
    doc.tree.tostring(doc.body, false, false)
}
