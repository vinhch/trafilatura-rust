use super::*;
use crate::error::TreeLimit;

/// `Tree` with a `td` root carrying `attr="value"`.
fn cell_with(attr: &str, value: &str) -> (Tree, NodeId) {
    let mut tree = Tree::new(LocalName::from("td"), Ns::Html, Vec::new());
    let cell = tree.root();
    tree.set(cell, attr, value);
    (tree, cell)
}

fn colspan_of(value: &str) -> Result<usize, ExtractError> {
    let (tree, cell) = cell_with("colspan", value);
    span(&tree, cell, "colspan")
}

#[test]
fn span_reads_nd_digits() {
    // oracle: _span(td, "colspan") for td.set("colspan", value)
    assert_eq!(colspan_of("\u{663}"), Ok(3));
    assert_eq!(colspan_of("\u{661}\u{662}"), Ok(12));
    assert_eq!(colspan_of("\u{FF11}"), Ok(1));
    assert_eq!(colspan_of("3"), Ok(3));
    assert_eq!(colspan_of("12"), Ok(12));
    // not Nd, or not only digits: 1
    for value in ["\u{b2}", "1\u{b2}", "2x", "", "-2", " 2"] {
        assert_eq!(colspan_of(value), Ok(1), "{value:?}");
    }
    // a missing attribute is 1
    let tree = Tree::new(LocalName::from("td"), Ns::Html, Vec::new());
    assert_eq!(span(&tree, tree.root(), "colspan"), Ok(1));
    // the attribute asked for, not another one
    let (tree, cell) = cell_with("rowspan", "7");
    assert_eq!(span(&tree, cell, "colspan"), Ok(1));
    assert_eq!(span(&tree, cell, "rowspan"), Ok(7));
}

#[test]
fn span_zero() {
    // oracle: _span(td, "colspan") is 0 for "0" and 7 for "0" * 97 + "7"
    assert_eq!(colspan_of("0"), Ok(0));
    assert_eq!(colspan_of(&format!("{}7", "0".repeat(97))), Ok(7));
}

#[test]
fn span_caps_at_100() {
    // oracle: _span(td, "colspan") is 100 for "100", "101" and "1" * 4300, 0 for "0" * 4300
    assert_eq!(colspan_of("100"), Ok(100));
    assert_eq!(colspan_of("101"), Ok(100));
    assert_eq!(colspan_of(&"1".repeat(4300)), Ok(100));
    assert_eq!(colspan_of(&"0".repeat(4300)), Ok(0));
}

#[test]
fn span_too_many_digits_is_invalid_value() {
    // oracle: _span(td, "colspan") raises ValueError for "0" * 4301, "٣" * 4301 and "1" * 4301
    let invalid = Err(ExtractError::Rejected(Rejection::InvalidValue));
    assert_eq!(colspan_of(&"0".repeat(4301)), invalid);
    assert_eq!(colspan_of(&"\u{663}".repeat(4301)), invalid);
    assert_eq!(colspan_of(&"1".repeat(4301)), invalid);
}

/// `handle_table(etree.fromstring(source), {*TAG_CATALOG, "table"}, Extractor(recall=recall))`:
/// the result and the source after the call.
fn table_with(source: &str, recall: bool) -> Result<(Option<String>, String), ExtractError> {
    let mut tree = xml(source);
    let root = tree.root();
    let options = options_with_recall(recall);
    let call = Call::default();
    let tags: Vec<&str> = TAG_CATALOG.iter().copied().chain(["table"]).collect();
    let processed = handle_table(&mut tree, root, &tags, &ctx_for(&options, &call))?;
    Ok((processed.map(|id| show(&tree, id)), show(&tree, root)))
}

fn table(source: &str) -> Option<String> {
    table_with(source, false).unwrap().0
}

