#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Parity of the crate's parsed tree with the html5 fixtures generated from upstream
//! (`docs/parity-harness.md`). Skips when the fixtures or the upstream checkout are missing.

#[allow(dead_code)]
mod common;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde_json::Value;
use trafilatura_rust::__internals::{DumpMode, HtmlInput, ParseLimits, dump, load_html};
use trafilatura_rust::{Call, ExtractError, Rejection};

/// Elements whose tree differs because html5lib (the fixture parser) predates a WHATWG change that
/// html5ever has: the 2025 "customizable select" parsing, where `select` and `option` keep phrasing
/// content such as `<span>` instead of dropping the tags, and the `search`, `hgroup` and
/// `selectedcontent` elements.
const PYTHON_PARSER_TAGS: [&str; 6] = [
    "search",
    "select",
    "option",
    "optgroup",
    "hgroup",
    "selectedcontent",
];

const CLASSES: [&str; 7] = [
    "match",
    "decompress",
    "encoding",
    "normalized",
    "python-parser",
    "port-bug",
    "upstream-error",
];

const DIFF_PAGES: usize = 20;
const DIFF_LINES: usize = 10;

struct Page {
    id: String,
    class: &'static str,
    expected: Option<String>,
    actual: Option<String>,
}

fn read_json(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn tree_of(variant: &Value) -> Option<String> {
    variant["tree"].as_str().map(str::to_string)
}

fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start().len()
}

/// The element a dump line is about: its own name, or for a text line the parent element's.
fn element_of<'a>(lines: &[&'a str], index: usize) -> Option<&'a str> {
    let line = *lines.get(index)?;
    let name = |line: &'a str| {
        line.trim_start()
            .split(' ')
            .next()
            .map(|n| n.split('@').next().unwrap_or(n))
    };
    if !line.trim_start().starts_with('#') {
        return name(line);
    }
    let depth = indent_of(line);
    lines
        .get(..index)?
        .iter()
        .rev()
        .find(|candidate| indent_of(candidate) < depth && !candidate.trim_start().starts_with('#'))
        .and_then(|parent| name(parent))
}

fn first_difference_names_python_parser_tag(expected: &str, actual: &str) -> bool {
    let left: Vec<&str> = expected.lines().collect();
    let right: Vec<&str> = actual.lines().collect();
    let Some(index) = (0..left.len().max(right.len())).find(|&i| left.get(i) != right.get(i))
    else {
        return false;
    };
    [element_of(&left, index), element_of(&right, index)]
        .into_iter()
        .flatten()
        .any(|name| PYTHON_PARSER_TAGS.contains(&name))
}

fn classify(
    source: &str,
    valid_utf8: bool,
    normalized: bool,
    expected: &Option<String>,
    actual: &Option<String>,
) -> &'static str {
    if expected == actual {
        return "match";
    }
    if source.ends_with(".gz") && cfg!(not(feature = "decompress")) {
        return "decompress";
    }
    if !valid_utf8 {
        return "encoding";
    }
    if normalized {
        return "normalized";
    }
    match (expected, actual) {
        (Some(e), Some(a)) if first_difference_names_python_parser_tag(e, a) => "python-parser",
        _ => "port-bug",
    }
}

/// The first `DIFF_LINES` lines that differ, position by position: `-` fixture, `+` crate.
fn diff(expected: &Option<String>, actual: &Option<String>) -> String {
    let shown = |tree: &Option<String>| tree.clone().unwrap_or_else(|| "<no tree>".to_string());
    let (expected, actual) = (shown(expected), shown(actual));
    let (mut left, mut right) = (expected.lines(), actual.lines());
    let mut out = String::new();
    let (mut number, mut count) = (0_usize, 0_usize);
    while count < DIFF_LINES {
        number += 1;
        let (a, b) = (left.next(), right.next());
        if a.is_none() && b.is_none() {
            break;
        }
        if a == b {
            continue;
        }
        count += 1;
        let _ = writeln!(out, "@@ line {number}");
        if let Some(a) = a {
            let _ = writeln!(out, "-{a}");
        }
        if let Some(b) = b {
            let _ = writeln!(out, "+{b}");
        }
    }
    out
}

