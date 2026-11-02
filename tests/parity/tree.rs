use super::*;

pub(crate) struct Page {
    pub(crate) id: String,
    pub(crate) classes: Vec<&'static str>,
    hunks: Vec<(Hunk, &'static str)>,
    expected: Option<String>,
    actual: Option<String>,
}

pub(crate) fn read_json(path: &Path) -> Value {
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
pub(crate) fn classify_page(
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
pub(crate) fn hunk_diff(
    expected: &[&str],
    actual: &[&str],
    hunks: &[(Hunk, &'static str)],
) -> String {
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
pub(crate) fn run_page(upstream: &Path, fixtures: &Path, entry: &Value) -> (Page, bool) {
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
pub(crate) fn shown_diff(
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
    // `str::lines` drops a final newline, so texts that differ only in it make no hunk.
    if all.is_empty() && final_newline_only(expected, actual) {
        return "differs only in the final newline\n".to_string();
    }
    hunk_diff(&left, &right, &all)
}

/// Whether two present texts differ and are equal once their trailing newlines are removed.
fn final_newline_only(expected: &Option<String>, actual: &Option<String>) -> bool {
    match (expected, actual) {
        (Some(expected), Some(actual)) => {
            expected != actual && expected.trim_end_matches('\n') == actual.trim_end_matches('\n')
        }
        _ => false,
    }
}

pub(crate) fn write_report(pages: &[Page], parser_count: usize) -> PathBuf {
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
