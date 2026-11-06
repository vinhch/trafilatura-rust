#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Parity of the crate's parsed tree with the html5 fixtures generated from upstream
//! (`docs/parity-harness.md`). Skips when the fixtures or the upstream checkout are missing.

#[allow(dead_code)]
#[path = "../common/mod.rs"]
mod common;
#[path = "../hunks/mod.rs"]
mod hunks;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::{Map, Value, json};
use trafilatura_rust::__internals::{
    DumpMode, HtmlInput, MAX_FILE_SIZE, ParseLimits, dump, load_html,
};
use trafilatura_rust::{Call, Document, ExtractError, Focus, Format, Input, Options, Rejection};

use hunks::{
    Hunk, is_encoding_hunk, is_encoding_text_hunk, is_html5lib_cell_space_hunk,
    is_python_parser_hunk,
};

mod metadata;
mod output;
mod self_tests;
mod tree;

use metadata::*;
use output::*;
use tree::*;

const CLASSES: [&str; 7] = [
    "match",
    "decompress",
    "encoding",
    "python-parser",
    "html5lib-cell-space",
    "port-bug",
    "upstream-error",
];

const OUTPUT_CLASSES: [&str; 7] = [
    "match",
    "decompress",
    "encoding",
    "known-metadata",
    "normalized",
    "port-bug",
    "upstream-error",
];

const OUTPUT_FORMATS: [(&str, Format); 6] = [
    ("txt", Format::Txt),
    ("markdown", Format::Markdown),
    ("html", Format::Html),
    ("xml", Format::Xml),
    ("xmltei", Format::XmlTei),
    ("json", Format::Json),
];

/// Pages the crate rejects with `InvalidValue` while upstream extracted text, because the oracle's
/// normalized tree dropped an element whose name is not an XML Name (D-2): (page id, reason). None
/// today.
const NORMALIZED_REJECTS: &[(&str, &str)] = &[];

const DIFF_PAGES: usize = 20;
const DIFF_LINES: usize = 10;

/// What the test does with the fixtures it found: `Ok(true)` run, `Ok(false)` skip with a hint, `Err`
/// fail. `require` is `PARITY_REQUIRE_FIXTURES`: CI sets it so a missing or empty fixture set fails
/// the job instead of passing without comparing anything.
fn gate(require: bool, present: bool, page_count: Option<usize>) -> Result<bool, &'static str> {
    match (present, page_count) {
        (false, _) if require => Err(
            "PARITY_REQUIRE_FIXTURES is set but the fixtures or the upstream checkout are missing",
        ),
        (false, _) => Ok(false),
        (true, Some(0)) if require => {
            Err("PARITY_REQUIRE_FIXTURES is set but index.json lists no pages")
        }
        (true, _) => Ok(true),
    }
}

/// The fixture directory, the upstream checkout and the index, or `None` when the test skips.
fn load_corpus() -> Option<(PathBuf, PathBuf, Value)> {
    let require = common::require_fixtures();
    let found = common::fixture_dir().zip(common::upstream_root());
    let index = found
        .as_ref()
        .map(|(fixtures, _)| read_json(&fixtures.join("index.json")));
    let page_count = index
        .as_ref()
        .and_then(|i| i["pages"].as_array())
        .map(Vec::len);
    match gate(require, found.is_some(), page_count) {
        Err(message) => panic!("{message}"),
        Ok(false) => {
            eprintln!(
                "parity: fixtures or the upstream checkout are missing; run scripts/fixtures.sh (or set TRAFILATURA_FIXTURES and TRAFILATURA_UPSTREAM) to enable this test"
            );
            None
        }
        Ok(true) => found
            .zip(index)
            .map(|((fixtures, upstream), index)| (fixtures, upstream, index)),
    }
}

