mod dispatch;
mod document;
mod lists;
mod paragraphs;
mod recover;
mod tables;

use std::borrow::Cow;
use std::sync::Arc;

use super::*;
use crate::cancel::CancelToken;
use crate::dom::{DumpMode, ParseLimits, dump, dump_from, parse_document};
use crate::html_processing::{convert_tags, tree_cleaning};
use crate::main_extractor::content::{extract_body, prune_unwanted_sections, recover_wild_text};
use crate::main_extractor::tables::{handle_table_limited, span};
use crate::settings::{
    Config, DEDUPE_SCAN_CAP, DEFAULT_MAX_NODES, Format, MIN_DUPLICATE_LENGTH, TAG_CATALOG,
};

fn options_with_recall(recall: bool) -> Extractor {
    Extractor::new(
        Config::default(),
        Format::Python,
        false,
        recall,
        None,
        false,
        false,
        Default::default(),
        Default::default(),
    )
}

fn options() -> Extractor {
    options_with_recall(false)
}

fn ctx_for<'a>(options: &'a Extractor, call: &'a Call<'a>) -> Ctx<'a> {
    Ctx {
        options: Cow::Borrowed(options),
        call,
    }
}

/// `etree.fromstring(source)`: tags with double-quoted attributes, self-closing tags, text and
/// tail; the first tag is the root.
fn xml(source: &str) -> Tree {
    let mut tree: Option<Tree> = None;
    let mut stack: Vec<NodeId> = Vec::new();
    for piece in source.split('<').skip(1) {
        let (tag, text) = piece.split_once('>').unwrap();
        if tag.starts_with('/') {
            stack.pop();
        } else {
            let (tag, self_closing) = match tag.strip_suffix('/') {
                Some(tag) => (tag, true),
                None => (tag, false),
            };
            let (name, rest) = tag.split_once(' ').unwrap_or((tag, ""));
            let attrs: Vec<(LocalName, Arc<str>)> = rest
                .split("\" ")
                .filter(|pair| !pair.is_empty())
                .map(|pair| {
                    let (key, value) = pair.split_once("=\"").unwrap();
                    (LocalName::from(key), Arc::from(value.trim_end_matches('"')))
                })
                .collect();
            let id = match (tree.as_mut(), stack.last()) {
                (Some(tree), Some(&parent)) => {
                    tree.add_child(parent, LocalName::from(name), Ns::Html, attrs)
                }
                _ => {
                    let new = Tree::new(LocalName::from(name), Ns::Html, attrs);
                    let root = new.root();
                    tree = Some(new);
                    root
                }
            };
            if !self_closing {
                stack.push(id);
            }
        }
        if let (Some(tree), Some(&top)) = (tree.as_mut(), stack.last()) {
            tree.add_text(top, text);
        }
    }
    tree.unwrap()
}

/// `etree.tostring(element, with_tail=False)`.
fn show(tree: &Tree, id: NodeId) -> String {
    let node = tree.node(id);
    let mut out = format!("<{}", node.name);
    for (key, value) in &node.attrs {
        out.push_str(&format!(" {key}=\"{value}\""));
    }
    if node.text.is_none() && tree.first_child(id).is_none() {
        out.push_str("/>");
        return out;
    }
    out.push('>');
    out.push_str(node.text.as_deref().unwrap_or(""));
    for child in tree.children(id) {
        out.push_str(&show(tree, child));
        out.push_str(tree.node(child).tail.as_deref().unwrap_or(""));
    }
    out.push_str(&format!("</{}>", node.name));
    out
}

/// The `tag` element of the html5 tree of `html` after `tree_cleaning` and `convert_tags` with
/// default options.
fn converted(html: &str, tag: &str) -> (Tree, NodeId) {
    let mut tree = parse_document(html, &ParseLimits::default(), &Call::default()).unwrap();
    let root = tree.root();
    let options = options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let root = tree_cleaning(&mut tree, root, &ctx).unwrap();
    convert_tags(&mut tree, root, &ctx, None).unwrap();
    let element = tree.find(root, tag).unwrap();
    (tree, element)
}
