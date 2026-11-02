use super::*;
use crate::html_processing::cases::{CASES, Case};

fn selector_list(name: &str) -> &'static [Selector] {
    match name {
        "OVERALL_DISCARD_XPATH" => OVERALL_DISCARD_XPATH,
        "RAW_TREE_PRUNE_XPATH" => RAW_TREE_PRUNE_XPATH,
        "TEASER_DISCARD_XPATH" => TEASER_DISCARD_XPATH,
        "PRECISION_DISCARD_XPATH" => PRECISION_DISCARD_XPATH,
        "DISCARD_IMAGE_ELEMENTS" => DISCARD_IMAGE_ELEMENTS,
        "REMOVE_COMMENTS_XPATH" => REMOVE_COMMENTS_XPATH,
        "REMOVE_COMMENTS_AND_LISTS_XPATH" => REMOVE_COMMENTS_AND_LISTS_XPATH,
        "COMMENTS_DISCARD_XPATH" => COMMENTS_DISCARD_XPATH,
        other => panic!("unknown selector list {other}"),
    }
}

fn case_flag(text: &str, op: &str) -> bool {
    match text {
        "true" => true,
        "false" => false,
        other => panic!("{op}: bad flag {other}"),
    }
}

/// Runs the operation of `case` on `tree` with the case's options and returns the root of the result:
/// the document root, or what `tree_cleaning` returns.
fn run_case(case: &Case, tree: &mut Tree) -> NodeId {
    let mut options = options();
    options.tables = case.options.tables;
    options.images = case.options.images;
    options.links = case.options.links;
    options.formatting = case.options.formatting;
    let focus = match case.options.focus {
        "balanced" => Focus::Balanced,
        "precision" => Focus::Precision,
        "recall" => Focus::Recall,
        other => panic!("bad focus {other}"),
    };
    options.focus = focus;
    let call = Call::default();
    let ctx = Ctx {
        options: Cow::Borrowed(&options),
        call: &call,
    };
    let document = tree.root();
    let target = if case.scope.is_empty() {
        document
    } else {
        tree.find(document, case.scope).unwrap()
    };
    let parts: Vec<&str> = case.op.split(':').collect();
    match parts.as_slice() {
        ["tree_cleaning", edits @ ..] => {
            let mut cleaned = MANUALLY_CLEANED.to_vec();
            for edit in edits.iter().flat_map(|edits| edits.split(',')) {
                let (sign, tag) = edit.split_at(1);
                match sign {
                    "+" => cleaned.push(tag),
                    "-" => cleaned.retain(|listed| *listed != tag),
                    _ => panic!("bad edit {edit}"),
                }
            }
            return tree_cleaning_with(tree, target, &ctx, &cleaned, MANUALLY_STRIPPED).unwrap();
        }
        ["prune_html"] => prune_html(tree, target, focus, &call).unwrap(),
        ["convert_tags"] => convert_tags(tree, target, &ctx, None).unwrap(),
        ["prune_unwanted_nodes", list, rest @ ..] => {
            let with_backup = rest == ["backup"];
            prune_unwanted_nodes(tree, target, selector_list(list), with_backup, &call).unwrap();
        }
        ["delete_by_link_density", tag, backtracking, precision] => delete_by_link_density(
            tree,
            target,
            tag,
            case_flag(backtracking, case.op),
            case_flag(precision, case.op),
            &call,
        )
        .unwrap(),
        _ => panic!("unknown op {}", case.op),
    }
    document
}

#[test]
fn cases_match_upstream() {
    assert!(!CASES.is_empty());
    let mut parser_differences = Vec::new();
    let mut mismatches = Vec::new();
    for (index, case) in CASES.iter().enumerate() {
        let mut tree =
            parse_document(case.html, &ParseLimits::default(), &Call::default()).unwrap();
        let (parsed, normalized) = dump(&tree, DumpMode::Normalized);
        if normalized || parsed != case.dump_in {
            parser_differences.push(format!(
                "case {index} {}\n{}\nrust:\n{parsed}\nupstream:\n{}",
                case.op, case.html, case.dump_in
            ));
            continue;
        }
        let returned = run_case(case, &mut tree);
        let (actual, _) = dump_from(&tree, returned, DumpMode::Normalized);
        if actual != case.dump_out {
            let options = &case.options;
            mismatches.push(format!(
                "case {index} {} {:?} tables={} images={} links={} formatting={} focus={}\n{}\nrust:\n{actual}\nupstream:\n{}",
                case.op,
                case.scope,
                options.tables,
                options.images,
                options.links,
                options.formatting,
                options.focus,
                case.html,
                case.dump_out
            ));
        }
    }
    assert!(
        parser_differences.is_empty(),
        "{} of {} snippets parse differently from html5lib (change the snippet)\n{}",
        parser_differences.len(),
        CASES.len(),
        parser_differences.join("\n")
    );
    assert!(
        mismatches.is_empty(),
        "{} of {} cases differ from upstream\n{}",
        mismatches.len(),
        CASES.len(),
        mismatches.join("\n")
    );
}
