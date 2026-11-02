use super::*;

#[test]
fn test_include_formatting_markdown() {
    // Markdown defaults to formatted, but an explicit include_formatting=False is honored (was
    // ignored for markdown).
    let doc =
        "<html><body><article><p>plain and <b>bold</b> text here.</p></article></body></html>";
    assert_eq!(
        extract(doc, &Kw::zero().markdown()).as_deref(),
        Some("plain and **bold** text here.")
    );
    assert_eq!(
        extract(doc, &Kw::zero().markdown().formatting(false)).as_deref(),
        Some("plain and bold text here.")
    );
    assert_eq!(
        extract(doc, &Kw::zero().formatting(true)).as_deref(),
        Some("plain and **bold** text here.")
    );
}

#[test]
fn test_serializer_tail_spacing() {
    // Item and cell tails keep source separators, in order, without stacked spaces.
    let cases = [
        ("<list><item><lb/>one</item></list>", "- one"),
        ("<list><item>\n  <lb/>one</item></list>", "- one"),
        ("<list><item>one<p><lb/>two</p></item></list>", "- one\ntwo"),
        ("<list><item>a<lb/>b<lb/>c</item></list>", "- a\nb\nc"),
        (
            "<list><item>\n  <hi rend='#b'>x</hi> y</item></list>",
            "- **x** y",
        ),
        (
            "<list><item><ref target='s'>s</ref>: who use <ref target='b'>b</ref></item></list>",
            "- [s](s): who use [b](b)",
        ),
        (
            "<list><item>use <hi rend='#b'>bots</hi>, to resell.</item></list>",
            "- use **bots**, to resell.",
        ),
        (
            "<table><row><cell><p>x<lb/>y</p>z</cell></row></table>",
            "| x y z |",
        ),
        (
            "<table><row><cell><list><item>a</item><item>b</item></list>z</cell></row></table>",
            "| a b z |",
        ),
        (
            "<table><row><cell>use <hi rend='#b'>bots</hi>, x</cell></row></table>",
            "| use **bots**, x |",
        ),
        (
            "<table><row><cell><hi rend='#b'>A</hi> <hi rend='#b'>B</hi>, x</cell></row></table>",
            "| **A B**, x |",
        ),
        (
            "<table><row><cell>a<ref target='u'/> b</cell></row></table>",
            "| a b |",
        ),
        (
            "<table><row><cell>x<p>y</p>z mid<p>w</p></cell></row></table>",
            "| x y z mid w |",
        ),
        ("<list><item>a<p>b</p>c<p>d</p></item></list>", "- a b c d"),
        ("<list><item><lb/></item><item>b</item></list>", "- b"),
        (
            r##"<p><hi rend="#b">a</hi><hi rend="#b">b</hi><hi rend="#b">c</hi> tail</p>"##,
            "**abc** tail",
        ),
    ];
    for (body, expected) in cases {
        let doc = format!("<doc>{body}</doc>");
        assert_eq!(md_with(&doc, true), expected, "{body}");
        assert_eq!(md_with(&doc, false), expected.replace("**", ""), "{body}");
    }
}

#[test]
fn test_serializer_nested_lb_in_item_keeps_break() {
    // A nested line break after item content keeps its newline.
    let text = xmltotxt(
        "<doc><list><item><p>para</p><p><lb/>second</p></item></list></doc>",
        true,
    );
    assert!(text.contains("\nsecond"));
}

#[test]
fn test_serializer_nested_list_tail_stays_in_parent_item() {
    // Text after a nested list belongs to the parent item, not to the last sub-item.
    let source = "<doc><list><item>L1<list><item>L2<list><item>L3</item></list> t2</item></list> t1</item><item>n</item></list></doc>";
    assert_eq!(
        xmltotxt(source, true),
        "- L1\n  - L2\n    - L3\n\n    t2\n\n  t1\n- n\n"
    );
    assert_eq!(
        xmltotxt(source, false),
        "- L1\n  - L2\n    - L3\nt2\nt1\n- n"
    );
}

#[test]
fn test_extract_snippet() {
    // Line breaks before inline content, underline and <wbr> on intro-less snippets.
    for (id, body, kw, expected) in [
        (
            "item-leading-br",
            "<ul><li>\n  <br>tail after break</li><li>x</li></ul>",
            Kw::zero().markdown(),
            "- tail after break\n- x",
        ),
        (
            "cell-br-between-bold",
            "<table><tr><td><strong>A</strong><br>\n<strong>B</strong>, x</td></tr></table>",
            Kw::zero().markdown().tables(true),
            "| **A** **B**, x |",
        ),
        (
            "p-br-between-bold",
            "<p><strong>A</strong><br><strong>B</strong></p>",
            Kw::zero().markdown(),
            "**A**\n\n**B**",
        ),
        // underline has no markdown syntax (__ is bold), adjacent runs still merge
        (
            "underline-as-html",
            "<p><u>a</u><u>b</u> c</p>",
            Kw::zero().markdown(),
            "<u>ab</u> c",
        ),
        // the parser nests the rest of the paragraph inside <wbr>
        (
            "wbr-keeps-text",
            "<p>long<wbr>word and more</p>",
            Kw::zero(),
            "longword and more",
        ),
    ] {
        assert_eq!(
            extract_doc(body, false, &kw).as_deref(),
            Some(expected),
            "{id}"
        );
    }
}

