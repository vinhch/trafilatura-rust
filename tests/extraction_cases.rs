#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! `extract`, `baseline` and `html2txt` on whole pages against what upstream gives
//! (`docs/parity-harness.md`). `extract` runs through the public API; `baseline` is internal.
//! The cases come from `tools/parity/parity/gen_extraction_cases.py`: the snippets of the
//! main-extractor cases, pages for each branch of the cascade, and JSON-LD and Discourse pages for
//! the baseline.

#[allow(dead_code)]
mod common;
#[allow(dead_code)]
mod trees;

#[path = "extraction_cases/cases.rs"]
mod cases;

use cases::{BASELINE_CASES, BaselineCase, EXTRACT_CASES, Expected, ExtractCase};
use trafilatura_rust::__internals::{HtmlInput, ParseLimits, baseline};
use trafilatura_rust::{
    Call, ExtractError, Extractor, Focus, Format, Input, Options, Rejection, html2txt,
};
use trees::describe;

/// How many differences a failure message spells out; the count covers all of them.
const SHOWN: usize = 12;

/// The `Extractor` of a case: its format and options, everything else at upstream's defaults.
fn extractor(case: &ExtractCase) -> Extractor {
    let format = match case.format {
        "txt" => Format::Txt,
        "markdown" => Format::Markdown,
        "html" => Format::Html,
        other => panic!("unknown format {other}"),
    };
    let focus = match case.options.focus {
        "balanced" => Focus::Balanced,
        "precision" => Focus::Precision,
        "recall" => Focus::Recall,
        other => panic!("unknown focus {other}"),
    };
    Options::new()
        .format(format)
        .focus(focus)
        .comments(case.options.comments)
        .formatting(case.options.formatting)
        .links(case.options.links)
        .images(case.options.images)
        .tables(case.options.tables)
        .build()
}

fn run_extract(case: &ExtractCase) -> Result<String, ExtractError> {
    let extractor = extractor(case);
    let mut input = Input::text(case.html);
    if let Some(url) = case.options.url {
        input = input.url(url);
    }
    extractor.extract(&input, &Call::default())
}

/// Whether `got` is what upstream gave. `Err(Rejected(_))` stands for upstream's `None` and for an
/// exception that escapes; the escape is the one `Rejection::InvalidValue`.
fn extract_matches(got: &Result<String, ExtractError>, want: &Expected) -> bool {
    match (got, want) {
        (Ok(got), Ok(Some(want))) => got == want,
        (Err(ExtractError::Rejected(_)), Ok(None)) => true,
        (Err(error), Err(_)) => *error == ExtractError::Rejected(Rejection::InvalidValue),
        _ => false,
    }
}

fn show_expected(want: &Expected) -> String {
    match want {
        Ok(Some(text)) => format!("Ok({text:?})"),
        Ok(None) => "None".to_string(),
        Err(name) => format!("raises {name}"),
    }
}

fn summary(found: &[String], total: usize, what: &str) -> String {
    let shown = found.iter().take(SHOWN).cloned().collect::<Vec<_>>();
    format!(
        "{} of {total} {what} differ from upstream, first {}:\n{}",
        found.len(),
        shown.len(),
        shown.join("\n")
    )
}

#[test]
fn extract_cases_match_upstream() {
    assert!(!EXTRACT_CASES.is_empty());
    let mut found = Vec::new();
    for (index, case) in EXTRACT_CASES.iter().enumerate() {
        let got = run_extract(case);
        if !extract_matches(&got, &case.expected) {
            found.push(format!(
                "case {index} {} {}\n{}\nrust:     {got:?}\nupstream: {}",
                case.format,
                describe(&case.options),
                case.html,
                show_expected(&case.expected)
            ));
        }
    }
    assert!(
        found.is_empty(),
        "{}",
        summary(&found, EXTRACT_CASES.len(), "extract cases")
    );
}

/// The differences between the crate's `baseline` and `html2txt` on a page and upstream's.
fn compare_baseline(case: &BaselineCase) -> Vec<String> {
    let mut found = Vec::new();
    let call = Call::default();
    let got = baseline(HtmlInput::Str(case.html), &ParseLimits::default(), &call)
        .map(|(_, _, text, length)| (text, length));
    let same = match (&got, &case.baseline) {
        (Ok(got), Ok((text, length))) => got.0 == *text && got.1 == *length,
        (Err(error), Err(_)) => *error == ExtractError::Rejected(Rejection::InvalidValue),
        _ => false,
    };
    if !same {
        found.push(format!(
            "baseline\nrust:     {got:?}\nupstream: {:?}",
            case.baseline
        ));
    }
    let text = html2txt(&Input::text(case.html), true, &call);
    if text.as_deref() != Ok(case.html2txt) {
        found.push(format!(
            "html2txt\nrust:     {text:?}\nupstream: {:?}",
            case.html2txt
        ));
    }
    found
}

#[test]
fn baseline_cases_match_upstream() {
    assert!(!BASELINE_CASES.is_empty());
    let mut found = Vec::new();
    for (index, case) in BASELINE_CASES.iter().enumerate() {
        let differences = compare_baseline(case);
        if !differences.is_empty() {
            found.push(format!(
                "case {index}\n{}\n{}",
                case.html,
                differences.join("\n")
            ));
        }
    }
    assert!(
        found.is_empty(),
        "{}",
        summary(&found, BASELINE_CASES.len(), "baseline cases")
    );
}

#[test]
fn utf8_page_raises_invalid_tag() {
    // oracle: extract(read("tests/resources/utf8.html"), options=Extractor(output_format=fmt,
    //   fast=True)) is None for txt, markdown and html: define_newelem raises
    //   ValueError("Invalid tag name 'http:'"), which bare_extraction catches
    let Some(root) = common::upstream_root() else {
        assert!(
            !common::require_fixtures(),
            "PARITY_REQUIRE_FIXTURES is set but the upstream checkout is missing"
        );
        eprintln!("extraction_cases: no upstream checkout, skipping utf8_page_raises_invalid_tag");
        return;
    };
    let bytes = std::fs::read(root.join("tests/resources/utf8.html")).unwrap();
    for format in [Format::Txt, Format::Markdown, Format::Html] {
        let result = Options::new()
            .format(format)
            .build()
            .extract(&Input::bytes(&bytes), &Call::default());
        assert_eq!(
            result,
            Err(ExtractError::Rejected(Rejection::InvalidValue)),
            "{format:?}"
        );
    }
}