#[test]
fn table_colspan_zero_row() {
    // oracle: etree.tostring(handle_table(etree.fromstring(s), {*TAG_CATALOG, "table"}, Extractor()))
    // colspan 0 adds no cell and counts for nothing in max_cols
    assert_eq!(
        table("<table><tr><td colspan=\"0\">a</td><td>b</td></tr></table>").as_deref(),
        Some("<table><row><cell>a</cell><cell>b</cell></row></table>")
    );
    assert_eq!(
        table("<table><tr><td colspan=\"0\">a</td></tr><tr><td>b</td><td>c</td></tr></table>")
            .as_deref(),
        Some(
            "<table><row><cell>a</cell><cell/></row><row><cell>b</cell><cell>c</cell></row></table>"
        )
    );
}

#[test]
fn table_colspan_pads_up_to_the_cap() {
    // oracle: len(handle_table(etree.fromstring(s), ...)[0]) is 100 for
    // s = '<table><tr><td colspan="500">a</td></tr></table>'
    let processed = table("<table><tr><td colspan=\"500\">a</td></tr></table>").unwrap();
    assert_eq!(processed.matches("<cell").count(), 100);
    // oracle: a colspan row shorter than the widest row is padded to it
    assert_eq!(
        table(
            "<table><tr><td colspan=\"2\">a</td></tr><tr><td>b</td><td>c</td><td>d</td></tr></table>"
        )
        .as_deref(),
        Some(
            "<table><row><cell>a</cell><cell/><cell/></row><row><cell>b</cell><cell>c</cell><cell>d</cell></row></table>"
        )
    );
}

#[test]
fn table_span_beyond_the_digit_limit_is_invalid_value() {
    // oracle: handle_table raises ValueError for colspan or rowspan of "1" * 4301
    let invalid = Err(ExtractError::Rejected(Rejection::InvalidValue));
    for attr in ["colspan", "rowspan"] {
        let source = format!(
            "<table><tr><td {attr}=\"{}\">a</td></tr></table>",
            "1".repeat(4301)
        );
        assert_eq!(table_with(&source, false), invalid, "{attr}");
    }
}