/// Runs one fixture page; the flag is whether the lxml tree differs from the html5 tree.
fn run_page(upstream: &Path, fixtures: &Path, entry: &Value) -> (Page, bool) {
    let id = entry["id"].as_str().unwrap().to_string();
    let source = entry["source"].as_str().unwrap();
    let valid_utf8 = entry["valid_utf8"].as_bool().unwrap();
    let fixture = read_json(&fixtures.join("pages").join(format!("{id}.json")));
    let html5 = &fixture["html5"];
    let parser_differs = tree_of(&fixture["lxml"]) != tree_of(html5);
    if html5["errors"]["tree"].is_string() {
        let page = Page {
            id,
            class: "upstream-error",
            expected: None,
            actual: None,
        };
        return (page, parser_differs);
    }
    let bytes = std::fs::read(upstream.join(source)).unwrap();
    let loaded = load_html(
        HtmlInput::Bytes(&bytes),
        &ParseLimits::default(),
        &Call::default(),
    );
    let (actual, rust_normalized) = match loaded {
        Ok(tree) => {
            let (text, changed) = dump(&tree, DumpMode::Normalized);
            (Some(text), changed)
        }
        Err(ExtractError::Rejected(Rejection::EmptyTree)) => (None, false),
        Err(other) => panic!("{id}: unexpected error {other:?}"),
    };
    let expected = tree_of(html5);
    let normalized = fixture["normalized"].as_bool().unwrap() || rust_normalized;
    let class = classify(source, valid_utf8, normalized, &expected, &actual);
    let page = Page {
        id,
        class,
        expected,
        actual,
    };
    (page, parser_differs)
}

fn write_report(pages: &[Page], parser_count: usize) -> PathBuf {
    let mut out = String::from("# Parity report: tree dump against the html5 fixtures\n\n");
    let _ = writeln!(out, "Pages: {}\n", pages.len());
    out.push_str("## Classes\n\n");
    for class in CLASSES {
        let count = pages.iter().filter(|p| p.class == class).count();
        let _ = writeln!(out, "- {class}: {count}");
    }
    let _ = writeln!(
        out,
        "\nparser (D-2, lxml tree differs from html5 tree): {parser_count}\n"
    );
    for class in CLASSES.iter().filter(|c| **c != "match") {
        let mut ids = pages.iter().filter(|p| p.class == *class).peekable();
        if ids.peek().is_none() {
            continue;
        }
        let _ = writeln!(out, "## Pages: {class}\n");
        for page in ids {
            let _ = writeln!(out, "{}", page.id);
        }
        out.push('\n');
    }
    out.push_str("## Diffs (first pages that do not match)\n\n");
    let mismatches = pages
        .iter()
        .filter(|p| p.class != "match" && p.class != "upstream-error");
    for page in mismatches.take(DIFF_PAGES) {
        let _ = writeln!(
            out,
            "### {} ({})\n\n```diff\n{}```\n",
            page.id,
            page.class,
            diff(&page.expected, &page.actual)
        );
    }
    let dir = common::report_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("report.md");
    std::fs::write(&path, out).unwrap();
    path
}

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

#[test]
fn gate_skips_only_when_fixtures_are_not_required() {
    assert_eq!(gate(false, false, None), Ok(false));
    assert!(gate(true, false, None).is_err());
    assert!(gate(true, true, Some(0)).is_err());
    assert_eq!(gate(false, true, Some(0)), Ok(true));
    assert_eq!(gate(true, true, Some(3)), Ok(true));
}

#[test]
fn tree_dump_matches_html5_fixture() {
    let require = std::env::var_os("PARITY_REQUIRE_FIXTURES").is_some_and(|v| !v.is_empty());
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
            return;
        }
        Ok(true) => {}
    }
    let (Some((fixtures, upstream)), Some(index)) = (found, index) else {
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
        .filter(|p| p.class == "port-bug")
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
