mod construct;
mod edit;
mod iter;
mod nodes;
mod script;
mod strip;
mod tostring;

use std::time::{Duration, Instant};

use super::*;
use crate::cancel::{Call, CancelToken};
use crate::error::{ExtractError, Rejection};
use crate::settings::DEFAULT_MAX_NODES;

fn parse(html: &str) -> Tree {
    parse_document(html, &ParseLimits::default(), &Call::default()).unwrap()
}

fn find(tree: &Tree, name: &str) -> NodeId {
    let mut stack = vec![tree.root()];
    while let Some(id) = stack.pop() {
        if &*tree.node(id).name == name {
            return id;
        }
        stack.extend(tree.children(id));
    }
    panic!("no element named {name}");
}

// Walks, strip and copy. Expected values come from lxml 6.1.3 / libxml2 2.14.6 through
// `scripts/oracle.sh`; the trees there are `etree.fromstring` of the same markup, rooted in the
// fragment's `div` here.

fn fragment(html: &str) -> Tree {
    parse_fragment(html, &ParseLimits::default(), &Call::default()).unwrap()
}

/// `<a>text<b/>tail</a>` of `id` and its descendants, without the tail of `id` itself.
fn xml(tree: &Tree, id: NodeId) -> String {
    let node = tree.node(id);
    let name = &*node.name;
    if node.text.is_none() && tree.first_child(id).is_none() {
        return format!("<{name}/>");
    }
    let mut out = format!("<{name}>{}", node.text.as_deref().unwrap_or(""));
    for child in tree.children(id) {
        out.push_str(&xml(tree, child));
        out.push_str(tree.node(child).tail.as_deref().unwrap_or(""));
    }
    out.push_str(&format!("</{name}>"));
    out
}