#[test]
fn tree_dump_matches_html5_fixture() {
    let Some((fixtures, upstream, index)) = load_corpus() else {
        return;
    };
    let mut pages = Vec::new();
    let mut parser_count = 0;
    for entry in index["pages"].as_array().unwrap() {
        let (page, parser_differs) = run_page(&upstream, &fixtures, entry);
        parser_count += usize::from(parser_differs);
        pages.push(page);
    }
    let path = write_report(&pages, parser_count);
    let port_bugs: Vec<&str> = pages
        .iter()
        .filter(|p| p.classes.contains(&"port-bug"))
        .map(|p| p.id.as_str())
        .collect();
    eprintln!("parity: report at {}", path.display());
    assert!(
        port_bugs.is_empty(),
        "{} port-bug pages (see {}): {port_bugs:?}",
        port_bugs.len(),
        path.display()
    );
}

#[test]
fn outputs_match_html5_fixture() {
    let Some((fixtures, upstream, index)) = load_corpus() else {
        return;
    };
    let entries = index["pages"].as_array().unwrap();
    let max_date = index["max_date"].as_str().unwrap();
    let results: Vec<OutputResult> = entries
        .iter()
        .flat_map(|entry| run_output_page(&upstream, &fixtures, entry, max_date))
        .collect();
    let path = write_output_report(&results, entries.len());
    let port_bugs: Vec<String> = results
        .iter()
        .filter(|r| r.classes.contains(&"port-bug"))
        .map(|r| format!("{} {}", r.id, r.format))
        .collect();
    eprintln!("parity: output report at {}", path.display());
    assert!(
        port_bugs.is_empty(),
        "{} port-bug outputs (see {}): {port_bugs:?}",
        port_bugs.len(),
        path.display()
    );
}

#[test]
fn metadata_matches_html5_fixture() {
    let Some((fixtures, upstream, index)) = load_corpus() else {
        return;
    };
    let max_date = index["max_date"].as_str().unwrap();
    let pages: Vec<MetaPage> = index["pages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| run_metadata_page(&upstream, &fixtures, entry, max_date))
        .collect();
    let stale = stale_known(&pages, METADATA_KNOWN);
    let path = write_metadata_report(&pages, &stale);
    let port_bugs: Vec<&str> = pages
        .iter()
        .filter(|p| p.classes.contains(&"port-bug"))
        .map(|p| p.id.as_str())
        .collect();
    eprintln!("parity: metadata report at {}", path.display());
    assert!(
        port_bugs.is_empty() && stale.is_empty(),
        "{} port-bug pages, {} stale entries (see {}): {port_bugs:?} {stale:?}",
        port_bugs.len(),
        stale.len(),
        path.display()
    );
}

/// Dropping the text of unread scripts and styles must not change any output: each corpus page
/// extracts the same with and without it under every focus and comments setting.
#[test]
#[ignore = "slow"]
fn script_text_drop_keeps_output() {
    let Some((_, upstream, index)) = load_corpus() else {
        return;
    };
    let mut differing = Vec::new();
    for entry in index["pages"].as_array().unwrap() {
        let id = entry["id"].as_str().unwrap();
        let bytes = std::fs::read(upstream.join(entry["source"].as_str().unwrap())).unwrap();
        for focus in [Focus::Precision, Focus::Balanced, Focus::Recall] {
            for comments in [true, false] {
                let extractor = Options::new()
                    .format(Format::Txt)
                    .focus(focus)
                    .comments(comments)
                    .build();
                let kept_extractor = Options::new()
                    .format(Format::Txt)
                    .focus(focus)
                    .comments(comments)
                    .keep_script_text(true)
                    .build();
                let page = Input::bytes(&bytes);
                let dropped = extractor.extract(&page, &Call::default());
                let kept = kept_extractor.extract(&page, &Call::default());
                if dropped != kept {
                    differing.push(format!("{id} {focus:?} comments={comments}"));
                }
            }
        }
    }
    assert!(
        differing.is_empty(),
        "{} outputs differ: {differing:?}",
        differing.len()
    );
}
