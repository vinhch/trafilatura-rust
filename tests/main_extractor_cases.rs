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

use std::collections::HashSet;

use cases::{CaseOptions, HTML_CASES, HtmlCase, XML_CASES, XmlCase};
use trafilatura_rust::__internals::{
    Config, DumpMode, Extractor, Focus, Format, NodeId, ParseLimits, Tree, convert_tags, dump,
    dump_from, extract_comments, extract_content, handle_formatting, handle_lists,
    handle_paragraphs, handle_quotes, handle_table, handle_textelem, handle_titles, parse_document,
    tree_cleaning,
};
use trafilatura_rust::Call;
use trees::{ctx_for, xml_tree};

fn extractor(options: &CaseOptions) -> Extractor {
    let focus = match options.focus {
        "balanced" => Focus::Balanced,
        "precision" => Focus::Precision,
        "recall" => Focus::Recall,
        other => panic!("bad focus {other}"),
    };
    let mut extractor = Extractor::new(
        Config::default(),
        Format::Python,
        focus == Focus::Precision,
        focus == Focus::Recall,
        Some(options.formatting),
        false,
        false,
        HashSet::new(),
        HashSet::new(),
    );
    extractor.tables = options.tables;
    extractor.images = options.images;
    extractor.links = options.links;
    extractor.comments = options.comments;
    extractor.url = options.url.map(str::to_string);
    extractor
}

fn describe(options: &CaseOptions) -> String {
    format!(
        "tables={} images={} links={} formatting={} focus={} comments={} url={:?}",
        options.tables,
        options.images,
        options.links,
        options.formatting,
        options.focus,
        options.comments,
        options.url
    )
}

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

/// `tree_cleaning` and `convert_tags` on the parsed snippet, then the case's operation.
fn run_html(case: &HtmlCase, mut tree: Tree) -> HtmlOutcome {
    let options = extractor(&case.options);
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let root = tree.root();
    let root = tree_cleaning(&mut tree, root, &ctx).unwrap();
    convert_tags(&mut tree, root, &ctx, None).unwrap();
    let comments = match case.op {
        "extract_content" => None,
        "comments_then_content" => {
            let (body, text) = extract_comments(&mut tree, root, &ctx).unwrap();
            Some((dump_from(&tree, body, DumpMode::Normalized).0, text))
        }
        other => panic!("unknown op {other}"),
    };
    let (body, text) = extract_content(&mut tree, root, &ctx).unwrap();
    HtmlOutcome {
        comments,
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
        let outcome = run_html(case, tree);
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
