use super::*;
use std::ops::Range;

/// One page and format of the output check.
pub(crate) struct OutputCase<'a> {
    pub(crate) source: &'a str,
    pub(crate) expected: Option<&'a str>,
    pub(crate) upstream_error: bool,
    pub(crate) actual: &'a Result<String, ExtractError>,
    /// The fixture values of the fields `METADATA_KNOWN` lists for this page; empty for other pages.
    pub(crate) known_values: &'a [String],
}

/// The fixture values of the fields `METADATA_KNOWN` lists for page `id`, read from the page's
/// fixture metadata. A list-valued field (`tags`) contributes each item.
pub(crate) fn known_values(id: &str, metadata: &Value) -> Vec<String> {
    METADATA_KNOWN
        .iter()
        .filter(|(page, _, _, _)| *page == id)
        .flat_map(|(_, field, _, _)| match &metadata[*field] {
            Value::String(text) => vec![text.clone()],
            Value::Array(items) => items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect(),
            _ => vec![],
        })
        .filter(|value| !value.is_empty())
        .collect()
}

/// Classes of one output and its classified hunks, first match wins: `match`, `decompress`,
/// `upstream-error`, then, when both sides have text, per hunk `known-metadata` or `port-bug`.
/// Text on one side only is a whole-output `port-bug`.
pub(crate) fn classify_output(case: &OutputCase) -> (Vec<&'static str>, Vec<(Hunk, &'static str)>) {
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
    let (Some(expected), Ok(actual)) = (case.expected, case.actual) else {
        return whole("port-bug");
    };
    let (left, right): (Vec<&str>, Vec<&str>) =
        (expected.lines().collect(), actual.lines().collect());
    let classified: Vec<(Hunk, &'static str)> = hunks::hunks(&left, &right)
        .into_iter()
        .map(|hunk| {
            let class = if known_metadata_hunk(&left, &right, &hunk, case.known_values) {
                "known-metadata"
            } else {
                "port-bug"
            };
            (hunk, class)
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

/// The lines strictly between `<teiHeader>` and `</teiHeader>`, or `None` when the output has no
/// header.
fn header_content(lines: &[&str]) -> Option<Range<usize>> {
    let open = lines.iter().position(|line| line.trim() == "<teiHeader>")?;
    let close = open
        + lines
            .iter()
            .skip(open)
            .position(|line| line.trim() == "</teiHeader>")?;
    Some(open + 1..close)
}

fn within(range: &Range<usize>, span: &Range<usize>) -> bool {
    range.start >= span.start && range.end <= span.end
}

/// Whether a text segment of `line` (the run between `>` and `<`, trimmed) equals `value`, or one
/// of its `", "`-separated pieces does (`<bibl>Sitename, 2008-07-30</bibl>`).
pub(crate) fn mentions_value(line: &str, value: &str) -> bool {
    line.split('<')
        .enumerate()
        .filter_map(|(i, part)| match i {
            0 => Some(part),
            _ => part.split_once('>').map(|(_, text)| text),
        })
        .map(str::trim)
        .any(|segment| segment == value || segment.split(", ").any(|piece| piece.trim() == value))
}

/// Rule for the TEI header of a page `METADATA_KNOWN` lists: both ranges lie inside the `teiHeader`
/// of their side, and each line pair holds a fixture value of a listed field on one side. The text
/// part is outside the header, so a text difference is never this class.
// Oracle: the header of `xmltei` carries the metadata fields; the metadata check already pins them.
fn known_metadata_hunk(
    expected: &[&str],
    actual: &[&str],
    hunk: &Hunk,
    known_values: &[String],
) -> bool {
    let (Some(left_header), Some(right_header)) =
        (header_content(expected), header_content(actual))
    else {
        return false;
    };
    if !within(&hunk.expected, &left_header) || !within(&hunk.actual, &right_header) {
        return false;
    }
    let left = expected.get(hunk.expected.clone()).unwrap_or_default();
    let right = actual.get(hunk.actual.clone()).unwrap_or_default();
    // A value under four characters can fill a whole segment by chance (a language code), so it
    // cannot tell which line it explains; a longer one must fill a segment or one of its
    // `", "`-separated pieces.
    let mentions = |line: &str| {
        known_values
            .iter()
            .filter(|value| value.chars().count() >= 4)
            .any(|value| mentions_value(line, value))
    };
    !left.is_empty()
        && left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(a, b)| mentions(a) || mentions(b))
}

pub(crate) struct OutputResult {
    pub(crate) id: String,
    pub(crate) format: &'static str,
    pub(crate) classes: Vec<&'static str>,
    pub(crate) valid_utf8: bool,
    hunks: Vec<(Hunk, &'static str)>,
    expected: Option<String>,
    actual: Option<String>,
}

/// The expected text of format `name` in the lxml fixture of a page. libxml2 writes no `nsN:` prefix
/// for a foreign element, so the text is used as it is.
pub(crate) fn expected_output(name: &str, fixture: &Value) -> Option<String> {
    fixture["lxml"]["formats"][name]
        .as_str()
        .map(str::to_string)
}

/// Runs one fixture page: per format, one extraction classified against the lxml fixture.
pub(crate) fn run_output_page(
    upstream: &Path,
    fixtures: &Path,
    entry: &Value,
    max_date: &str,
) -> Vec<OutputResult> {
    let id = entry["id"].as_str().unwrap();
    let source = entry["source"].as_str().unwrap();
    let valid_utf8 = entry["valid_utf8"].as_bool().unwrap();
    let fixture = read_json(&fixtures.join("pages").join(format!("{id}.json")));
    let bytes = std::fs::read(upstream.join(source)).unwrap();
    // The TEI header writes `filedate` from `max_date`, so every format runs with the index's one.
    let call = Call::default().max_date(max_date);
    OUTPUT_FORMATS
        .iter()
        .map(|&(name, format)| {
            let actual = Options::new()
                .format(format)
                .build()
                .extract(&Input::bytes(&bytes), &call);
            let expected = expected_output(name, &fixture);
            let known = known_values(id, &fixture["lxml"]["metadata"]);
            let (classes, hunks) = classify_output(&OutputCase {
                source,
                expected: expected.as_deref(),
                upstream_error: fixture["lxml"]["errors"][name].is_string(),
                actual: &actual,
                known_values: &known,
            });
            OutputResult {
                id: id.to_string(),
                format: name,
                classes,
                valid_utf8,
                hunks,
                expected,
                actual: actual.as_ref().ok().cloned(),
            }
        })
        .collect()
}

/// Indices of the outputs worth a diff: rows holding `port-bug` first, then the other rows that differ
/// (not `["match"]` or `["upstream-error"]`), each group in page order.
pub(crate) fn diff_order(rows: &[&[&'static str]]) -> Vec<usize> {
    let (urgent, rest): (Vec<_>, Vec<_>) = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| **row != ["match"] && **row != ["upstream-error"])
        .partition(|(_, row)| row.contains(&"port-bug"));
    urgent.into_iter().chain(rest).map(|(i, _)| i).collect()
}

pub(crate) fn write_output_report(results: &[OutputResult], pages: usize) -> PathBuf {
    let classes = &OUTPUT_CLASSES;
    let mut out = "# Parity report: output against the lxml fixtures\n\n".to_string();
    for (name, _) in OUTPUT_FORMATS {
        let port_bugs = results
            .iter()
            .filter(|r| r.format == name && r.classes.contains(&"port-bug"));
        let scope = format!(", {name}");
        out.push_str(&gate_count_line(&scope, port_bugs.map(|r| r.valid_utf8)));
    }
    out.push('\n');
    let _ = writeln!(out, "Pages: {pages}\n");
    out.push_str("## Classes\n\n");
    for (name, _) in OUTPUT_FORMATS {
        let counts: Vec<String> = classes
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
    for class in classes.iter().filter(|c| **c != "match") {
        let mut rows = results
            .iter()
            .filter(|r| r.classes.contains(class))
            .peekable();
        if rows.peek().is_none() {
            continue;
        }
        let _ = writeln!(out, "\n## Pages: {class}\n");
        for row in rows {
            let _ = writeln!(out, "{} {}", row.id, row.format);
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
