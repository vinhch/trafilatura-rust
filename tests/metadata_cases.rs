#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! `extract_metadata` against the pages upstream reads (`docs/parity-harness.md`). The `date` of a
//! case is not compared: htmldate lands in phase 4c.

#[path = "metadata_cases/cases.rs"]
mod cases;

use std::collections::HashSet;

use cases::{METADATA_CASES, MetadataCase};
use trafilatura_rust::__internals::{DateParams, ParseLimits, extract_metadata, parse_document};
use trafilatura_rust::{Call, Document};

fn run(case: &MetadataCase) -> Document {
    let call = Call::default();
    let mut tree = parse_document(case.html, &ParseLimits::default(), &call).unwrap();
    let root = tree.root();
    let date_config = DateParams {
        original_date: true,
        extensive_search: false,
        max_date: Some("2024-01-31".to_owned()),
    };
    let blacklist: HashSet<String> = case
        .author_blacklist
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    extract_metadata(
        &mut tree,
        root,
        case.default_url,
        Some(&date_config),
        false,
        &blacklist,
        &call,
    )
    .unwrap()
}

/// The fields of `case`, as `(name, rust value, upstream value)`, that differ.
fn differences(case: &MetadataCase, document: &Document) -> Vec<String> {
    let strings = [
        ("title", &document.title, case.title),
        ("author", &document.author, case.author),
        ("url", &document.url, case.url),
        ("hostname", &document.hostname, case.hostname),
        ("description", &document.description, case.description),
        ("sitename", &document.sitename, case.sitename),
        ("fingerprint", &document.fingerprint, case.fingerprint),
        ("id", &document.id, case.id),
        ("license", &document.license, case.license),
        ("comments", &document.comments, case.comments),
        ("raw_text", &document.raw_text, case.raw_text),
        ("text", &document.text, case.text),
        ("language", &document.language, case.language),
        ("image", &document.image, case.image),
        ("pagetype", &document.pagetype, case.pagetype),
    ];
    let mut found: Vec<String> = strings
        .iter()
        .filter(|(_, rust, upstream)| rust.as_deref() != *upstream)
        .map(|(name, rust, upstream)| format!("{name}: rust {rust:?}, upstream {upstream:?}"))
        .collect();
    for (name, rust, upstream) in [
        ("categories", &document.categories, case.categories),
        ("tags", &document.tags, case.tags),
    ] {
        let rust: Option<Vec<&str>> = rust
            .as_ref()
            .map(|list| list.iter().map(String::as_str).collect());
        if rust.as_deref() != upstream {
            found.push(format!("{name}: rust {rust:?}, upstream {upstream:?}"));
        }
    }
    found
}

#[test]
fn metadata_cases_match_python() {
    assert!(!METADATA_CASES.is_empty());
    let mut mismatches = Vec::new();
    for (index, case) in METADATA_CASES.iter().enumerate() {
        let found = differences(case, &run(case));
        if !found.is_empty() {
            let html: String = case.html.chars().take(300).collect();
            mismatches.push(format!(
                "case {index} default_url={:?} blacklist={:?}\n{html}\n{}",
                case.default_url,
                case.author_blacklist,
                found.join("\n")
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of {} cases differ from upstream\n{}",
        mismatches.len(),
        METADATA_CASES.len(),
        mismatches.join("\n")
    );
}
