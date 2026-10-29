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
mod hunks;

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use regex::Regex;
use serde_json::Value;
use trafilatura_rust::__internals::{
    DumpMode, HtmlInput, MAX_FILE_SIZE, ParseLimits, dump, load_html,
};
use trafilatura_rust::{Call, ExtractError, Focus, Format, Input, Options, Rejection};

use hunks::{PYTHON_PARSER_TAGS, indent_of};

const CLASSES: [&str; 7] = [
    "match",
    "decompress",
    "encoding",
    "normalized",
    "python-parser",
    "port-bug",
    "upstream-error",
];

const OUTPUT_CLASSES: [&str; 6] = [
    "match",
    "decompress",
    "encoding",
    "normalized",
    "port-bug",
    "upstream-error",
];

const OUTPUT_FORMATS: [(&str, Format); 3] = [
    ("txt", Format::Txt),
    ("markdown", Format::Markdown),
    ("html", Format::Html),
];

/// Pages the crate rejects with `InvalidValue` while upstream extracted text, because the oracle's
/// normalized tree dropped an element whose name is not an XML Name (D-2): (page id, reason). None
/// today.
const NORMALIZED_REJECTS: &[(&str, &str)] = &[];

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
    let shown = |text: &Option<String>| text.clone().unwrap_or_else(|| "<none>".to_string());
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
        MAX_FILE_SIZE,
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
fn gate_skips_only_when_fixtures_are_not_required() {
    assert_eq!(gate(false, false, None), Ok(false));
    assert!(gate(true, false, None).is_err());
    assert!(gate(true, true, Some(0)).is_err());
    assert_eq!(gate(false, true, Some(0)), Ok(true));
    assert_eq!(gate(true, true, Some(3)), Ok(true));
}

static NS_DECLARATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#" xmlns:ns\d+="[^"]*""#).unwrap());
static NS_TAG_PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<(/?)ns\d+:").unwrap());

/// Removes the `nsN:` prefixes lxml writes for foreign elements of the oracle's html5 tree, as
/// `rewrite_namespaces` in `tools/parity/parity/gen_xml_output_cases.py` does (D-2): the crate writes
/// such an element by its local name.
fn strip_lxml_namespaces(expected: &str) -> String {
    let without_declarations = NS_DECLARATION.replace_all(expected, "");
    NS_TAG_PREFIX
        .replace_all(&without_declarations, "<$1")
        .into_owned()
}

/// One page and format of the output check.
struct OutputCase<'a> {
    id: &'a str,
    source: &'a str,
    valid_utf8: bool,
    expected: Option<&'a str>,
    upstream_error: bool,
    actual: &'a Result<String, ExtractError>,
}

/// Class of one output, in order: match, decompress, encoding, upstream-error, normalized (a page in
/// `normalized_rejects` only), port-bug.
fn classify_output(case: &OutputCase, normalized_rejects: &[(&str, &str)]) -> &'static str {
    let rejected = |rejection: Option<Rejection>| match case.actual {
        Err(ExtractError::Rejected(found)) => rejection.is_none_or(|want| want == *found),
        _ => false,
    };
    match (case.expected, case.actual) {
        (Some(expected), Ok(actual)) if expected == actual => return "match",
        (None, Err(ExtractError::Rejected(_))) if !case.upstream_error => return "match",
        _ => {}
    }
    if case.source.ends_with(".gz") && cfg!(not(feature = "decompress")) {
        return "decompress";
    }
    if !case.valid_utf8 {
        return "encoding";
    }
    if case.upstream_error {
        return if rejected(Some(Rejection::InvalidValue)) {
            "upstream-error"
        } else {
            "port-bug"
        };
    }
    let listed = normalized_rejects.iter().any(|(id, _)| *id == case.id);
    if listed && case.expected.is_some() && rejected(Some(Rejection::InvalidValue)) {
        return "normalized";
    }
    "port-bug"
}

struct OutputResult {
    id: String,
    format: &'static str,
    class: &'static str,
    expected: Option<String>,
    actual: Option<String>,
}

