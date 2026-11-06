#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Port of upstream `tests/json_metadata_tests.py` at the release pinned in `Cargo.toml`
//! (`[package.metadata.upstream]`). Each test keeps its upstream name.
//! Adaptations are commented where a test differs.

#[allow(dead_code)] // the helpers this binary does not call belong to the other test binaries
#[path = "../common/mod.rs"]
mod common;

use trafilatura_rust::__internals::{
    JSON_AUTHOR_1, JSON_AUTHOR_2, extract_json, extract_json_author, extract_meta_json,
    normalize_authors, normalize_json, process_parent,
};
use trafilatura_rust::{Call, Document};

use common::{blank_document, extract, json_of, parse_html};

mod json_extraction;
mod normalize_and_types;

/// `extract_meta_json(html.fromstring(html), metadata)` into `metadata`.
fn fill_meta_json(metadata: &mut Document, html: &str) {
    let (tree, root) = parse_html(html);
    extract_meta_json(&tree, root, metadata, &Call::default()).unwrap();
}

/// `extract_meta_json` of `html` into an empty `Document()`.
fn meta_json(html: &str) -> Document {
    let mut metadata = blank_document();
    fill_meta_json(&mut metadata, html);
    metadata
}

/// `process_parent(json_list, Document())` for a JSON list of objects.
fn process_parent_of(text: &str) -> Document {
    let json = json_of(text);
    let parent: Vec<_> = json.items(json.root()).collect();
    let mut metadata = blank_document();
    process_parent(&json, &parent, &mut metadata, &Call::default()).unwrap();
    metadata
}

/// `extract_json(schema, Document())` for a JSON value.
fn extract_json_of(text: &str) -> Document {
    let json = json_of(text);
    let mut metadata = blank_document();
    extract_json(&json, json.root(), &mut metadata, &Call::default()).unwrap();
    metadata
}

/// `normalize_authors(current_authors, author_string)`.
fn authors(current: Option<&str>, author_string: &str) -> Option<String> {
    normalize_authors(current, author_string, &Call::default()).unwrap()
}