#[test]
fn test_markdown_sup_sub_keep_boundary() {
    // sup/sub must keep a boundary, else 100<sup>2</sup> reads as 1002 (issue #889)
    let kw = Kw::zero().markdown();
    let sup = "<html><body><article><p>The layer has 100<sup>2</sup>=10000 nodes.</p></article></body></html>";
    let sub = "<html><body><article><p>Written 2011<sub>15ya</sub> in winter.</p></article></body></html>";
    assert_eq!(
        extract(sup, &kw).as_deref(),
        Some("The layer has 100<sup>2</sup>=10000 nodes.")
    );
    assert_eq!(
        extract(sub, &kw).as_deref(),
        Some("Written 2011<sub>15ya</sub> in winter.")
    );
    // flanking whitespace stays outside the tags, exactly as for the symmetric markers
    let spaced = "<html><body><article><p>x <sup> 2 </sup> y</p></article></body></html>";
    let bold = "<html><body><article><p>x <b> 2 </b> y</p></article></body></html>";
    assert_eq!(extract(spaced, &kw).as_deref(), Some("x  <sup>2</sup>  y"));
    assert_eq!(extract(bold, &kw).as_deref(), Some("x  **2**  y"));
}

#[test]
fn test_srcless_graphic_dropped_text_kept() {
    // A src-less image is dropped from a quote, the text around it in a paragraph is kept.
    let options = default_options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let mut tree = xml_tree("<quote>a<graphic/></quote>");
    let root = tree.root();
    let quote = handle_quotes(&mut tree, root, &ctx).unwrap().unwrap();
    assert_eq!(tree.tostring(quote, false, true), "<quote>a</quote>");
    let mut tree = xml_tree("<p>a<graphic/>b</p>");
    let root = tree.root();
    let tags: Vec<&str> = TAG_CATALOG.iter().copied().chain(["graphic"]).collect();
    let para = handle_paragraphs(&mut tree, root, &tags, &ctx)
        .unwrap()
        .unwrap();
    assert_eq!(tree.itertext(para).collect::<String>(), "ab");
}

#[test]
fn test_nested_block_tail_kept() {
    // The tail of a nested list or paragraph is kept, whether the block itself is kept or dropped.
    let cases = [
        (
            "<quote>a<list><item/></list> b</quote>",
            "<quote>a b</quote>",
        ),
        ("<quote>a<p><hi/></p> b</quote>", "<quote>a b</quote>"),
        ("<quote>a<list><item/></list> </quote>", "<quote>a</quote>"),
        (
            "<quote>a<list><item/></list>b</quote>",
            "<quote>a b</quote>",
        ),
        (
            "<quote>a <list><item/></list>b</quote>",
            "<quote>a b</quote>",
        ),
        ("<quote>a<graphic/>b</quote>", "<quote>ab</quote>"),
        (
            "<table><tr><td>x<list><item/></list> b</td></tr></table>",
            "<table><row><cell>x b</cell></row></table>",
        ),
        (
            "<table><tr><td>x<list><item/></list> </td></tr></table>",
            "<table><row><cell>x</cell></row></table>",
        ),
        (
            "<table><tr><td>x<list><item>i</item></list> b</td></tr></table>",
            "<table><row><cell>x<list><item>i</item></list> b</cell></row></table>",
        ),
    ];
    let options = extractor(Format::Python, false, true, None, (false, false), &[], &[]);
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let tags: Vec<&str> = TAG_CATALOG.iter().copied().chain(["table"]).collect();
    for (block, expected) in cases {
        let mut tree = xml_tree(block);
        let element = tree.root();
        let result = if tree.tag(element) == Some("quote") {
            handle_quotes(&mut tree, element, &ctx)
        } else {
            handle_table(&mut tree, element, &tags, &ctx)
        };
        assert_eq!(
            tree.tostring(result.unwrap().unwrap(), false, true),
            expected,
            "{block}"
        );
    }
}

#[test]
fn test_block_in_inline_wrapper_in_cell() {
    // A block wrapped in a link or bold inside a cell is kept.
    for (id, cell, links, expected) in [
        (
            "bold",
            "<b><div>Block in bold</div></b>",
            false,
            "| Block in bold | y |",
        ),
        (
            "link",
            r#"<a href="/x"><p>Block in link</p></a>"#,
            true,
            "| Block in link | y |",
        ),
        (
            "link-with-inline",
            r#"<a href="/x"><b>bold</b><p>after</p></a>"#,
            true,
            "| [**bold**](/x) after | y |",
        ),
    ] {
        let output = table_md(
            &format!("<table><tr><td>{cell}</td><td>y</td></tr></table>"),
            Kw::zero().links(links),
        )
        .unwrap();
        assert_eq!(output.lines().next_back(), Some(expected), "{id}");
    }
}

#[test]
fn test_markdown_empty_sup_sub_are_dropped() {
    // An empty sup/sub must be removed without taking the text after it (issue #889).
    // process_node() hands a textless element its tail as text, so without the removal in
    // convert_tags() the new marker would wrap the *following* words: a<sup>b</sup>
    let kw = Kw::zero().markdown();
    for tag in ["sup", "sub"] {
        let doc = format!("<html><body><article><p>a<{tag}></{tag}>b</p></article></body></html>");
        assert_eq!(extract(&doc, &kw).as_deref(), Some("ab"), "{tag}");
    }
    // a footnote marker whose only child is dropped must still not swallow the sentence
    let footnote = r#"<html><body><article><p>Fact<sup><img src="x.png"/></sup> follows here.</p></article></body></html>"#;
    assert_eq!(
        extract(footnote, &kw).as_deref(),
        Some("Fact follows here.")
    );
    // the tail survives in precision mode too, where CUT_EMPTY_ELEMS pruning would drop it
    let tailed = "<html><body><article><p>alpha<sup></sup>beta gamma.</p></article></body></html>";
    assert_eq!(extract(tailed, &kw).as_deref(), Some("alphabeta gamma."));
    assert_eq!(
        extract(tailed, &kw.focus(Focus::Precision)).as_deref(),
        Some("alphabeta gamma.")
    );
}
