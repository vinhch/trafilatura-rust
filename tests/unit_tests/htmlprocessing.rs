use super::*;

/// `etree.fromstring(source)[0]`: the tree and the first child of its root.
fn first_child_of(source: &str) -> (Tree, NodeId) {
    let tree = xml_tree(source);
    let child = tree.children(tree.root()).next().unwrap();
    (tree, child)
}

/// `.//div[@class='widget']`
fn widget_divs(
    tree: &Tree,
    context: NodeId,
    _call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    Ok(tree
        .iterdescendants(context, &["div"])
        .filter(|&div| tree.get(div, "class") == Some("widget"))
        .collect())
}

#[test]
fn test_htmlprocessing() {
    // the dummy tree exists only because `xml_to_text` takes `&mut Tree` even for `None`
    let mut empty = xml_tree("<doc/>");
    assert_eq!(
        xml_to_text(&mut empty, None, false, &Call::default()).unwrap(),
        ""
    );
    let mut options = default_options();
    options.tables = true;
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let mut tree = xml_tree("<html/>");
    let root = tree.root();
    assert_eq!(tree_cleaning(&mut tree, root, &ctx).unwrap(), root);
    let mut tree = xml_tree("<unwanted/>");
    let root = tree.root();
    prune_html(&mut tree, root, Focus::Balanced, &call).unwrap();
    let mut convert_options = default_options();
    convert_options.tables = true;
    convert_options.formatting = true;
    convert_options.images = true;
    convert_options.links = true;
    // html5 foster-parents the `a` out of the table (D-2); the assertions hold on either tree
    let (mut mydoc, mydoc_root) = lxml_fromstring(
        r#"<html><body><table><a href="">Link</a></table><img src="test.jpg"/><u>Underlined</u><tt>True Type</tt><sub>Text</sub><sup>Text</sup></body></html>"#,
    );
    convert_tags(
        &mut mydoc,
        mydoc_root,
        &ctx_for(&convert_options, &call),
        None,
    )
    .unwrap();
    assert!(mydoc.find(mydoc_root, "ref").is_some());
    assert!(mydoc.find(mydoc_root, "graphic").is_some());
    assert!(
        mydoc
            .iter(mydoc_root, &["hi"])
            .any(|hi| mydoc.get(hi, "rend") == Some("#t"))
    );
    assert!(mydoc.find(mydoc_root, "table").is_some());

    // multiple images inside a link must keep their original order after being
    // lifted out of the <ref> (addnext reverses order if iterated forward)
    let (mut multi_img, multi_root) = lxml_fromstring(
        r#"<html><body><a href="/x"><img src="a.jpg"/><img src="b.jpg"/><img src="c.jpg"/></a></body></html>"#,
    );
    convert_tags(
        &mut multi_img,
        multi_root,
        &ctx_for(&convert_options, &call),
        None,
    )
    .unwrap();
    let srcs: Vec<_> = multi_img
        .iter(multi_root, &["graphic"])
        .map(|graphic| multi_img.get(graphic, "src").unwrap().to_string())
        .collect();
    assert_eq!(srcs, ["a.jpg", "b.jpg", "c.jpg"]);

    // the tree_cleaning of upstream's next lines runs on the converted `mydoc`
    convert_options.tables = false;
    let cleaned = tree_cleaning(&mut mydoc, mydoc_root, &ctx_for(&convert_options, &call)).unwrap();
    assert!(mydoc.find(cleaned, "graphic").is_some());
    assert!(mydoc.find(cleaned, "table").is_none());

    // delete_element on an element without a parent leaves it unchanged
    let mut tree = xml_tree("<test/>");
    let root = tree.root();
    let before = dump(&tree, DumpMode::Exact);
    delete_element(&mut tree, root, true);
    assert_eq!(dump(&tree, DumpMode::Exact), before);

    // merge with parent function
    let mut element = xml_tree("<test/>");
    let root = element.root();
    merge_with_parent(&mut element, root, false, &call).unwrap();
    assert_eq!(element.tostring(root, false, false), "<test/>");

    let merge_spans = |html: &str| {
        let (mut mydoc, mydoc_root) = lxml_fromstring(html);
        let mut spans = LiveIter::new(&mydoc, mydoc_root, &["span"]);
        while let Some(span) = spans.next(&mydoc) {
            merge_with_parent(&mut mydoc, span, false, &call).unwrap();
        }
        mydoc.tostring(mydoc_root, false, false)
    };
    assert!(
        merge_spans("<html><body><p><span>A</span><span>B</span><span>C</span></p></body></html>")
            .contains("<p>A B C</p>")
    );
    assert!(
        merge_spans(
            "<html><body><p><span>A</span><span>B</span> tail<span>C</span></p></body></html>"
        )
        .contains("<p>A B tail C</p>")
    );

    // paywalls
    let html = r#"<html><body><main><p>1</p><p id="premium">2</p><p>3</p></main></body></html>"#;
    assert_eq!(extract(html, &Kw::zero()).unwrap(), "1\n3");
    // deviation: D-1, upstream asserts the same with `fast=False`
    // fencedframe
    let html = "<html><body><article><p>real</p><fencedframe><p>ad</p></fencedframe><p>more</p></article></body></html>";
    assert!(!extract(html, &Kw::zero()).unwrap().contains("ad"));
    // deviation: D-1, upstream asserts the same with `fast=False`
    let html = "<html><body><fencedframe><article><h1>ad</h1><p>buy now buy now buy now buy now</p></article></fencedframe></body></html>";
    // deviation: D-1, upstream runs this one with `fast=False`
    assert!(!extract(html, &Kw::zero()).unwrap().contains("ad"));
    assert_eq!(extract(html, &Kw::new()), None);

    // test tail of node deleted if set as text
    let (mut tree, node) = first_child_of("<div><p></p>tail</div>");
    process_node(&mut tree, node, &ctx);
    assert_eq!(tree.node(node).text.as_deref(), Some("tail"));
    assert_eq!(tree.node(node).tail, None);
    let (mut tree, node) = first_child_of("<list><item></item>text in tail</list>");
    process_node(&mut tree, node, &ctx);
    assert_eq!(tree.node(node).text.as_deref(), Some("text in tail"));
    assert_eq!(tree.node(node).tail, None);
    let (mut tree, line_break) = first_child_of("<p><lb/>tail</p>");
    process_node(&mut tree, line_break, &ctx);
    assert_eq!(tree.node(line_break).text, None);
    assert_eq!(tree.node(line_break).tail.as_deref(), Some("tail"));
    let (mut tree, node) = first_child_of("<div><p>some text</p>tail</div>");
    process_node(&mut tree, node, &ctx);
    assert_eq!(tree.node(node).text.as_deref(), Some("some text"));
    assert_eq!(tree.node(node).tail.as_deref(), Some("tail"));
    let (mut tree, node) =
        first_child_of("<p><ref target='url'><hi rend='#b'>bold</hi>inner</ref>outer</p>");
    let processed = handle_textnode(&mut tree, node, &ctx, true, false).unwrap();
    assert_eq!(tree.node(processed).tail.as_deref(), Some("outer"));
    let (mut tree, node) = first_child_of("<p><ref target='url'>text</ref>tail</p>");
    let processed = handle_textnode(&mut tree, node, &ctx, true, false).unwrap();
    assert_eq!(tree.node(processed).tail.as_deref(), Some("tail"));
    assert_eq!(tree.node(processed).text.as_deref(), Some("text"));
    let (mut tree, node) = first_child_of("<p><ref target='url'></ref>tail</p>");
    let processed = handle_textnode(&mut tree, node, &ctx, true, false).unwrap();
    assert_eq!(tree.node(processed).tail.as_deref(), Some(""));
    assert_eq!(tree.node(processed).text.as_deref(), Some("tail"));
    let (mut tree, node) =
        first_child_of("<p><ref target='url'>text<hi rend='#b'>bold</hi></ref>tail</p>");
    let processed = handle_textnode(&mut tree, node, &ctx, true, false).unwrap();
    assert_eq!(tree.node(processed).tail.as_deref(), Some("tail"));
    assert_eq!(tree.node(processed).text.as_deref(), Some("text"));

    // fix for bug 807
    let (mut tree, node) = lxml_fromstring("<div><p><span>span</span> span tail</p> p tail </div>");
    assert_eq!(tree.text_content(node), "span span tail p tail ");
    prune_unwanted_nodes(&mut tree, node, &[spans], false, &call).unwrap();
    assert_eq!(tree.text_content(node), " span tail p tail ");

    // link_density_test_tables: a text-rich table whose only <ref> links carry no text
    // (e.g. icon/flag links wrapping images) has zero link text -> not boilerplate, kept
    let linkless_table = xml_tree(&format!(
        r#"<table><cell>{}<ref target="/x"></ref></cell></table>"#,
        "word ".repeat(50)
    ));
    assert!(!link_density_test_tables(&linkless_table, linkless_table.root(), &call).unwrap());
    // short table (elemlen < 200) is never removed regardless of link ratio
    let short_table = xml_tree(&format!(
        r#"<table><cell>short {}</cell></table>"#,
        r#"<ref target="/x">link</ref> "#.repeat(5)
    ));
    assert!(!link_density_test_tables(&short_table, short_table.root(), &call).unwrap());
    // replace_element_text: an empty <ref> (no text) yields an empty string
    let mut empty_ref = xml_tree("<ref/>");
    let root = empty_ref.root();
    assert_eq!(
        replace_element_text(&mut empty_ref, root, false, false, &call).unwrap(),
        ""
    );

    // regression #797: a code block with a tailless <lb> must not emit a literal "None"
    let mut code = xml_tree("<code>print(1)<lb/></code>");
    let root = code.root();
    let code_md = replace_element_text(&mut code, root, true, false, &call).unwrap();
    assert!(!code_md.contains("None"));
    assert!(code_md.contains("print(1)"));

    // handle_paragraphs: a trailing <lb> with no tail is stripped from the output
    let mut tree = xml_tree("<p>text<lb>x</lb></p>");
    let para = tree.root();
    let processed = handle_paragraphs(&mut tree, para, &["p", "lb"], &ctx)
        .unwrap()
        .unwrap();
    assert!(tree.find(processed, "lb").is_none());

    // handle_paragraphs: non-INLINE_CARRIED children of <hi> are stripped with a leading space
    let mut formatting = default_options();
    formatting.formatting = true;
    let ctx = ctx_for(&formatting, &call);
    let mut tree = xml_tree(r##"<p><hi rend="#b">pre<quote>mid</quote>end</hi></p>"##);
    let para = tree.root();
    let processed = handle_paragraphs(&mut tree, para, TAG_CATALOG, &ctx)
        .unwrap()
        .unwrap();
    let hi = tree
        .children(processed)
        .find(|&c| tree.node(c).name == *"hi")
        .unwrap();
    let text = tree.node(hi).text.as_deref().unwrap();
    assert!(text.contains("pre"));
    assert!(text.contains(" mid"));

    let mut tree = xml_tree(r##"<p><hi rend="#b">start<lb/>tail</hi></p>"##);
    let para = tree.root();
    let processed = handle_paragraphs(&mut tree, para, TAG_CATALOG, &ctx)
        .unwrap()
        .unwrap();
    let hi = tree
        .children(processed)
        .find(|&c| tree.node(c).name == *"hi")
        .unwrap();
    assert!(tree.node(hi).text.as_deref().unwrap().contains(" tail"));
}

// The XML lines of `test_htmlprocessing`.
#[test]
fn test_htmlprocessing_phase_6_lines() {
    let html = "<html><body><article><h1>Test headline</h1><p>Test</p></article></body></html>";
    assert!(
        extract_xml_tree(html, &Kw::zero())
            .unwrap()
            .contains(r#"<head rend="h1">Test headline</head>"#)
    );
}

// The TEI line of `test_htmlprocessing`: the XML-TEI output comes with Task 6.
#[test]
#[ignore = "phase 6"]
fn test_htmlprocessing_tei_phase_6_lines() {
    let html = "<html><body><article><h1>Test headline</h1><p>Test</p></article></body></html>";
    assert!(
        extract_xmltei_tree(html, &Kw::zero())
            .unwrap()
            .contains(r#"<ab rend="h1" type="header">Test headline</ab>"#)
    );
}

#[test]
fn test_prune_unwanted_nodes_guard() {
    // An over-pruning trip leaves the same subtree in place; upstream's `result is node` is that
    // the div is still a child of body.
    let call = Call::default();
    for (widget_words, pruned) in [(300, false), (3, true)] {
        let (mut tree, doc) = lxml_fromstring(&format!(
            "<html><body><div><p>real text</p><div class='widget'>{}</div></div></body></html>",
            "x ".repeat(widget_words)
        ));
        let node = tree.find(doc, "div").unwrap();
        let before = tree.text_content(doc);
        prune_unwanted_nodes(&mut tree, node, &[widget_divs], true, &call).unwrap();
        assert_eq!(
            tree.node(node).parent(),
            Some(tree.find(doc, "body").unwrap())
        );
        assert_eq!(tree.text_content(doc) != before, pruned);
    }
}

#[test]
fn test_extract_restored_subtree_not_extracted_twice() {
    // A later BODY_XPATH pass sees the restored subtree with its consumed paragraph marked done.
    let hidden = format!(
        "<div style='display:none'>{}</div>",
        "Hidden block sentence. ".repeat(30)
    );
    let page = format!(
        "<html><body><main id='content'><article><p>Lead paragraph of the story.</p>{hidden}</article><p>Outside paragraph one.</p></main></body></html>"
    );
    let result = extract(&page, &Kw::zero()).unwrap();
    assert_eq!(result.matches("Lead paragraph of the story.").count(), 1);
    assert!(result.contains("Outside paragraph one."));
}
