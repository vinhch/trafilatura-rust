#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Parity of the crate's parsed tree, output and metadata with the lxml fixtures generated from upstream
//! (`docs/parity-harness.md`). Skips when the fixtures or the upstream checkout are missing.

#[allow(dead_code)]
#[path = "../common/mod.rs"]
mod common;
#[path = "../hunks/mod.rs"]
mod hunks;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use blake2::digest::consts::U16;
use blake2::{Blake2b, Digest};
use serde_json::{Map, Value, json};
use trafilatura_rust::__internals::{
    HtmlInput, MAX_FILE_SIZE, ParseLimits, decode_file, detect_encoding, dump, load_html,
};
use trafilatura_rust::{Call, Document, ExtractError, Focus, Format, Input, Options, Rejection};

use encoding_cases::CORPUS_ENCODING_CASES;
use hunks::{Hunk, is_encoding_hunk, is_encoding_text_hunk};

mod encoding_cases;
mod metadata;
mod output;
mod self_tests;
mod tree;

use metadata::*;
use output::*;
use tree::*;

/// Classes of a tree report.
const TREE_CLASSES: [&str; 5] = [
    "match",
    "decompress",
    "encoding",
    "port-bug",
    "upstream-error",
];

/// Classes of an output report.
const OUTPUT_CLASSES: [&str; 6] = [
    "match",
    "decompress",
    "encoding",
    "known-metadata",
    "port-bug",
    "upstream-error",
];

/// The line of an lxml report that gives the Phase 10 gate counts: `rows` holds one `valid_utf8`
/// flag per page (or output) with a `port-bug` class. `scope` names a format, or is empty.
fn gate_count_line(scope: &str, rows: impl Iterator<Item = bool>) -> String {
    let (valid, all) = rows.fold((0, 0), |(valid, all), valid_utf8| {
        (valid + usize::from(valid_utf8), all + 1)
    });
    format!("Gate count (port-bug pages){scope}: valid-UTF-8 {valid}, all {all}\n")
}

const OUTPUT_FORMATS: [(&str, Format); 6] = [
    ("txt", Format::Txt),
    ("markdown", Format::Markdown),
    ("html", Format::Html),
    ("xml", Format::Xml),
    ("xmltei", Format::XmlTei),
    ("json", Format::Json),
];

/// Pages whose difference from lxml in one format is accepted: (page id, format or `tree`/`metadata`,
/// deviation id). The page keeps its `port-bug` hunks in the report and is classed `accepted`. None
/// today: every valid-UTF-8 page matches lxml in every format.
const LXML_ACCEPTED: &[(&str, &str, &str)] = &[];

/// Most accepted pages one format may carry: 1% of the valid-UTF-8 pages (stop rule a).
const LXML_CAP: usize = 10;

/// Relabels the `port-bug` class of page `id` in `format` as `accepted` when `accepted` lists the page
/// for that format. A listed page with no `port-bug` keeps its classes, so `accepted_failures` sees it
/// as stale.
fn accept(accepted: &[(&str, &str, &str)], format: &str, id: &str, classes: &mut [&'static str]) {
    if accepted
        .iter()
        .any(|(page, listed, _)| *page == id && *listed == format)
    {
        for class in classes.iter_mut().filter(|class| **class == "port-bug") {
            *class = "accepted";
        }
    }
}

/// The failures of the `accepted` list for `format`: more entries than `cap`, and an entry whose page
/// is not in `accepted_ids` (classed `accepted` after `accept`), so it no longer differs in `format`.
fn accepted_failures(
    accepted: &[(&str, &str, &str)],
    format: &str,
    accepted_ids: &[&str],
    cap: usize,
) -> Vec<String> {
    let entries: Vec<&str> = accepted
        .iter()
        .filter(|(_, listed, _)| *listed == format)
        .map(|(id, _, _)| *id)
        .collect();
    let mut failures = Vec::new();
    if entries.len() > cap {
        failures.push(format!(
            "{format}: {} accepted pages, the cap is {cap}",
            entries.len()
        ));
    }
    failures.extend(
        entries
            .iter()
            .filter(|id| !accepted_ids.contains(*id))
            .map(|id| format!("{format}: stale accepted entry {id}")),
    );
    failures
}

/// The ids of the pages classed `accepted`.
fn accepted_ids<'a>(pages: impl Iterator<Item = (&'a str, &'a [&'static str])>) -> Vec<&'a str> {
    pages
        .filter(|(_, classes)| classes.contains(&"accepted"))
        .map(|(id, _)| id)
        .collect()
}

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

