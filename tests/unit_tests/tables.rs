use super::*;

/// `_table_txt(table)`.
fn table_txt(table: &str) -> Option<String> {
    extract_doc(table, false, &Kw::zero().tables(true))
}

/// `handle_table(html.fromstring(table), TAG_CATALOG, Extractor(recall=recall))` as `tostring`, `None`
/// where there is no table.
fn handle_table_of(tree: &mut Tree, table: NodeId, recall: bool) -> Option<String> {
    let options = extractor(
        Format::Python,
        false,
        recall,
        None,
        (false, false),
        &[],
        &[],
    );
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let processed = handle_table(tree, table, TAG_CATALOG, &ctx).unwrap()?;
    Some(tree.tostring(processed, false, true))
}

const IMG_URL: &str = "http://aa.bb/c.jpg";

const IMG_TABLE_HEADER: &str = "<tr><td>a</td><td>b</td><td>c</td></tr>";

const IMG_TABLE_TAIL: &str = "<td><p>b</p><p>c</p></td><td>d</td>";

/// A link and bold paragraph, plus a table cell with a link and bold and an image cell: shared by
/// the combination tests.
const COMBO_DOC: &str = concat!(
    "<html><body><article>",
    r#"<p>Intro with a <a href="http://x.io/p">link</a> and <b>bold</b> word.</p>"#,
    "<table><tr><td>h1</td><td>h2</td></tr>",
    r#"<tr><td><a href="http://x.io/c"><b>bold link</b></a></td>"#,
    r#"<td><img src="http://x.io/i.jpg" alt="pic"/></td></tr></table>"#,
    "</article></body></html>"
);

/// `extract(_COMBO_DOC, output_format="markdown", config=ZERO_CONFIG, **_COMBO_ALL_ON)`.
fn combo_all_on() -> Kw {
    Kw::zero()
        .markdown()
        .links(true)
        .formatting(true)
        .images(true)
        .tables(true)
}

/// A first-row tail cell and a 3-cell second row that sets the table to 3 columns; the first row
/// fills to 3 via a colspan placeholder or a trailing pad depending on the span.
const COLSPAN_ROWS: &str = "<td>b</td></tr><tr><td>c</td><td>d</td><td>e</td></tr></table>";