fn run_output_page(upstream: &Path, fixtures: &Path, entry: &Value) -> Vec<OutputResult> {
    let id = entry["id"].as_str().unwrap();
    let source = entry["source"].as_str().unwrap();
    let valid_utf8 = entry["valid_utf8"].as_bool().unwrap();
    let fixture = read_json(&fixtures.join("pages").join(format!("{id}.json")));
    let bytes = std::fs::read(upstream.join(source)).unwrap();
    OUTPUT_FORMATS
        .iter()
        .map(|&(name, format)| {
            let expected = fixture["html5"]["formats"][name]
                .as_str()
                .map(|text| match name {
                    "html" => {
                        let stripped = strip_lxml_namespaces(text);
                        assert!(
                            !stripped.contains(" xmlns:ns") && !NS_TAG_PREFIX.is_match(&stripped),
                            "{id}: namespace prefixes left in the expected html"
                        );
                        stripped
                    }
                    _ => text.to_string(),
                });
            let actual = Options::new()
                .format(format)
                .build()
                .extract(&Input::bytes(&bytes), &Call::default());
            let class = classify_output(
                &OutputCase {
                    id,
                    source,
                    valid_utf8,
                    expected: expected.as_deref(),
                    upstream_error: fixture["html5"]["errors"][name].is_string(),
                    actual: &actual,
                },
                NORMALIZED_REJECTS,
            );
            OutputResult {
                id: id.to_string(),
                format: name,
                class,
                expected,
                actual: actual.ok(),
            }
        })
        .collect()
}

/// Indices of the outputs worth a diff: `port-bug` and `normalized` first, then the other classes
/// that differ, each group in page order.
fn diff_order(classes: &[&str]) -> Vec<usize> {
    let (urgent, rest): (Vec<_>, Vec<_>) = classes
        .iter()
        .enumerate()
        .filter(|(_, class)| !matches!(**class, "match" | "upstream-error"))
        .partition(|(_, class)| matches!(**class, "port-bug" | "normalized"));
    urgent.into_iter().chain(rest).map(|(i, _)| i).collect()
}

fn write_output_report(results: &[OutputResult], pages: usize) -> PathBuf {
    let mut out = String::from("# Parity report: output against the html5 fixtures\n\n");
    let _ = writeln!(out, "Pages: {pages}\n");
    out.push_str("## Classes\n\n");
    for (name, _) in OUTPUT_FORMATS {
        let counts: Vec<String> = OUTPUT_CLASSES
            .iter()
            .map(|class| {
                let count = results
                    .iter()
                    .filter(|r| r.format == name && r.class == *class)
                    .count();
                format!("{class} {count}")
            })
            .collect();
        let _ = writeln!(out, "- {name}: {}", counts.join(", "));
    }
    for class in OUTPUT_CLASSES.iter().filter(|c| **c != "match") {
        let mut rows = results.iter().filter(|r| r.class == *class).peekable();
        if rows.peek().is_none() {
            continue;
        }
        let _ = writeln!(out, "\n## Pages: {class}\n");
        for row in rows {
            let reason = NORMALIZED_REJECTS
                .iter()
                .find(|(id, _)| *id == row.id && *class == "normalized")
                .map_or(String::new(), |(_, reason)| format!(": {reason}"));
            let _ = writeln!(out, "{} {}{reason}", row.id, row.format);
        }
    }
    out.push_str("\n## Diffs (first outputs that do not match)\n\n");
    let classes: Vec<&str> = results.iter().map(|r| r.class).collect();
    for row in diff_order(&classes)
        .into_iter()
        .take(DIFF_PAGES)
        .map(|i| &results[i])
    {
        let _ = writeln!(
            out,
            "### {} {} ({})\n\n```diff\n{}```\n",
            row.id,
            row.format,
            row.class,
            diff(&row.expected, &row.actual)
        );
    }
    let dir = common::report_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("output-report.md");
    std::fs::write(&path, out).unwrap();
    path
}