/// Every tree against lxml's `fromstring`.
#[test]
fn tree_dump_matches_lxml_fixture() {
    let Some((fixtures, upstream, index)) = load_corpus() else {
        return;
    };
    let mut pages: Vec<Page> = index["pages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| run_page(&upstream, &fixtures, entry))
        .collect();
    let path = write_report(&pages);
    eprintln!("parity: report at {}", path.display());
    for page in &mut pages {
        accept(LXML_ACCEPTED, "tree", &page.id, &mut page.classes);
    }
    let accepted = accepted_ids(pages.iter().map(|p| (p.id.as_str(), p.classes.as_slice())));
    let failures = accepted_failures(LXML_ACCEPTED, "tree", &accepted, LXML_CAP);
    let port_bugs: Vec<&str> = pages
        .iter()
        .filter(|p| p.valid_utf8 && p.classes.contains(&"port-bug"))
        .map(|p| p.id.as_str())
        .collect();
    assert!(
        port_bugs.is_empty() && failures.is_empty(),
        "{} valid-UTF-8 port-bug pages (see {}): {port_bugs:?}; {failures:?}",
        port_bugs.len(),
        path.display()
    );
}

#[test]
fn outputs_match_lxml_fixture() {
    let Some((fixtures, upstream, index)) = load_corpus() else {
        return;
    };
    let entries = index["pages"].as_array().unwrap();
    let max_date = index["max_date"].as_str().unwrap();
    let mut results: Vec<OutputResult> = entries
        .iter()
        .flat_map(|entry| run_output_page(&upstream, &fixtures, entry, max_date))
        .collect();
    let path = write_output_report(&results, entries.len());
    eprintln!("parity: output report at {}", path.display());
    for result in &mut results {
        accept(
            LXML_ACCEPTED,
            result.format,
            &result.id,
            &mut result.classes,
        );
    }
    let port_bugs: Vec<String> = results
        .iter()
        .filter(|r| r.valid_utf8 && r.classes.contains(&"port-bug"))
        .map(|r| format!("{} {}", r.id, r.format))
        .collect();
    let mut failures = Vec::new();
    for (format, _) in OUTPUT_FORMATS {
        let accepted = accepted_ids(
            results
                .iter()
                .filter(|r| r.format == format)
                .map(|r| (r.id.as_str(), r.classes.as_slice())),
        );
        failures.extend(accepted_failures(
            LXML_ACCEPTED,
            format,
            &accepted,
            LXML_CAP,
        ));
    }
    assert!(
        port_bugs.is_empty() && failures.is_empty(),
        "{} valid-UTF-8 port-bug outputs (see {}): {port_bugs:?}; {failures:?}",
        port_bugs.len(),
        path.display()
    );
}

#[test]
fn metadata_matches_lxml_fixture() {
    let Some((fixtures, upstream, index)) = load_corpus() else {
        return;
    };
    let max_date = index["max_date"].as_str().unwrap();
    let mut pages: Vec<MetaPage> = index["pages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| run_metadata_page(&upstream, &fixtures, entry, max_date))
        .collect();
    let stale = stale_known(&pages, METADATA_KNOWN);
    let path = write_metadata_report(&pages, &stale);
    eprintln!("parity: metadata report at {}", path.display());
    for page in &mut pages {
        accept(LXML_ACCEPTED, "metadata", &page.id, &mut page.classes);
    }
    let accepted = accepted_ids(pages.iter().map(|p| (p.id.as_str(), p.classes.as_slice())));
    let failures = accepted_failures(LXML_ACCEPTED, "metadata", &accepted, LXML_CAP);
    let port_bugs: Vec<&str> = pages
        .iter()
        .filter(|p| p.valid_utf8 && p.classes.contains(&"port-bug"))
        .map(|p| p.id.as_str())
        .collect();
    assert!(
        port_bugs.is_empty() && stale.is_empty() && failures.is_empty(),
        "{} valid-UTF-8 port-bug pages, {} stale entries (see {}): {port_bugs:?} {stale:?} {failures:?}",
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

/// `detect_encoding` and `decode_file` on every non-UTF-8 corpus page give what charset_normalizer gives
/// upstream: the ranked guesses, and the decoded text by length and digest.
#[test]
fn corpus_encoding_matches_charset_normalizer() {
    let Some((_, upstream, index)) = load_corpus() else {
        return;
    };
    let non_utf8: Vec<(&str, &str)> = index["pages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["valid_utf8"] == Value::Bool(false))
        .map(|entry| {
            (
                entry["id"].as_str().unwrap(),
                entry["source"].as_str().unwrap(),
            )
        })
        .collect();
    let mut index_ids: Vec<&str> = non_utf8.iter().map(|(id, _)| *id).collect();
    let mut row_ids: Vec<&str> = CORPUS_ENCODING_CASES.iter().map(|row| row.id).collect();
    index_ids.sort_unstable();
    row_ids.sort_unstable();
    assert_eq!(
        row_ids, index_ids,
        "the encoding table and the fixtures list different non-UTF-8 pages"
    );
    let mut failures = Vec::new();
    for row in CORPUS_ENCODING_CASES {
        let source = non_utf8
            .iter()
            .find_map(|(id, source)| (*id == row.id).then_some(*source))
            .unwrap();
        let bytes = std::fs::read(upstream.join(source)).unwrap();
        let guesses = detect_encoding(&bytes);
        let decoded = decode_file(&bytes, MAX_FILE_SIZE);
        let digest: [u8; 16] = Blake2b::<U16>::digest(decoded.as_bytes()).into();
        if guesses != row.guesses {
            failures.push(format!(
                "{}: guesses {guesses:?}, oracle {:?}",
                row.id, row.guesses
            ));
        }
        if decoded.chars().count() != row.decoded_chars || digest != row.decoded_blake2b {
            failures.push(format!("{}: decoded text differs", row.id));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}