#[test]
fn test_handle_table() {
    // handle_table output structure on small and broken tables.
    let cases = [
        // comments in a table (the tree keeps none)
        (
            "<table><!-- c1 --><tr><td>cell text<!-- c2 --></td></tr></table>",
            false,
            "<row><cell>cell text</cell></row>",
        ),
        (
            "<table><tr><!-- ignored --><td>visible</td></tr></table>",
            false,
            "<row><cell>visible</cell></row>",
        ),
        (
            "<table><tr><td>cell1</td><td>cell2</td></tr><tr><td>cell3</td><td>cell4</td></tr></table>",
            false,
            "<row><cell>cell1</cell><cell>cell2</cell></row><row><cell>cell3</cell><cell>cell4</cell></row>",
        ),
        (
            "<table><tr><td><p>text</p><p>more text</p></td></tr></table>",
            false,
            "<row><cell><p>text</p><p>more text</p></cell></row>",
        ),
        (
            "<table><tr><td>text<lb/><p>more text</p></td></tr></table>",
            false,
            "<row><cell>text<p>more text</p></cell></row>",
        ),
        // the ref stays in the cell, a <p> wrapper detached it
        (
            "<table><tr><td><ref target='test'>link</ref></td></tr></table>",
            false,
            "<row><cell><ref target=\"test\">link</ref></cell></row>",
        ),
        (
            "<table><tr><th>Month</th><th>Days</th></tr><tr><td>January</td><td>31</td></tr></table>",
            false,
            "<row><cell role=\"head\">Month</cell><cell role=\"head\">Days</cell></row><row><cell>January</cell><cell>31</cell></row>",
        ),
        (
            "<table><tr><th>Name</th><th colspan='2'>Phone</th></tr><tr><td>J</td><td>p1</td><td>p2</td></tr></table>",
            false,
            "<row><cell role=\"head\">Name</cell><cell role=\"head\">Phone</cell><cell role=\"head\"/></row><row><cell>J</cell><cell>p1</cell><cell>p2</cell></row>",
        ),
        (
            "<table><tr><td><hi>highlighted text</hi></td></tr></table>",
            false,
            "<row><cell><hi>highlighted text</hi></cell></row>",
        ),
        // only the outer table is processed, the extraction walk re-extracts the inner one
        (
            "<table><tr><td>\n<table><tr><td>1</td></tr></table>\n</td></tr></table>",
            false,
            "<row><cell>\n</cell></row>",
        ),
        (
            "<table><tr><td>\n<table><tr><td>1</td></tr></table>\n</td><td>text1</td></tr><tr><td>text2</td></tr></table>",
            false,
            "<row><cell>\n</cell><cell>text1</cell></row><row><cell>text2</cell><cell/></row>",
        ),
        (
            "<table><tr><td>A</td></tr><tr><td><table><tr><td>inner</td></tr></table></td></tr><tr><td>AFTER</td></tr></table>",
            false,
            "<row><cell>A</cell></row><row><cell>AFTER</cell></row>",
        ),
        // the tail after a nested table is kept, after the cell text or its last child
        (
            "<table><tr><td>before<table><tr><td>i</td></tr></table>after</td></tr></table>",
            false,
            "<row><cell>before after</cell></row>",
        ),
        (
            "<table><tr><td><del>struck</del><table><tr><td>i</td></tr></table>after</td></tr></table>",
            false,
            "<row><cell><del>struck</del> after</cell></row>",
        ),
        // lists in cells are only kept in recall mode
        (
            "<table><tr><td><p>a list</p><list><item>one</item></list></td></tr></table>",
            false,
            "<row><cell><p>a list</p></cell></row>",
        ),
        (
            "<table><tr><td><p>a list</p><list><item>one</item></list></td></tr></table>",
            true,
            "<row><cell><p>a list</p><list><item>one</item></list></cell></row>",
        ),
        // broken tables: orphan cells, no <tr> at all
        (
            "<table><td>cell1</td><tr><td>cell2</td></tr></table>",
            false,
            "<row><cell>cell1</cell></row><row><cell>cell2</cell></row>",
        ),
        (
            "<table><td>a</td><td>b</td></table>",
            false,
            "<row><cell>a</cell><cell>b</cell></row>",
        ),
        // colgroup/col are layout hints, a whitespace-only caption is skipped
        (
            "<table><colgroup><col style='width:50%'><col style='width:50%'></colgroup><tr><td>a</td><td>b</td></tr></table>",
            false,
            "<row><cell>a</cell><cell>b</cell></row>",
        ),
        (
            "<table><col span='2'><tr><td>x</td><td>y</td></tr></table>",
            false,
            "<row><cell>x</cell><cell>y</cell></row>",
        ),
        (
            "<table><col><td>orphan</td><tr><td>normal</td></tr></table>",
            false,
            "<row><cell>orphan</cell></row><row><cell>normal</cell></row>",
        ),
        (
            "<table><caption>  </caption><tr><td>x</td></tr></table>",
            false,
            "<row><cell>x</cell></row>",
        ),
    ];
    for (table, recall, expected) in cases {
        let (mut tree, element) = lxml_fromstring(table);
        assert_eq!(
            handle_table_of(&mut tree, element, recall).as_deref(),
            Some(format!("<table>{expected}</table>").as_str()),
            "{table}"
        );
    }
}