#[test]
fn table_rowspan_phantoms() {
    // oracle: etree.tostring(handle_table(etree.fromstring(s), {*TAG_CATALOG, "table"}, Extractor()))
    let cases = [
        // a rowspan leaves a placeholder in the continuation row
        (
            "<table><tr><td rowspan=\"2\">x</td><td>a</td></tr><tr><td>b</td></tr></table>",
            "<table><row><cell>x</cell><cell>a</cell></row><row><cell/><cell>b</cell></row></table>",
        ),
        // rowspan and colspan occupy every spanned column
        (
            "<table><tr><td rowspan=\"2\" colspan=\"2\">big</td><td>c</td></tr><tr><td>x</td></tr></table>",
            "<table><row><cell>big</cell><cell/><cell>c</cell></row><row><cell/><cell/><cell>x</cell></row></table>",
        ),
        // the placeholder lasts rowspan - 1 rows, then the column is free
        (
            "<table><tr><td>a</td><td rowspan=\"3\">b</td></tr><tr><td>c</td></tr><tr><td>d</td></tr><tr><td>e</td></tr></table>",
            "<table><row><cell>a</cell><cell>b</cell></row><row><cell>c</cell><cell/></row><row><cell>d</cell><cell/></row><row><cell>e</cell><cell/></row></table>",
        ),
        // an empty row consumes one row of a pending rowspan and is dropped
        (
            "<table><tr><td rowspan=\"2\">x</td></tr><tr></tr><tr><td>y</td></tr></table>",
            "<table><row><cell>x</cell></row><row><cell>y</cell></row></table>",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(table(source).as_deref(), Some(expected), "{source}");
    }
}

#[test]
fn table_header_row_once() {
    // oracle: etree.tostring(handle_table(etree.fromstring(s), {*TAG_CATALOG, "table"}, Extractor()))
    // only the first row holding a th is a header row; its th cells are the head cells
    let cases = [
        (
            "<table><tr><th>a</th></tr><tr><th>b</th><td>c</td></tr></table>",
            "<table><row><cell role=\"head\">a</cell><cell/></row><row><cell>b</cell><cell>c</cell></row></table>",
        ),
        (
            "<table><tr><td>a</td><th>b</th></tr><tr><th>c</th><th>d</th></tr></table>",
            "<table><row><cell>a</cell><cell role=\"head\">b</cell></row><row><cell>c</cell><cell>d</cell></row></table>",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(table(source).as_deref(), Some(expected), "{source}");
    }
}

#[test]
fn table_captions() {
    // oracle: r = handle_table(t, TAG_CATALOG, Extractor()) for t = etree.fromstring(s); the pairs
    // are etree.tostring(r) and etree.tostring(t)
    let (processed, source) = table_with(
        "<table><caption> Cap <b>x</b> </caption><tr><td>a</td><td>b</td></tr></table>",
        false,
    )
    .unwrap();
    assert_eq!(
        processed.as_deref(),
        Some(
            "<table><row><cell role=\"head\">Cap  x</cell><cell/></row><row><cell>a</cell><cell>b</cell></row></table>"
        )
    );
    assert_eq!(
        source,
        "<table><done> Cap <done>x</done> </done><done><done>a</done><done>b</done></done></table>"
    );
    // an image in a caption stays for the main loop, without its tail
    let (processed, source) = table_with(
        "<table><caption>C<graphic src=\"a.jpg\"/>t</caption><tr><td>a</td></tr></table>",
        false,
    )
    .unwrap();
    assert_eq!(
        processed.as_deref(),
        Some("<table><row><cell role=\"head\">C t</cell></row><row><cell>a</cell></row></table>")
    );
    assert_eq!(
        source,
        "<table><done>C<graphic src=\"a.jpg\"/></done><done><done>a</done></done></table>"
    );
    // each caption is a row of its own, wherever it sits
    assert_eq!(
        table("<table><caption>One</caption><caption>Two</caption><tr><td>a</td></tr></table>")
            .as_deref(),
        Some(
            "<table><row><cell role=\"head\">One</cell></row><row><cell role=\"head\">Two</cell></row><row><cell>a</cell></row></table>"
        )
    );
    assert_eq!(
        table("<table><tr><td>a</td></tr><caption>Late</caption></table>").as_deref(),
        Some("<table><row><cell role=\"head\">Late</cell></row><row><cell>a</cell></row></table>")
    );
}

#[test]
fn table_cell_content() {
    // oracle: r = handle_table(t, TAG_CATALOG, Extractor()) for t = etree.fromstring(s); the pairs
    // are etree.tostring(r) and etree.tostring(t)
    // a line break before text becomes a paragraph, inline elements are carried
    let (processed, source) =
        table_with("<table><tr><td>x<hi>y</hi><lb/>z</td></tr></table>", false).unwrap();
    assert_eq!(
        processed.as_deref(),
        Some("<table><row><cell>x<hi>y</hi><p>z</p></cell></row></table>")
    );
    assert_eq!(
        source,
        "<table><done><done>x<done>y</done><done/>z</done></done></table>"
    );
    // a link wrapping inline content is carried as a subtree
    let (processed, source) = table_with(
        "<table><tr><td><ref><hi>link</hi></ref></td></tr></table>",
        false,
    )
    .unwrap();
    assert_eq!(
        processed.as_deref(),
        Some("<table><row><cell><ref><hi>link</hi></ref></cell></row></table>")
    );
    assert_eq!(
        source,
        "<table><done><done><done><done>link</done></done></done></done></table>"
    );
    // nested tables are left in the source for the main loop and their tails join the cell
    let (processed, source) = table_with(
        "<table><tr><td>a<table><tr><td>b</td></tr></table>t1<table><tr><td>c</td></tr></table>t2</td></tr></table>",
        false,
    )
    .unwrap();
    assert_eq!(
        processed.as_deref(),
        Some("<table><row><cell>a t1 t2</cell></row></table>")
    );
    assert_eq!(
        source,
        "<table><done><done>a<table><tr><td>b</td></tr></table><table><tr><td>c</td></tr></table></done></done></table>"
    );
    // an unknown element between rows is renamed and left out
    let (processed, source) =
        table_with("<table><tr><td>a</td></tr><svg/></table>", false).unwrap();
    assert_eq!(
        processed.as_deref(),
        Some("<table><row><cell>a</cell></row></table>")
    );
    assert_eq!(source, "<table><done><done>a</done></done><done/></table>");
}

#[test]
fn table_cell_foreign_element() {
    // oracle: p = parse_document('<html><body><table><tr><td>a <svg><title>t</title></svg> b</td>
    // </tr></table></body></html>') after tree_cleaning and convert_tags;
    // r = handle_table(p.find('.//table'), TAG_CATALOG, Extractor()); dump_tree(r) and dump_tree(p).
    // The svg title is an unknown element here: its text goes and its tail stays
    let (mut tree, table) = converted(
        "<html><body><table><tr><td>a <svg><title>t</title></svg> b</td></tr></table></body></html>",
        "table",
    );
    let options = options();
    let call = Call::default();
    let processed = handle_table(&mut tree, table, TAG_CATALOG, &ctx_for(&options, &call))
        .unwrap()
        .unwrap();
    // oracle: scripts/oracle.sh 'from lxml.html import fromstring; from parity.treedump import dump_tree;
    //   from trafilatura.utils import HTML_PARSER; from trafilatura.settings import Extractor;
    //   from trafilatura.htmlprocessing import tree_cleaning, convert_tags; from trafilatura.main_extractor import handle_table, TAG_CATALOG;
    //   o = Extractor(); t = convert_tags(tree_cleaning(fromstring(<page>, parser=HTML_PARSER), o), o);
    //   print(dump_tree(handle_table(t.find(".//table"), TAG_CATALOG, o))); print(dump_tree(t))'
    assert_eq!(
        dump_from(&tree, processed),
        "table\n  row\n    cell\n      #text \"a b\"\n"
    );
    assert_eq!(
        dump(&tree),
        "html\n  body\n    table\n      done\n        done\n          #text \"a b\"\n"
    );
}

#[test]
fn table_walk_checks_cancellation() {
    let options = options();
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    let mut tree = xml("<table><tr><td>a<hi>b</hi></td></tr></table>");
    let root = tree.root();
    assert_eq!(
        handle_table(&mut tree, root, TAG_CATALOG, &ctx_for(&options, &call)),
        Err(ExtractError::Cancelled)
    );
}

/// `handle_table_limited` on `source` with a budget of `budget` cells.
fn table_with_budget(source: &str, budget: usize) -> Result<Option<String>, ExtractError> {
    let mut tree = xml(source);
    let root = tree.root();
    let options = options();
    let call = Call::default();
    let processed = handle_table_limited(
        &mut tree,
        root,
        TAG_CATALOG,
        &ctx_for(&options, &call),
        budget,
    )?;
    Ok(processed.map(|id| show(&tree, id)))
}

#[test]
fn max_output_nodes_is_two_parses() {
    assert_eq!(DEFAULT_MAX_NODES, 500_000);
    assert_eq!(MAX_OUTPUT_NODES, 1_000_000);
    assert_eq!(MAX_OUTPUT_NODES, 2 * DEFAULT_MAX_NODES);
}

#[test]
fn table_span_amplification_is_bounded() {
    // each case creates over a million cells on an unbounded run
    let spanning = "<td rowspan=\"100\" colspan=\"100\">x</td>".repeat(100);
    let empty_rows = "<tr></tr>".repeat(99);
    let phantoms = format!("<table><tr>{spanning}</tr>{empty_rows}</table>");
    let narrow_rows = "<tr><td>b</td></tr>".repeat(20_000);
    let padding = format!("<table><tr><td colspan=\"100\">a</td></tr>{narrow_rows}</table>");
    let rejected = Err(ExtractError::Rejected(Rejection::TreeTooLarge(
        TreeLimit::TableCells,
    )));
    // 5_000: the first row alone overruns it; 15_000: it is the phantoms of the later rows
    assert_eq!(table_with_budget(&phantoms, 5_000), rejected);
    assert_eq!(table_with_budget(&phantoms, 15_000), rejected);
    assert_eq!(table_with_budget(&padding, 100_000), rejected);
}

#[test]
fn table_under_the_node_budget_matches_handle_table() {
    let source = "<table><tr><td colspan=\"3\">a</td></tr><tr><td>b</td></tr></table>";
    let expected = table(source);
    assert!(expected.is_some());
    assert_eq!(table_with_budget(source, 100), Ok(expected));
}

#[test]
fn table_budget_ignores_the_arena_size() {
    let source = "<table><tr><td colspan=\"3\">a</td></tr><tr><td>b</td></tr></table>";
    let expected = table(source);
    assert!(expected.is_some());
    let mut tree = xml(source);
    let root = tree.root();
    // detached elements, as the copies of the cascade leave in the arena
    for _ in 0..500 {
        tree.new_element("p");
    }
    let (options, call) = (options(), Call::default());
    let budget = 10;
    assert!(tree.len() > 50 * budget);
    let processed = handle_table_limited(
        &mut tree,
        root,
        TAG_CATALOG,
        &ctx_for(&options, &call),
        budget,
    )
    .unwrap()
    .map(|id| show(&tree, id));
    assert_eq!(processed, expected);
}

#[test]
fn table_budget_is_shared_by_the_tables_of_one_tree() {
    let source = "<table><tr><td>a</td><td>b</td><td>c</td></tr></table>";
    let (options, call) = (options(), Call::default());
    let ctx = ctx_for(&options, &call);
    let mut tree = xml(source);
    let first = tree.root();
    let second = tree.copy(first, &call).unwrap();
    // a 3-cell table fits a budget of 4 once, not twice
    assert!(handle_table_limited(&mut tree, first, TAG_CATALOG, &ctx, 4).is_ok());
    assert_eq!(
        handle_table_limited(&mut tree, second, TAG_CATALOG, &ctx, 4),
        Err(ExtractError::Rejected(Rejection::TreeTooLarge(
            TreeLimit::TableCells
        )))
    );
}

#[test]
fn table_empty_rows_are_not_padded() {
    // oracle: r = handle_table(etree.fromstring("<table><tr><td colspan=\"100\">x</td></tr>" +
    // "<tr></tr>" * 3000 + "</table>"), TAG_CATALOG, Extractor()): (len(r), len(r[0])) is (1, 100)
    let rows = "<tr></tr>".repeat(3000);
    let source = format!("<table><tr><td colspan=\"100\">x</td></tr>{rows}</table>");
    let mut tree = xml(&source);
    let root = tree.root();
    let options = options();
    let call = Call::default();
    let nodes_before = tree.len();
    let processed = handle_table(&mut tree, root, TAG_CATALOG, &ctx_for(&options, &call))
        .unwrap()
        .unwrap();
    assert_eq!(tree.child_count(processed), 1);
    assert_eq!(
        tree.children(tree.first_child(processed).unwrap()).count(),
        100
    );
    // one `row` element per source row and the cells of the kept row, no padding for the others
    assert!(
        tree.len() - nodes_before < 4000 + 200,
        "{} nodes created",
        tree.len() - nodes_before
    );
}

#[test]
fn table_cell_drops_empty_inline_wrapper() {
    // oracle: etree.tostring(handle_table(etree.fromstring(s), {*TAG_CATALOG, "table"}, Extractor()))
    // for s = '<table><tr><td>x<hi></hi></td></tr></table>': handle_textnode rejects the empty `hi`,
    // which adds nothing to the cell
    assert_eq!(
        table("<table><tr><td>x<hi></hi></td></tr></table>").as_deref(),
        Some("<table><row><cell>x</cell></row></table>")
    );
}