#[test]
fn strip_lxml_namespaces_removes_only_ns_prefixes() {
    assert_eq!(
        strip_lxml_namespaces(
            r#"<p><ns0:svg xmlns:ns0="http://www.w3.org/2000/svg"><ns0:path/></ns0:svg></p>"#
        ),
        "<p><svg><path/></svg></p>"
    );
    assert_eq!(
        strip_lxml_namespaces("<ns12:g xmlns:ns12=\"u\"></ns12:g>"),
        "<g></g>"
    );
    assert_eq!(
        strip_lxml_namespaces("<svg:g>x</svg:g>"),
        "<svg:g>x</svg:g>"
    );
    assert_eq!(strip_lxml_namespaces("<p>ns0:x</p>"), "<p>ns0:x</p>");
}

#[test]
fn output_report_lists_port_bugs_first() {
    let mut classes = vec!["encoding"; 25];
    classes.push("port-bug");
    classes.insert(3, "match");
    let order = diff_order(&classes);
    let port_bug = classes.len() - 1;
    assert_eq!(order[0], port_bug);
    assert!(order[..DIFF_PAGES].contains(&port_bug));
    assert_eq!(order.len(), 26);
    assert_eq!(order[1..4], [0, 1, 2]);
}

#[test]
fn classify_output_names_each_class() {
    let rejected = |r| Err(ExtractError::Rejected(r));
    let text: Result<String, ExtractError> = Ok("a".to_string());
    let other: Result<String, ExtractError> = Ok("b".to_string());
    let too_short = rejected(Rejection::TooShort);
    let invalid = rejected(Rejection::InvalidValue);
    let case = |expected, upstream_error, actual| OutputCase {
        id: "page",
        source: "page.html",
        valid_utf8: true,
        expected,
        upstream_error,
        actual,
    };
    let listed = [("page", "element name is not an XML Name")];
    let class = |case: &OutputCase| classify_output(case, &[]);

    assert_eq!(class(&case(Some("a"), false, &text)), "match");
    assert_eq!(class(&case(None, false, &too_short)), "match");
    assert_eq!(class(&case(None, false, &text)), "port-bug");
    assert_eq!(class(&case(Some("a"), false, &too_short)), "port-bug");
    assert_eq!(class(&case(Some("a"), false, &other)), "port-bug");

    let gz = OutputCase {
        source: "page.html.gz",
        ..case(Some("a"), false, &other)
    };
    let gz_class = if cfg!(feature = "decompress") {
        "port-bug"
    } else {
        "decompress"
    };
    assert_eq!(class(&gz), gz_class);
    let latin1 = OutputCase {
        valid_utf8: false,
        ..case(Some("a"), false, &other)
    };
    assert_eq!(class(&latin1), "encoding");

    assert_eq!(class(&case(None, true, &invalid)), "upstream-error");
    assert_eq!(class(&case(None, true, &too_short)), "port-bug");
    assert_eq!(class(&case(None, true, &text)), "port-bug");

    let dropped = case(Some("a"), false, &invalid);
    assert_eq!(classify_output(&dropped, &listed), "normalized");
    assert_eq!(classify_output(&dropped, &[]), "port-bug");
    let two_texts = case(Some("a"), false, &other);
    assert_eq!(classify_output(&two_texts, &listed), "port-bug");
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

#[test]
fn outputs_match_html5_fixture() {
    let Some((fixtures, upstream, index)) = load_corpus() else {
        return;
    };
    let entries = index["pages"].as_array().unwrap();
    let results: Vec<OutputResult> = entries
        .iter()
        .flat_map(|entry| run_output_page(&upstream, &fixtures, entry))
        .collect();
    let path = write_output_report(&results, entries.len());
    let port_bugs: Vec<String> = results
        .iter()
        .filter(|r| r.class == "port-bug")
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
                let page = Input::bytes(&bytes);
                let dropped = extractor.extract(&page, &Call::default());
                let kept = extractor
                    .with_script_text()
                    .extract(&page, &Call::default());
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
