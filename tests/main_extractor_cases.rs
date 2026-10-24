#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! The `main_extractor` handlers and extractors against the trees upstream leaves
//! (`docs/parity-harness.md`). An integration test because half of the cases start from
//! `etree.fromstring` snippets, which `tests/trees` builds.

#[allow(dead_code)]
mod trees;

#[path = "main_extractor_cases/cases.rs"]
mod cases;

use cases::{HTML_CASES, HtmlCase, XML_CASES, XmlCase};
use trafilatura_rust::__internals::{
    DumpMode, NodeId, ParseLimits, dump, dump_from, handle_formatting, handle_lists,
    handle_paragraphs, handle_quotes, handle_table, handle_textelem, handle_titles, parse_document,
};
use trafilatura_rust::Call;
use trees::{ctx_for, describe, extractor, run_pipeline, xml_tree};

/// Runs the handler of `case`; returns the dump of its result and of the whole snippet.
fn run_xml(case: &XmlCase) -> (Option<String>, String) {
    let options = extractor(&case.options);
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let mut tree = xml_tree(case.xml);
    let root = tree.root();
    let element = if case.target.is_empty() {
        root
    } else {
        tree.iter(root, &[case.target]).next().unwrap()
    };
    let tags = case.potential_tags.unwrap_or(&[]);
    let result: Option<NodeId> = match case.handler {
        "handle_table" => handle_table(&mut tree, element, tags, &ctx).unwrap(),
        "handle_lists" => handle_lists(&mut tree, element, &ctx).unwrap(),
        "handle_quotes" => handle_quotes(&mut tree, element, &ctx).unwrap(),
        "handle_paragraphs" => handle_paragraphs(&mut tree, element, tags, &ctx).unwrap(),
        "handle_titles" => handle_titles(&mut tree, element, &ctx).unwrap(),
        "handle_formatting" => handle_formatting(&mut tree, element, &ctx),
        "handle_textelem" => handle_textelem(&mut tree, element, tags, &ctx).unwrap(),
        other => panic!("unknown handler {other}"),
    };
    let result = result.map(|node| dump_from(&tree, node, DumpMode::Normalized).0);
    (result, dump_from(&tree, root, DumpMode::Normalized).0)
}

/// What the pipeline of `case` leaves: the comments body (dump, text) and the content body.
struct HtmlOutcome {
    comments: Option<(String, String)>,
    body: (String, String),
}

/// The pipeline of `case` on its html, the bodies as dumps and texts.
fn run_html(case: &HtmlCase) -> HtmlOutcome {
    let (tree, comments, body, text) = run_pipeline(case.html, case.op, &case.options);
    HtmlOutcome {
        comments: comments
            .map(|(body, text)| (dump_from(&tree, body, DumpMode::Normalized).0, text)),
        body: (dump_from(&tree, body, DumpMode::Normalized).0, text),
    }
}

#[test]
fn xml_cases_match_upstream() {
    assert!(!XML_CASES.is_empty());
    let mut mismatches = Vec::new();
    for (index, case) in XML_CASES.iter().enumerate() {
        let (result, source) = run_xml(case);
        if result.as_deref() != case.dump_result || source != case.dump_source {
            mismatches.push(format!(
                "case {index} {} target={:?} potential_tags={:?} {}\n{}\nrust result:\n{}\nupstream result:\n{}\nrust source:\n{source}\nupstream source:\n{}",
                case.handler,
                case.target,
                case.potential_tags,
                describe(&case.options),
                case.xml,
                result.as_deref().unwrap_or("None\n"),
                case.dump_result.unwrap_or("None\n"),
                case.dump_source,
            ));
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

#[test]
fn html_cases_match_upstream() {
    assert!(!HTML_CASES.is_empty());
    let mut parser_differences = Vec::new();
    let mut mismatches = Vec::new();
    for (index, case) in HTML_CASES.iter().enumerate() {
        let tree = parse_document(case.html, &ParseLimits::default(), &Call::default()).unwrap();
        let (parsed, normalized) = dump(&tree, DumpMode::Normalized);
        if normalized || parsed != case.dump_in {
            parser_differences.push(format!(
                "case {index} {}\n{}\nrust:\n{parsed}\nupstream:\n{}",
                case.op, case.html, case.dump_in
            ));
            continue;
        }
        let outcome = run_html(case);
        let comments = outcome
            .comments
            .as_ref()
            .map(|(dump, text)| (dump.as_str(), text.as_str()));
        let expected_comments = case.dump_comments.zip(case.comments_text);
        if comments != expected_comments
            || outcome.body != (case.dump_body.to_string(), case.body_text.to_string())
        {
            mismatches.push(format!(
                "case {index} {} {}\n{}\nrust comments:\n{:?}\nupstream comments:\n{:?}\nrust body:\n{}\ntext {:?}\nupstream body:\n{}\ntext {:?}",
                case.op,
                describe(&case.options),
                case.html,
                comments,
                expected_comments,
                outcome.body.0,
                outcome.body.1,
                case.dump_body,
                case.body_text,
            ));
        }
    }
    assert!(
        parser_differences.is_empty(),
        "{} of {} snippets parse differently from html5lib (change the snippet)\n{}",
        parser_differences.len(),
        HTML_CASES.len(),
        parser_differences.join("\n")
    );
    assert!(
        mismatches.is_empty(),
        "{} of {} cases differ from upstream\n{}",
        mismatches.len(),
        HTML_CASES.len(),
        mismatches.join("\n")
    );
}
