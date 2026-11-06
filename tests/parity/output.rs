use super::*;
use std::ops::Range;

static NS_DECLARATION: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r#" xmlns:ns\d+="[^"]*""#).unwrap());
static NS_TAG_PREFIX: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"<(/?)ns\d+:").unwrap());

/// Removes the `nsN:` prefixes lxml writes for foreign elements of the oracle's html5 tree, as
/// `rewrite_namespaces` in `tools/parity/parity/gen_xml_output_cases.py` does (D-2): the crate writes
/// such an element by its local name.
pub(crate) fn strip_lxml_namespaces(expected: &str) -> String {
    let without_declarations = NS_DECLARATION.replace_all(expected, "");
    NS_TAG_PREFIX
        .replace_all(&without_declarations, "<$1")
        .into_owned()
}

/// One page and format of the output check.
pub(crate) struct OutputCase<'a> {
    pub(crate) id: &'a str,
    pub(crate) source: &'a str,
    pub(crate) valid_utf8: bool,
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
        .filter(|(page, _, _)| *page == id)
        .flat_map(|(_, field, _)| match &metadata[*field] {
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
/// `upstream-error`, `normalized` (a page in `normalized_rejects` only, whole output), then, when both
/// sides have text, per hunk `encoding` (page not valid UTF-8), `known-metadata`, or `port-bug`. A
/// fingerprint hunk is `encoding` when the page is not valid UTF-8 and every other hunk is `encoding`,
/// else `port-bug`. Text on one side only is a whole-output `port-bug`.
pub(crate) fn classify_output(
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
    let mut classified: Vec<(Hunk, &'static str)> = hunks::hunks(&left, &right)
        .into_iter()
        .map(|hunk| {
            let class = if !case.valid_utf8 && is_encoding_text_hunk(&left, &right, &hunk) {
                "encoding"
            } else if is_fingerprint_hunk(&left, &right, &hunk) {
                "fingerprint"
            } else if known_metadata_hunk(&left, &right, &hunk, case.known_values) {
                "known-metadata"
            } else {
                "port-bug"
            };
            (hunk, class)
        })
        .collect();
    // A fingerprint follows the text of its page: D-3 changes both, and a fingerprint next to any
    // other kind of difference is not explained.
    let fingerprint_class = if !case.valid_utf8
        && classified
            .iter()
            .all(|(_, class)| matches!(*class, "encoding" | "fingerprint"))
    {
        "encoding"
    } else {
        "port-bug"
    };
    for (_, class) in &mut classified {
        if *class == "fingerprint" {
            *class = fingerprint_class;
        }
    }
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
    let mentions = |line: &str| {
        known_values
            .iter()
            .any(|value| line.contains(value.as_str()))
    };
    !left.is_empty()
        && left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(a, b)| mentions(a) || mentions(b))
}

/// `line` with the value of each fingerprint blanked: the text of `<note type="fingerprint">` and the
/// `fingerprint` attribute of the xml `<doc>`.
fn without_fingerprint(line: &str) -> String {
    blank_after(
        &blank_after(line, "type=\"fingerprint\">", '<'),
        "fingerprint=\"",
        '"',
    )
}

/// `text` with the value after each `marker` removed up to `end`, which stays in the output.
fn blank_after(text: &str, marker: &str, end: char) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some((head, tail)) = rest.split_once(marker) {
        out.push_str(head);
        out.push_str(marker);
        rest = match tail.split_once(end) {
            Some((_, after)) => {
                out.push(end);
                after
            }
            None => "",
        };
    }
    out.push_str(rest);
    out
}

/// Whether the hunk differs only in a fingerprint value: each line pair is equal once the values are
/// blanked, and some pair differs.
fn is_fingerprint_hunk(expected: &[&str], actual: &[&str], hunk: &Hunk) -> bool {
    let left = expected.get(hunk.expected.clone()).unwrap_or_default();
    let right = actual.get(hunk.actual.clone()).unwrap_or_default();
    !left.is_empty()
        && left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(a, b)| without_fingerprint(a) == without_fingerprint(b))
        && left.iter().zip(right).any(|(a, b)| a != b)
}

pub(crate) struct OutputResult {
    pub(crate) id: String,
    pub(crate) format: &'static str,
    pub(crate) classes: Vec<&'static str>,
    hunks: Vec<(Hunk, &'static str)>,
    expected: Option<String>,
    actual: Option<String>,
}

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
    let known = known_values(id, &fixture["html5"]["metadata"]);
    OUTPUT_FORMATS
        .iter()
        .map(|&(name, format)| {
            let expected = fixture["html5"]["formats"][name]
                .as_str()
                .map(|text| match name {
                    // deviation: D-2 (the oracle writes `nsN:` prefixes for foreign elements)
                    "html" | "xml" | "xmltei" => {
                        let stripped = strip_lxml_namespaces(text);
                        assert!(
                            !stripped.contains(" xmlns:ns") && !NS_TAG_PREFIX.is_match(&stripped),
                            "{id}: namespace prefixes left in the expected {name}"
                        );
                        stripped
                    }
                    _ => text.to_string(),
                });
            let actual = Options::new()
                .format(format)
                .build()
                .extract(&Input::bytes(&bytes), &call);
            let (classes, hunks) = classify_output(
                &OutputCase {
                    id,
                    source,
                    valid_utf8,
                    expected: expected.as_deref(),
                    upstream_error: fixture["html5"]["errors"][name].is_string(),
                    actual: &actual,
                    known_values: &known,
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
pub(crate) fn diff_order(rows: &[&[&'static str]]) -> Vec<usize> {
    let (urgent, rest): (Vec<_>, Vec<_>) = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| **row != ["match"] && **row != ["upstream-error"])
        .partition(|(_, row)| row.iter().any(|c| matches!(*c, "port-bug" | "normalized")));
    urgent.into_iter().chain(rest).map(|(i, _)| i).collect()
}

pub(crate) fn write_output_report(results: &[OutputResult], pages: usize) -> PathBuf {
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
