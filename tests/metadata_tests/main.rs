#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Port of upstream `tests/metadata_tests.py` at the release pinned in `Cargo.toml`
//! (`[package.metadata.upstream]`). Each test keeps its upstream name
//! and its cases in upstream order; an adaptation is commented where the test differs.

#[allow(dead_code)] // the helpers this binary does not call belong to the other test binaries
#[path = "../common/mod.rs"]
mod common;

use std::collections::HashSet;

use common::{blank_document, extract, json_of, parse_html};

use trafilatura_rust::__internals::{
    Config, DateParams, Extractor, Format, JSON_MINIFY, NodeId, Selector, Tree, check_authors,
    extract_json, extract_json_parse_error, extract_meta_json, extract_metadata, extract_metainfo,
    extract_title, extract_url, normalize_tags, process_parent,
};
use trafilatura_rust::{Call, ExtractError};

mod page_fields;
mod tags_json_license;

fn owned(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}