#[test]
fn handle_table_of_malformed_rows_matches_upstream() {
    // Malformed tables: libxml2 builds the tree as it is written, and upstream's handle_table reads
    // that tree. Each expectation is upstream's value on the lxml tree.
    // oracle: scripts/oracle.sh 'from lxml.html import fromstring, tostring; from trafilatura.utils import HTML_PARSER;
    //   from trafilatura.settings import Extractor; from trafilatura.main_extractor import handle_table, TAG_CATALOG;
    //   o = Extractor(); [print(repr(tostring(handle_table(fromstring(s, parser=HTML_PARSER), TAG_CATALOG, o),
    //   encoding="unicode"))) for s in [<the three tables below>]]'
    let cases = [
        // a td directly in the table after a tr gets a row of its own
        (
            "<table><tr><td>a</td></tr><td>b</td><tr><td>c</td></tr></table>",
            "<row><cell>a</cell><cell>b</cell></row><row><cell>c</cell></row>",
        ),
        // a p in a tr stays in the row
        (
            "<table><tr><p>text</p></tr><tr><td>cell</td></tr></table>",
            "<row><cell>cell</cell></row>",
        ),
        // a td in a div in a td nests inside the outer cell
        (
            "<table><tr><td><div><td>inner</td></div></td></tr></table>",
            "<row><cell><cell>inner</cell></cell></row>",
        ),
    ];
    for (table, upstream) in cases {
        let (mut tree, element) = lxml_fromstring(table);
        assert_eq!(
            handle_table_of(&mut tree, element, false),
            Some(format!("<table>{upstream}</table>")),
            "{table}"
        );
    }
}

