//! The libxml2 tree rules, against the trees lxml builds from the snippets of `tree_cases.rs`.

use std::fmt::Write as _;

use crate::cancel::Call;
use crate::dom::parse::ParseLimits;
use crate::dom::parse::builder::build_document;
use crate::dom::parse::tree_cases::{TREE_CASES, TreeCase};
use crate::dom::{Tree, dump, dump_from};
use crate::error::{ExtractError, Rejection};

/// What differs between lxml's tree of `case` and the one the builder makes, `None` when they match.
fn mismatch(case: &TreeCase) -> Option<String> {
    let result = build_document(case.html, &ParseLimits::default(), &Call::default());
    if case.document == "!empty" {
        return match result {
            Err(ExtractError::Rejected(Rejection::EmptyTree)) => None,
            Ok(tree) => Some(format!(
                "expected: lxml raises \"Document is empty\"\nactual:\n{}",
                dump(&tree)
            )),
            Err(error) => Some(format!("expected: EmptyTree\nactual error: {error:?}")),
        };
    }
    let tree = match result {
        Ok(tree) => tree,
        Err(error) => return Some(format!("expected a tree\nactual error: {error:?}")),
    };
    let actual_root = dump(&tree);
    let actual_others: Vec<String> = document_others(&tree);
    if actual_root == case.document && actual_others == case.others {
        return None;
    }
    let mut report = String::new();
    if actual_root != case.document {
        let _ = write!(
            report,
            "root expected:\n{}root actual:\n{}",
            case.document, actual_root
        );
    }
    if actual_others != case.others {
        let _ = write!(
            report,
            "others expected: {:?}\nothers actual: {:?}",
            case.others, actual_others
        );
    }
    Some(report)
}

/// The dumps of the document-level elements after the root.
fn document_others(tree: &Tree) -> Vec<String> {
    tree.document_roots
        .iter()
        .skip(1)
        .map(|&id| dump_from(tree, id))
        .collect()
}

#[test]
fn document_trees_match_lxml() {
    let failures: Vec<String> = TREE_CASES
        .iter()
        .filter_map(|case| {
            mismatch(case).map(|diff| format!("{} ({:?}):\n{diff}", case.name, case.html))
        })
        .collect();
    assert!(
        failures.is_empty(),
        "{} of {} cases differ from lxml:\n{}",
        failures.len(),
        TREE_CASES.len(),
        failures.join("\n")
    );
}
