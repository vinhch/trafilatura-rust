use super::*;

pub(crate) struct Page {
    pub(crate) id: String,
    pub(crate) classes: Vec<&'static str>,
    pub(crate) valid_utf8: bool,
    hunks: Vec<(Hunk, &'static str)>,
    expected: Option<String>,
    actual: Option<String>,
}

pub(crate) fn read_json(path: &Path) -> Value {
    let text = std::fs::read_to_string(path).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn tree_of(fixture: &Value) -> Option<String> {
    fixture["tree"].as_str().map(str::to_string)
}

/// The classes of a page (sorted, unique) and its classified hunks. `match`, `decompress` and a dump
/// that is `None` on one side (an empty tree against a tree) are whole-page; every hunk of a dump that
/// differs is a `port-bug`.
pub(crate) fn classify_page(
    source: &str,
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
        .map(|hunk| (hunk, "port-bug"))
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

/// The classified page of the lxml fixture: `upstream-error` when the fixture records an error for the
/// tree, else `actual` (the crate's dump, `None` for an empty tree) against its dump.
pub(crate) fn tree_page(
    id: String,
    source: &str,
    valid_utf8: bool,
    fixture: &Value,
    actual: &Option<String>,
) -> Page {
    let fixture = &fixture["lxml"];
    if fixture["errors"]["tree"].is_string() {
        return Page {
            id,
            classes: vec!["upstream-error"],
            valid_utf8,
            hunks: vec![],
            expected: None,
            actual: None,
        };
    }
    let expected = tree_of(fixture);
    let (classes, hunks) = classify_page(source, &expected, actual);
    Page {
        id,
        classes,
        valid_utf8,
        hunks,
        expected,
        actual: actual.clone(),
    }
}

/// Runs one fixture page: the crate's tree dump against the lxml fixture. A page whose fixture records
/// an upstream error is not parsed.
pub(crate) fn run_page(upstream: &Path, fixtures: &Path, entry: &Value) -> Page {
    let id = entry["id"].as_str().unwrap().to_string();
    let source = entry["source"].as_str().unwrap();
    let valid_utf8 = entry["valid_utf8"].as_bool().unwrap();
    let fixture = read_json(&fixtures.join("pages").join(format!("{id}.json")));
    if fixture["lxml"]["errors"]["tree"].is_string() {
        return tree_page(id, source, valid_utf8, &fixture, &None);
    }
    let bytes = std::fs::read(upstream.join(source)).unwrap();
    let loaded = load_html(
        HtmlInput::Bytes(&bytes),
        MAX_FILE_SIZE,
        &ParseLimits::default(),
        &Call::default(),
    );
    let actual = match &loaded {
        Ok(tree) => Some(dump(tree)),
        Err(ExtractError::Rejected(Rejection::EmptyTree)) => None,
        Err(other) => panic!("{id}: unexpected error {other:?}"),
    };
    tree_page(id, source, valid_utf8, &fixture, &actual)
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

pub(crate) fn write_report(pages: &[Page]) -> PathBuf {
    let classes = &TREE_CLASSES;
    let mut out = "# Parity report: tree dump against the lxml fixtures\n\n".to_string();
    let port_bugs = pages.iter().filter(|p| p.classes.contains(&"port-bug"));
    out.push_str(&gate_count_line("", port_bugs.map(|p| p.valid_utf8)));
    out.push('\n');
    let _ = writeln!(out, "Pages: {}\n", pages.len());
    out.push_str("## Classes\n\n");
    for &class in classes {
        let count = pages.iter().filter(|p| p.classes.contains(&class)).count();
        let hunk_count: usize = pages
            .iter()
            .map(|p| p.hunks.iter().filter(|(_, c)| *c == class).count())
            .sum();
        let _ = writeln!(out, "- {class}: {count} pages, {hunk_count} hunks");
    }
    out.push('\n');
    for class in classes.iter().filter(|c| **c != "match") {
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
