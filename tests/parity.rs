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

use hunks::{
    Hunk, is_encoding_hunk, is_encoding_text_hunk, is_html5lib_cell_space_hunk,
    is_python_parser_hunk,
};

const CLASSES: [&str; 7] = [
    "match",
    "decompress",
    "encoding",
    "python-parser",
    "html5lib-cell-space",
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
    classes: Vec<&'static str>,
    hunks: Vec<(Hunk, &'static str)>,
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

/// Class of one hunk of the tree dump, first match wins: `encoding` (page not valid UTF-8),
/// `python-parser`, `html5lib-cell-space`, else `port-bug`.
fn classify_hunk(
    expected: &[&str],
    actual: &[&str],
    valid_utf8: bool,
    hunk: &Hunk,
) -> &'static str {
    if !valid_utf8 && is_encoding_hunk(expected, actual, hunk) {
        "encoding"
    } else if is_python_parser_hunk(expected, actual, hunk) {
        "python-parser"
    } else if is_html5lib_cell_space_hunk(expected, actual, hunk) {
        "html5lib-cell-space"
    } else {
        "port-bug"
    }
}

/// The classes of a page (sorted, unique) and its classified hunks. `match`, `decompress` and a dump
/// that is `None` on one side (an empty tree against a tree) are whole-page.
fn classify_page(
    source: &str,
    valid_utf8: bool,
    expected: &Option<String>,
    actual: &Option<String>,
) -> (Vec<&'static str>, Vec<(Hunk, &'static str)>) {
    if expected == actual {
        return (vec!["match"], vec![]);
    }
    if source.ends_with(".gz") && cfg!(not(feature = "decompress")) {
        return (vec!["decompress"], vec![]);
    }
    let (Some(expected), Some(actual)) = (expected, actual) else {
        return (vec!["port-bug"], vec![]);
    };
    let (left, right): (Vec<&str>, Vec<&str>) =
        (expected.lines().collect(), actual.lines().collect());
    let classified: Vec<(Hunk, &'static str)> = hunks::hunks(&left, &right)
        .into_iter()
        .map(|hunk| {
            let class = classify_hunk(&left, &right, valid_utf8, &hunk);
            (hunk, class)
        })
        .collect();
    // Texts that differ only in the final newline make no line hunk and are still a difference.
    if classified.is_empty() {
        return (vec!["port-bug"], vec![]);
    }
    let classes: std::collections::BTreeSet<&'static str> =
        classified.iter().map(|(_, class)| *class).collect();
    (classes.into_iter().collect(), classified)
}

/// Lines of one side of a hunk shown in the report.
const HUNK_LINES: usize = 20;

/// The first `DIFF_LINES` hunks, `port-bug` first and then in order: a `@@ -start,len +start,len
/// (class)` header, the removed fixture lines (`-`), then the inserted crate lines (`+`).
fn hunk_diff(expected: &[&str], actual: &[&str], hunks: &[(Hunk, &'static str)]) -> String {
    let (urgent, rest): (Vec<_>, Vec<_>) =
        hunks.iter().partition(|(_, class)| *class == "port-bug");
    let mut out = String::new();
    for (hunk, class) in urgent.into_iter().chain(rest).take(DIFF_LINES) {
        let (e, a) = (&hunk.expected, &hunk.actual);
        let _ = writeln!(
            out,
            "@@ -{},{} +{},{} ({class})",
            e.start + 1,
            e.len(),
            a.start + 1,
            a.len()
        );
        for (sign, lines, range) in [('-', expected, e), ('+', actual, a)] {
            let shown = lines.get(range.clone()).unwrap_or_default();
            for line in shown.iter().take(HUNK_LINES) {
                let _ = writeln!(out, "{sign}{line}");
            }
            if shown.len() > HUNK_LINES {
                let _ = writeln!(out, "{sign}... {} more lines", shown.len() - HUNK_LINES);
            }
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
            classes: vec!["upstream-error"],
            hunks: vec![],
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
    let actual = match loaded {
        Ok(tree) => Some(dump(&tree, DumpMode::Normalized).0),
        Err(ExtractError::Rejected(Rejection::EmptyTree)) => None,
        Err(other) => panic!("{id}: unexpected error {other:?}"),
    };
    let expected = tree_of(html5);
    let (classes, hunks) = classify_page(source, valid_utf8, &expected, &actual);
    let page = Page {
        id,
        classes,
        hunks,
        expected,
        actual,
    };
    (page, parser_differs)
}

/// The hunks to print for a page or output that differs: its classified hunks, or for one classified
/// whole (a side that is `None`) every line as one `port-bug` hunk.
fn shown_diff(
    expected: &Option<String>,
    actual: &Option<String>,
    classified: &[(Hunk, &'static str)],
) -> String {
    let lines = |text: &Option<String>| -> Vec<String> {
        text.as_deref()
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    };
    let (left, right) = (lines(expected), lines(actual));
    let (left, right): (Vec<&str>, Vec<&str>) = (
        left.iter().map(String::as_str).collect(),
        right.iter().map(String::as_str).collect(),
    );
    if !classified.is_empty() {
        return hunk_diff(&left, &right, classified);
    }
    let all: Vec<(Hunk, &'static str)> = hunks::hunks(&left, &right)
        .into_iter()
        .map(|hunk| (hunk, "port-bug"))
        .collect();
    hunk_diff(&left, &right, &all)
}

fn write_report(pages: &[Page], parser_count: usize) -> PathBuf {
    let mut out = String::from("# Parity report: tree dump against the html5 fixtures\n\n");
    let _ = writeln!(out, "Pages: {}\n", pages.len());
    out.push_str("## Classes\n\n");
    for class in CLASSES {
        let count = pages.iter().filter(|p| p.classes.contains(&class)).count();
        let hunk_count: usize = pages
            .iter()
            .map(|p| p.hunks.iter().filter(|(_, c)| *c == class).count())
            .sum();
        let _ = writeln!(out, "- {class}: {count} pages, {hunk_count} hunks");
    }
    let _ = writeln!(
        out,
        "\nparser (D-2, lxml tree differs from html5 tree): {parser_count}\n"
    );
    for class in CLASSES.iter().filter(|c| **c != "match") {
        let mut ids = pages
            .iter()
            .filter(|p| p.classes.contains(class))
            .peekable();
        if ids.peek().is_none() {
            continue;
        }
        let _ = writeln!(out, "## Pages: {class}\n");
        for page in ids {
            let _ = writeln!(out, "{}", page.id);
        }
        out.push('\n');
    }
    out.push_str("## Diffs (pages with a port-bug hunk first, then the others)\n\n");
    let (urgent, rest): (Vec<&Page>, Vec<&Page>) = pages
        .iter()
        .filter(|p| p.classes != ["match"] && p.classes != ["upstream-error"])
        .partition(|p| p.classes.contains(&"port-bug"));
    for page in urgent.into_iter().chain(rest).take(DIFF_PAGES) {
        let _ = writeln!(
            out,
            "### {} ({})\n\n```diff\n{}```\n",
            page.id,
            page.classes.join(", "),
            shown_diff(&page.expected, &page.actual, &page.hunks)
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

/// Classes of one output and its classified hunks, first match wins: `match`, `decompress`,
/// `upstream-error`, `normalized` (a page in `normalized_rejects` only, whole output), then, when both
/// sides have text, per hunk `encoding` (page not valid UTF-8) or `port-bug`. Text on one side only is
/// a whole-output `port-bug`.
fn classify_output(
    case: &OutputCase,
    normalized_rejects: &[(&str, &str)],
) -> (Vec<&'static str>, Vec<(Hunk, &'static str)>) {
    let whole = |class: &'static str| (vec![class], vec![]);
    let rejected = |rejection: Option<Rejection>| match case.actual {
        Err(ExtractError::Rejected(found)) => rejection.is_none_or(|want| want == *found),
        _ => false,
    };
    match (case.expected, case.actual) {
        (Some(expected), Ok(actual)) if expected == actual => return whole("match"),
        (None, Err(ExtractError::Rejected(_))) if !case.upstream_error => return whole("match"),
        _ => {}
    }
    if case.source.ends_with(".gz") && cfg!(not(feature = "decompress")) {
        return whole("decompress");
    }
    if case.upstream_error {
        return whole(if rejected(Some(Rejection::InvalidValue)) {
            "upstream-error"
        } else {
            "port-bug"
        });
    }
    let listed = normalized_rejects.iter().any(|(id, _)| *id == case.id);
    if listed && case.expected.is_some() && rejected(Some(Rejection::InvalidValue)) {
        return whole("normalized");
    }
    let (Some(expected), Ok(actual)) = (case.expected, case.actual) else {
        return whole("port-bug");
    };
    let (left, right): (Vec<&str>, Vec<&str>) =
        (expected.lines().collect(), actual.lines().collect());
    let classified: Vec<(Hunk, &'static str)> = hunks::hunks(&left, &right)
        .into_iter()
        .map(|hunk| {
            let encoding = !case.valid_utf8 && is_encoding_text_hunk(&left, &right, &hunk);
            (hunk, if encoding { "encoding" } else { "port-bug" })
        })
        .collect();
    // Texts that differ only in the final newline make no line hunk and are still a difference.
    if classified.is_empty() {
        return whole("port-bug");
    }
    let classes: std::collections::BTreeSet<&'static str> =
        classified.iter().map(|(_, class)| *class).collect();
    (classes.into_iter().collect(), classified)
}

struct OutputResult {
    id: String,
    format: &'static str,
    classes: Vec<&'static str>,
    hunks: Vec<(Hunk, &'static str)>,
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
            let (classes, hunks) = classify_output(
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
                classes,
                hunks,
                expected,
                actual: actual.ok(),
            }
        })
        .collect()
}

/// Indices of the outputs worth a diff: rows holding `port-bug` or `normalized` first, then the other
/// rows that differ (not `["match"]` or `["upstream-error"]`), each group in page order.
fn diff_order(rows: &[&[&'static str]]) -> Vec<usize> {
    let (urgent, rest): (Vec<_>, Vec<_>) = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| **row != ["match"] && **row != ["upstream-error"])
        .partition(|(_, row)| row.iter().any(|c| matches!(*c, "port-bug" | "normalized")));
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
                let mine = || results.iter().filter(|r| r.format == name);
                let count = mine().filter(|r| r.classes.contains(class)).count();
                let hunk_count: usize = mine()
                    .map(|r| r.hunks.iter().filter(|(_, c)| c == class).count())
                    .sum();
                format!("{class} {count} ({hunk_count} hunks)")
            })
            .collect();
        let _ = writeln!(out, "- {name}: {}", counts.join(", "));
    }
    for class in OUTPUT_CLASSES.iter().filter(|c| **c != "match") {
        let mut rows = results
            .iter()
            .filter(|r| r.classes.contains(class))
            .peekable();
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
    out.push_str("\n## Diffs (outputs with a port-bug hunk first, then the others)\n\n");
    let rows: Vec<&[&'static str]> = results.iter().map(|r| r.classes.as_slice()).collect();
    for row in diff_order(&rows)
        .into_iter()
        .take(DIFF_PAGES)
        .map(|i| &results[i])
    {
        let _ = writeln!(
            out,
            "### {} {} ({})\n\n```diff\n{}```\n",
            row.id,
            row.format,
            row.classes.join(", "),
            shown_diff(&row.expected, &row.actual, &row.hunks)
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
    let encoding: &[&str] = &["encoding"];
    let mut rows = vec![encoding; 25];
    rows.push(&["port-bug"]);
    rows.insert(3, &["match"]);
    let order = diff_order(&rows);
    let port_bug = rows.len() - 1;
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
    let class = |case: &OutputCase| classify_output(case, &[]).0;

    assert_eq!(class(&case(Some("a"), false, &text)), ["match"]);
    assert_eq!(class(&case(None, false, &too_short)), ["match"]);
    assert_eq!(class(&case(None, false, &text)), ["port-bug"]);
    assert_eq!(class(&case(Some("a"), false, &too_short)), ["port-bug"]);
    assert_eq!(class(&case(Some("a"), false, &other)), ["port-bug"]);

    let gz = OutputCase {
        source: "page.html.gz",
        ..case(Some("a"), false, &other)
    };
    let gz_class = if cfg!(feature = "decompress") {
        "port-bug"
    } else {
        "decompress"
    };
    assert_eq!(class(&gz), [gz_class]);

    let latin1 = |expected, actual| OutputCase {
        valid_utf8: false,
        ..case(expected, false, actual)
    };
    let decoded: Result<String, ExtractError> = Ok("bli¿ej".to_string());
    assert_eq!(class(&latin1(Some("bliżej"), &decoded)), ["encoding"]);
    let ellipsis: Result<String, ExtractError> = Ok("Idealen (…), hinter".to_string());
    assert_eq!(
        class(&latin1(Some("Idealen ( ), hinter"), &ellipsis)),
        ["encoding"]
    );
    assert_eq!(class(&latin1(Some("a"), &other)), ["port-bug"]);
    assert_eq!(class(&latin1(Some("a"), &too_short)), ["port-bug"]);

    assert_eq!(class(&case(None, true, &invalid)), ["upstream-error"]);
    assert_eq!(class(&case(None, true, &too_short)), ["port-bug"]);
    assert_eq!(class(&case(None, true, &text)), ["port-bug"]);

    let dropped = case(Some("a"), false, &invalid);
    assert_eq!(classify_output(&dropped, &listed).0, ["normalized"]);
    assert_eq!(classify_output(&dropped, &[]).0, ["port-bug"]);
    let two_texts = case(Some("a"), false, &other);
    assert_eq!(classify_output(&two_texts, &listed).0, ["port-bug"]);

    let (classes, hunks) =
        classify_output(&latin1(Some("a\nb\nc"), &Ok("a\nx\nc".to_string())), &[]);
    assert_eq!(classes, ["port-bug"]);
    assert_eq!(hunks.len(), 1);
}

#[test]
fn classify_page_none_side_is_port_bug() {
    let dump = Some("html\n".to_string());
    let page = |e: &Option<String>, a: &Option<String>| classify_page("p.html", true, e, a);
    assert_eq!(page(&dump, &None), (vec!["port-bug"], vec![]));
    assert_eq!(page(&None, &dump), (vec!["port-bug"], vec![]));
    assert_eq!(page(&None, &None), (vec!["match"], vec![]));
}

#[test]
fn classify_page_mixes_classes() {
    let expected = Some(
        "html\n  p\n    #text \"bliżej\"\n  q\n  select\n    option\n      #text \"1 Person\"\n"
            .to_string(),
    );
    let actual = Some(
        "html\n  p\n    #text \"bli¿ej\"\n  q\n  select\n    option\n      #text \"1 \"\n      span\n        #text \"Person\"\n"
            .to_string(),
    );
    let (classes, hunks) = classify_page("p.html", false, &expected, &actual);
    assert_eq!(classes, ["encoding", "python-parser"]);
    assert_eq!(hunks.len(), 2);
}

#[test]
fn classify_page_names_cell_space_hunk() {
    let expected = Some("html\n  td\n    p\n      #text \" was\"\n".to_string());
    let actual =
        Some("html\n  td\n    p\n      a\n        #text \" \"\n      #text \"was\"\n".to_string());
    let (classes, hunks) = classify_page("p.html", true, &expected, &actual);
    assert_eq!(classes, ["html5lib-cell-space"]);
    assert_eq!(hunks.len(), 1);
}

#[test]
fn classify_page_ascii_change_is_port_bug() {
    let expected = Some("html\n  #text \"a\"\n".to_string());
    let actual = Some("html\n  #text \"b\"\n".to_string());
    let (classes, hunks) = classify_page("p.html", true, &expected, &actual);
    assert_eq!(classes, ["port-bug"]);
    assert_eq!(hunks.len(), 1);
}

#[test]
fn hunk_diff_puts_port_bug_hunks_first() {
    let lines: Vec<String> = (0..31).map(|i| format!("line {i}")).collect();
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let classed: Vec<(Hunk, &'static str)> = (0..31)
        .map(|i| {
            let class = if i == 30 { "port-bug" } else { "encoding" };
            (
                Hunk {
                    expected: i..i + 1,
                    actual: i..i + 1,
                },
                class,
            )
        })
        .collect();
    let out = hunk_diff(&refs, &refs, &classed);
    assert!(
        out.starts_with("@@ -31,1 +31,1 (port-bug)\n-line 30\n+line 30\n"),
        "{out}"
    );
    assert_eq!(
        out.lines().filter(|l| l.starts_with("@@")).count(),
        DIFF_LINES
    );
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
    let results: Vec<OutputResult> = entries
        .iter()
        .flat_map(|entry| run_output_page(&upstream, &fixtures, entry))
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
