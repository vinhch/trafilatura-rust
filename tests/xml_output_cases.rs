#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! The TXT, Markdown and HTML output against what upstream gives for extracted bodies and for
//! `etree.fromstring` snippets (`docs/parity-harness.md`). An integration test because the
//! snippets start from `tests/trees`. The html cases are not checked against the html5lib parse
//! here: `tests/main_extractor_cases.rs` checks the parse of the snippets of
//! `gen_main_extractor_cases` only, and a parser difference on the other snippets shows up here as
//! an output mismatch.

#[allow(dead_code)]
mod trees;

#[path = "xml_output_cases/cases.rs"]
mod cases;

use cases::{HTML_CASES, HtmlOutCase, Output, XML_CASES, XmlOutCase};
use trafilatura_rust::__internals::{NodeId, Tree, build_html_output, xml_to_text};
use trafilatura_rust::{Call, ExtractError, Rejection};
use trees::{describe, run_pipeline, xml_tree};

type Got = Result<String, ExtractError>;

/// The three outputs of a tree: TXT, Markdown, and the HTML output of a copy of `body`.
fn outputs(tree: &mut Tree, body: NodeId) -> (Got, Got, Got) {
    let call = Call::default();
    let txt = xml_to_text(tree, Some(body), false, &call);
    let md = xml_to_text(tree, Some(body), true, &call);
    let copy = tree.copy(body);
    let html = build_html_output(tree, copy, None, &call);
    (txt, md, html)
}

/// Whether `got` is what upstream gave. The crate has one error for the `ValueError` and the
/// `KeyError` upstream raises, so either name matches it.
fn matches(got: &Got, want: &Output) -> bool {
    match (got, want) {
        (Ok(got), Ok(want)) => got == want,
        (Err(error), Err(_)) => *error == ExtractError::Rejected(Rejection::InvalidValue),
        _ => false,
    }
}

fn show_got(got: &Got) -> String {
    match got {
        Ok(text) => format!("Ok({text:?})"),
        Err(error) => format!("Err({error:?})"),
    }
}

fn show_want(want: &Output) -> String {
    match want {
        Ok(text) => format!("Ok({text:?})"),
        Err(name) => format!("Err({name})"),
    }
}

/// The differences between the three outputs of a tree and the three expected; empty when they match.
fn compare(label: &str, got: &(Got, Got, Got), want: [&Output; 3]) -> Vec<String> {
    [(&got.0, "txt"), (&got.1, "md"), (&got.2, "html")]
        .into_iter()
        .zip(want)
        .filter(|((got, _), want)| !matches(got, want))
        .map(|((got, name), want)| {
            format!(
                "{label} {name}\nrust:     {}\nupstream: {}",
                show_got(got),
                show_want(want)
            )
        })
        .collect()
}

fn run_html(case: &HtmlOutCase) -> Vec<String> {
    let (mut tree, comments, body, _) = run_pipeline(case.html, case.op, &case.options);
    let mut found = compare(
        "body",
        &outputs(&mut tree, body),
        [&case.body_txt, &case.body_md, &case.body_html],
    );
    match (comments, &case.comments) {
        (None, None) => {}
        (Some((comments, _)), Some((txt, md, html))) => found.extend(compare(
            "comments",
            &outputs(&mut tree, comments),
            [txt, md, html],
        )),
        (got, want) => found.push(format!(
            "comments body: rust {} upstream {}",
            got.is_some(),
            want.is_some()
        )),
    }
    found
}

fn run_xml(case: &XmlOutCase) -> Vec<String> {
    let mut tree = xml_tree(case.xml);
    let root = tree.root();
    compare(
        "snippet",
        &outputs(&mut tree, root),
        [&case.txt, &case.md, &case.html],
    )
}

#[test]
fn html_cases_match_upstream() {
    assert!(!HTML_CASES.is_empty());
    let mut mismatches = Vec::new();
    for (index, case) in HTML_CASES.iter().enumerate() {
        let found = run_html(case);
        if !found.is_empty() {
            mismatches.push(format!(
                "case {index} {} {}\n{}\n{}",
                case.op,
                describe(&case.options),
                case.html,
                found.join("\n")
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of {} cases differ from upstream\n{}",
        mismatches.len(),
        HTML_CASES.len(),
        mismatches.join("\n")
    );
}

#[test]
fn xml_cases_match_upstream() {
    assert!(!XML_CASES.is_empty());
    let mut mismatches = Vec::new();
    for (index, case) in XML_CASES.iter().enumerate() {
        let found = run_xml(case);
        if !found.is_empty() {
            mismatches.push(format!("case {index}\n{}\n{}", case.xml, found.join("\n")));
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of {} cases differ from upstream\n{}",
        mismatches.len(),
        XML_CASES.len(),
        mismatches.join("\n")
    );
}