#[test]
fn test_table_image_in_cell() {
    // Images in table cells render as GFM inline images; link wrapping is preserved.
    for (cell1, expected_cell1) in [
        (
            format!(r#"<td>a<img src="{IMG_URL}" alt="img"/><span>a</span></td>"#),
            format!("a ![img]({IMG_URL}) a"),
        ),
        (
            format!(
                r#"<td><a href="{IMG_URL}"><img src="{IMG_URL}" alt="img"/><span>a</span></a></td>"#
            ),
            format!("![img]({IMG_URL}) a"),
        ),
        (
            format!(r#"<td><img src="{IMG_URL}" alt="img"/><span>a</span></td>"#),
            format!("![img]({IMG_URL}) a"),
        ),
        (
            format!(
                r#"<td><img src="{IMG_URL}" alt="img1"/><span>a</span><img src="{IMG_URL}" alt="img2"/></td>"#
            ),
            format!("![img1]({IMG_URL}) a ![img2]({IMG_URL})"),
        ),
    ] {
        let html = format!(
            "<html><body><article><table>{IMG_TABLE_HEADER}<tr>{cell1}{IMG_TABLE_TAIL}</tr></table></article></body></html>"
        );
        let result = extract(&html, &Kw::zero().markdown().images(true).tables(true));
        assert_eq!(
            result,
            Some(format!("| a | b | c | \n| {expected_cell1} | b c | d |")),
            "{cell1}"
        );
    }
}

#[test]
fn test_combined_links_formatting_images_tables() {
    // All four flags on: cells keep formatting + images, and a link wrapping formatting keeps its
    // target.
    assert_eq!(
        extract(COMBO_DOC, &combo_all_on()).as_deref(),
        Some(concat!(
            "Intro with a [link](http://x.io/p) and **bold** word.\n\n",
            "| h1 | h2 | \n| [**bold link**](http://x.io/c) | ![pic](http://x.io/i.jpg) |"
        ))
    );
}

#[test]
fn test_combined_flags_toggle_off() {
    // Pin current behavior: disabling one flag changes only its feature (combined doc).
    let all_on = combo_all_on();
    for (kw, expected) in [
        (
            all_on.clone().links(false),
            "Intro with a link and **bold** word.\n\n| h1 | h2 | \n| **bold link** | ![pic](http://x.io/i.jpg) |",
        ),
        (
            all_on.clone().formatting(false),
            "Intro with a [link](http://x.io/p) and bold word.\n| h1 | h2 | \n| [bold link](http://x.io/c) | ![pic](http://x.io/i.jpg) |",
        ),
        (
            all_on.clone().images(false),
            "Intro with a [link](http://x.io/p) and **bold** word.\n\n| h1 | h2 | \n| [**bold link**](http://x.io/c) |  |",
        ),
        (
            all_on.tables(false),
            "Intro with a [link](http://x.io/p) and **bold** word.",
        ),
    ] {
        assert_eq!(extract(COMBO_DOC, &kw).as_deref(), Some(expected));
    }
}

#[test]
fn test_table_colspan_padding() {
    // A colspan=2 cell materializes an inline placeholder; the row fills to the table's column
    // count.
    assert!(
        table_txt(&format!("<table><tr><td colspan='2'>a</td>{COLSPAN_ROWS}"))
            .unwrap()
            .contains("| a |  | b |")
    );
}

#[test]
fn test_table_bad_span_attr_treated_as_colspan1() {
    // Non-colspan span attributes are ignored (colspan=1); trailing pad fills to max_cols.
    for bad_span in [
        r#"span="2""#,
        r#"span="2.1""#,
        r#"span="-1""#,
        r#"span="abc""#,
    ] {
        assert!(
            table_txt(&format!("<table><tr><td {bad_span}>a</td>{COLSPAN_ROWS}"))
                .unwrap()
                .contains("| a | b |  |"),
            "{bad_span}"
        );
    }
}

#[test]
fn test_table_huge_or_bad_colspan_no_crash() {
    // Huge or non-numeric colspan must not crash or discard the document (#657).
    for first_cell in [
        r#"<td colspan="9007199254740991">a</td>"#,
        r#"<th colspan="9007199254740991">a</th>"#,
        r#"<td colspan="2x">a</td>"#,
    ] {
        assert!(
            table_txt(&format!("<table><tr>{first_cell}{COLSPAN_ROWS}")).is_some(),
            "{first_cell}"
        );
    }
}

#[test]
fn test_colspan_zero_trust() {
    // `span` must default to 1 for non-decimal values (isdigit() admits superscripts that int() rejects)
    let cases = [
        ("1", 1),
        ("3", 3),
        ("12", 12),
        ("\u{b2}", 1),
        ("1\u{b2}", 1),
        ("2x", 1),
        ("", 1),
        ("-2", 1),
    ];
    for (colspan, expected) in cases {
        let mut tree = xml_tree("<tr/>");
        let cell = tree.new_element("td");
        tree.set(cell, "colspan", colspan);
        assert_eq!(span(&tree, cell, "colspan"), Ok(expected), "{colspan:?}");
    }
    // oracle: scripts/oracle.sh over _span(fromstring('<td colspan="N">x</td>', parser=HTML_PARSER), "colspan"):
    // lxml keeps the td, so the span is its value for "3" and "12"
    for (colspan, expected) in [("3", 3), ("12", 12)] {
        let (tree, cell) = lxml_fromstring(&format!("<td colspan=\"{colspan}\">x</td>"));
        assert_eq!(span(&tree, cell, "colspan"), Ok(expected), "{colspan:?}");
    }
}

#[test]
fn test_table_rowspan() {
    // Rowspan placeholders keep the columns aligned.
    for (table, suffix) in [
        // a rowspan leaves a placeholder in the continuation row
        (
            "<tr><td rowspan='2'>x</td><td>a</td></tr><tr><td>b</td></tr>",
            "| x | a | \n|  | b |",
        ),
        // rowspan + colspan registers every spanned column
        (
            "<tr><td rowspan='2' colspan='2'>big</td><td>c</td></tr><tr><td>x</td></tr>",
            "|  |  | x |",
        ),
        // padding a short row consumes the rowspan, no stale placeholder below
        (
            "<tr><td>a</td><td rowspan='2'>b</td><td>c</td></tr><tr><td>x</td></tr><tr><td>d</td><td>e</td><td>f</td></tr>",
            "| d | e | f |",
        ),
        // an empty <tr> still consumes one row of a pending rowspan
        (
            "<tr><td rowspan='3'>a</td><td>b</td></tr><tr></tr><tr><td>c</td></tr>",
            "| a | b | \n|  | c |",
        ),
        (
            "<tr><td rowspan='2'>a</td><td>b</td></tr><tr></tr><tr><td>c</td><td>d</td></tr>",
            "| a | b | \n| c | d |",
        ),
    ] {
        assert!(
            table_md(&format!("<table>{table}</table>"), Kw::zero())
                .unwrap()
                .ends_with(suffix),
            "{table}"
        );
    }
}

#[test]
fn test_table_empty_cells_and_rows() {
    // Empty cells are kept blank to keep columns aligned; rows that are entirely empty are dropped.
    for (html, suffix) in [
        ("<table><tr><td></td><td>b</td></tr></table>", "|  | b |"),
        ("<table><tr><td>a</td><td></td></tr></table>", "| a |  |"),
        (
            "<table><tr><td>a</td><td>b</td></tr><tr><td></td><td></td></tr></table>",
            "| a | b |",
        ),
        (
            "<table><tr><td>a</td><td>c</td></tr><tr><td></td><td></td></tr><tr><td>d</td><td>e</td></tr></table>",
            "| a | c | \n| d | e |",
        ),
        (
            "<table><tr><td>a</td><td>c</td></tr><tr></tr><tr><td>d</td><td>e</td></tr></table>",
            "| a | c | \n| d | e |",
        ),
    ] {
        assert!(
            table_md(html, Kw::zero()).unwrap().ends_with(suffix),
            "{html}"
        );
    }
}

#[test]
fn test_table_cell_list_no_row_break() {
    // A <ul> in a cell (no recall mode) must not inject a row-breaking newline.
    let result = table_md(
        "<table><tr><td><ul><li>i1</li><li>i2</li></ul></td><td>b</td></tr></table>",
        Kw::zero(),
    )
    .unwrap();
    let row = result.rsplit("\n\n").next().unwrap();
    assert!(!row.contains('\n'));
    assert!(row.ends_with("| b |"));
}

#[test]
fn test_table_cell_block_elements_flattened() {
    // Block elements in a cell are flattened to inline: no ## marker, space-separated, no mashing.
    for (html, suffix, kw) in [
        (
            "<table><tr><td><h2>Title</h2></td><td>b</td></tr></table>",
            "| Title | b |",
            Kw::zero(),
        ),
        (
            "<table><tr><td><p>para</p></td><td>b</td></tr></table>",
            "| para | b |",
            Kw::zero(),
        ),
        (
            "<table><tr><td><h2>Title</h2>txt</td><td>b</td></tr></table>",
            "| Title txt | b |",
            Kw::zero(),
        ),
        (
            "<table><tr><td><ul><li>i1</li><li>i2</li></ul></td><td>b</td></tr></table>",
            "| i1 i2 | b |",
            Kw::zero().focus(Focus::Recall),
        ),
    ] {
        assert!(table_md(html, kw).unwrap().ends_with(suffix), "{html}");
    }
}

#[test]
fn test_table_cell_keeps_nested_formatting() {
    // Regression #829/#396: formatting/images wrapped in a block inside a cell must survive.
    for (expected, cell, kw) in [
        (
            "**bold**",
            "<p><b>bold</b></p>",
            Kw::zero().formatting(true),
        ),
        (
            "![a](/i.jpg)",
            "<p><img src='/i.jpg' alt='a'></p>",
            Kw::zero().images(true),
        ),
        (
            "pre **mid** post",
            "<p>pre <b>mid</b> post</p>",
            Kw::zero().formatting(true),
        ),
        (
            "x ~~gone~~ y",
            "<p>x <del>gone</del> y</p>",
            Kw::zero().formatting(true),
        ),
        (
            "x `c` y",
            "<p>x <code>c</code> y</p>",
            Kw::zero().formatting(true),
        ),
    ] {
        assert!(
            table_md(
                &format!("<table><tr><td>{cell}</td><td>x</td></tr></table>"),
                kw
            )
            .unwrap()
            .contains(expected),
            "{cell}"
        );
    }
}
