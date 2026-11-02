use super::*;

/// The result of `handle_paragraphs(source, potential_tags)` and the source after the call.
fn paragraph_in(source: &str, potential_tags: &[&str]) -> (String, String) {
    let mut tree = xml(source);
    let root = tree.root();
    let options = options();
    let call = Call::default();
    let processed = handle_paragraphs(&mut tree, root, potential_tags, &ctx_for(&options, &call))
        .unwrap()
        .unwrap();
    (show(&tree, processed), show(&tree, root))
}

/// The result of `handle_paragraphs(source, TAG_CATALOG)` and the source after the call.
fn paragraph(source: &str) -> (String, String) {
    paragraph_in(source, TAG_CATALOG)
}

#[test]
fn quote_tags_are_the_catalog_plus_ref_and_graphic() {
    let mut expected: Vec<&str> = TAG_CATALOG.to_vec();
    expected.extend(["ref", "graphic"]);
    assert_eq!(QUOTE_TAGS, expected);
}

#[test]
fn paragraph_walk_stops_after_stripped_hi() {
    // oracle: etree.tostring(handle_paragraphs(etree.fromstring(s), TAG_CATALOG, Extractor()))
    // for s = '<p>A <hi>b<lb/>c</hi> e <hi>f</hi> g</p>'; the source is left as
    // <done>A <done>b c</done> e <hi>f</hi> g</done>
    let (processed, source) = paragraph("<p>A <hi>b<lb/>c</hi> e <hi>f</hi> g</p>");
    assert_eq!(processed, "<p>A <hi>b c</hi> e </p>");
    assert_eq!(source, "<done>A <done>b c</done> e <hi>f</hi> g</done>");
    // oracle: the same for '<p>A <hi>b</hi> e <hi>f</hi> g</p>'
    let (processed, source) = paragraph("<p>A <hi>b</hi> e <hi>f</hi> g</p>");
    assert_eq!(processed, "<p>A <hi>b</hi> e <hi>f</hi> g</p>");
    assert_eq!(source, "<done>A <done>b</done> e <done>f</done> g</done>");
}

#[test]
fn paragraph_strips_foreign_descendants() {
    // oracle: dump_tree(handle_paragraphs(p, TAG_CATALOG, Extractor())) with
    // p = parse_document("<p>a <svg><title>t</title></svg> b</p>").find('.//p'): 'p', '#text "a t b"';
    // dump_tree(doc) then has the document as html > head, body > done > #text "a t b"
    let mut tree = parse_document(
        "<p>a <svg><title>t</title></svg> b</p>",
        &ParseLimits::default(),
        &Call::default(),
    )
    .unwrap();
    let p = tree.find(tree.root(), "p").unwrap();
    let options = options();
    let call = Call::default();
    let processed = handle_paragraphs(&mut tree, p, TAG_CATALOG, &ctx_for(&options, &call))
        .unwrap()
        .unwrap();
    assert_eq!(
        dump_from(&tree, processed, DumpMode::Exact).0,
        "p\n  #text \"a t b\"\n"
    );
    assert_eq!(
        dump(&tree, DumpMode::Exact).0,
        "html\n  head\n  body\n    done\n      #text \"a t b\"\n"
    );
}

#[test]
fn textnode_comments_fix_false_keeps_empty_lb_before_inline() {
    let options = options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    // oracle: handle_textnode(p[0], Extractor(), comments_fix=fix, preserve_spaces=False) on
    // etree.fromstring('<p><lb/><hi>x</hi></p>'): the lb for fix False, None for True
    let mut tree = xml("<p><lb/><hi>x</hi></p>");
    let lb = tree.first_child(tree.root()).unwrap();
    assert_eq!(handle_textnode(&mut tree, lb, &ctx, false, false), Some(lb));
    assert_eq!(handle_textnode(&mut tree, lb, &ctx, true, false), None);
    // oracle: etree.tostring(handle_paragraphs(etree.fromstring('<p>a<lb/><hi>x</hi></p>'), TAG_CATALOG, ...))
    assert_eq!(
        paragraph("<p>a<lb/><hi>x</hi></p>").0,
        "<p>a<lb/><hi>x</hi></p>"
    );
}

#[test]
fn paragraph_empty_join_is_empty_text_node() {
    // oracle: p = etree.fromstring('<div><p><hi>Bold</hi> rest</p>Some tail text here</div>')[0];
    // r = handle_paragraphs(p, TAG_CATALOG, Extractor()); r.text == '' and
    // list(r.itertext()) == ['', 'Bold', ' rest']
    let mut tree = xml("<div><p><hi>Bold</hi> rest</p>Some tail text here</div>");
    let p = tree.first_child(tree.root()).unwrap();
    let options = options();
    let call = Call::default();
    let processed = handle_paragraphs(&mut tree, p, TAG_CATALOG, &ctx_for(&options, &call))
        .unwrap()
        .unwrap();
    assert_eq!(tree.node(processed).text.as_deref(), Some(""));
    assert_eq!(
        tree.itertext_lxml(processed).collect::<Vec<_>>(),
        ["", "Bold", " rest"]
    );
}

#[test]
fn paragraph_strips_children_tags_at_every_depth() {
    // oracle: handle_paragraphs(etree.fromstring(s), TAG_CATALOG, Extractor()) for
    // s = '<p>x<hi>a<quote>b<hi>c</hi></quote></hi>z</p>': <p>x<hi>a b</hi>z</p>, and the source
    // is left as <done>x<done>a b<hi>c</hi></done>z</done>
    let (processed, source) = paragraph("<p>x<hi>a<quote>b<hi>c</hi></quote></hi>z</p>");
    assert_eq!(processed, "<p>x<hi>a b</hi>z</p>");
    assert_eq!(source, "<done>x<done>a b<hi>c</hi></done>z</done>");
}

#[test]
fn paragraph_walk_checks_cancellation() {
    let mut tree = xml("<p>a<hi>b</hi></p>");
    let root = tree.root();
    let options = options();
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    assert_eq!(
        handle_paragraphs(&mut tree, root, TAG_CATALOG, &ctx_for(&options, &call)),
        Err(ExtractError::Cancelled)
    );
}

#[test]
fn code_block_needs_direct_code_child() {
    // oracle: is_code_block_element(el) for each source below
    let check = |source: &str, on_child: bool| {
        let tree = xml(source);
        let element = if on_child {
            tree.first_child(tree.root()).unwrap()
        } else {
            tree.root()
        };
        is_code_block_element(&tree, element)
    };
    assert!(!check("<quote><p><code>x</code></p></quote>", false));
    assert!(check("<quote><code>x</code></quote>", false));
    assert!(check("<quote> <code>x</code> </quote>", false));
    assert!(!check("<quote><code>x</code>tail</quote>", false));
    assert!(!check("<quote>t<code>x</code></quote>", false));
    assert!(!check("<quote><code>x</code><hi>y</hi></quote>", false));
    assert!(check("<quote lang=\"py\">x</quote>", false));
    assert!(check(
        "<div class=\"highlight\"><quote>x</quote></div>",
        true
    ));
    assert!(!check("<div class=\"x\"><quote>x</quote></div>", true));
}

/// `handle_image` on `<graphic src=.. alt="a" title="t"/>` with the tail `tl`.
fn image(src: &str, url: Option<&str>) -> Result<Option<String>, ExtractError> {
    let mut tree = xml(&format!("<graphic src=\"{src}\" alt=\"a\" title=\"t\"/>"));
    let root = tree.root();
    tree.node_mut(root).tail = Some("tl".to_string());
    let mut options = options();
    options.url = url.map(str::to_string);
    let processed = handle_image(&mut tree, root, Some(&options))?;
    Ok(processed.map(|id| {
        format!(
            "{}{}",
            show(&tree, id),
            tree.node(id).tail.as_deref().unwrap()
        )
    }))
}

#[test]
fn image_urljoin_error_is_invalid_value() {
    // oracle: etree.tostring(handle_image(el, options)) with options.url = url
    assert_eq!(
        image("//[::1/x.jpg", Some("https://example.org/")),
        Err(ExtractError::Rejected(Rejection::InvalidValue))
    );
    assert_eq!(
        image("/x.jpg", Some("https://example.org/"))
            .unwrap()
            .as_deref(),
        Some("<graphic src=\"https://example.org/x.jpg\" alt=\"a\" title=\"t\"/>tl")
    );
    assert_eq!(
        image("//x/y.jpg", None).unwrap().as_deref(),
        Some("<graphic src=\"http://x/y.jpg\" alt=\"a\" title=\"t\"/>tl")
    );
    // a source starting with "http" is kept, url or not; a relative one without url stays as is
    assert_eq!(
        image("httpfoo.jpg", Some("https://example.org/"))
            .unwrap()
            .as_deref(),
        Some("<graphic src=\"httpfoo.jpg\" alt=\"a\" title=\"t\"/>tl")
    );
    assert_eq!(
        image("x.jpg", None).unwrap().as_deref(),
        Some("<graphic src=\"x.jpg\" alt=\"a\" title=\"t\"/>tl")
    );
}

#[test]
fn titles_copy_drops_done_children() {
    // oracle: r = handle_titles(e, Extractor()) for e = etree.fromstring(
    // '<head>T<done>old</done>X<hi>new</hi>tail</head>'): <head>T<hi>new</hi>tail</head>, and the
    // source is left as <head>T<done>old</done>X<done>new</done>tail</head>
    let mut tree = xml("<head>T<done>old</done>X<hi>new</hi>tail</head>");
    let root = tree.root();
    let options = options();
    let call = Call::default();
    let title = handle_titles(&mut tree, root, &ctx_for(&options, &call))
        .unwrap()
        .unwrap();
    assert_eq!(show(&tree, title), "<head>T<hi>new</hi>tail</head>");
    assert_eq!(
        show(&tree, root),
        "<head>T<done>old</done>X<done>new</done>tail</head>"
    );
    // oracle: handle_titles(etree.fromstring('<head><done>Lead. </done>Teaser</head>'), ...) is None
    let mut tree = xml("<head><done>Lead. </done>Teaser</head>");
    let root = tree.root();
    assert_eq!(
        handle_titles(&mut tree, root, &ctx_for(&options, &call)),
        Ok(None)
    );
}

#[test]
fn other_elements_w3_code_becomes_code() {
    let options = options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    // oracle: r = handle_other_elements(e, {"div"}, Extractor()) for
    // e = etree.fromstring('<div class="w3-code notranslate">x<p>y</p>z</div>'):
    // <code class="w3-code notranslate">x<p>y</p>z</code>, source <done class="w3-code notranslate">x<done>y</done>z</done>
    let mut tree = xml("<div class=\"w3-code notranslate\">x<p>y</p>z</div>");
    let root = tree.root();
    let code = handle_other_elements(&mut tree, root, &["div"], &ctx)
        .unwrap()
        .unwrap();
    assert_eq!(
        show(&tree, code),
        "<code class=\"w3-code notranslate\">x<p>y</p>z</code>"
    );
    assert_eq!(
        show(&tree, root),
        "<done class=\"w3-code notranslate\">x<done>y</done>z</done>"
    );
    // oracle: a w3-code div is a code block even with an empty potential_tags
    let mut tree = xml("<div class=\"w3-code\">x</div>");
    let root = tree.root();
    let code = handle_other_elements(&mut tree, root, &[], &ctx)
        .unwrap()
        .unwrap();
    assert_eq!(show(&tree, code), "<code class=\"w3-code\">x</code>");
    // oracle: a div in potential_tags becomes a p without attributes; otherwise, and for any other
    // tag, None with the source untouched
    let mut tree = xml("<div id=\"a\" class=\"b\"> x </div>");
    let root = tree.root();
    let paragraph = handle_other_elements(&mut tree, root, &["div"], &ctx)
        .unwrap()
        .unwrap();
    assert_eq!(show(&tree, paragraph), "<p> x </p>");
    let mut tree = xml("<div id=\"a\"> x </div>");
    let root = tree.root();
    assert_eq!(handle_other_elements(&mut tree, root, &[], &ctx), Ok(None));
    assert_eq!(show(&tree, root), "<div id=\"a\"> x </div>");
    let mut tree = xml("<section>x</section>");
    let root = tree.root();
    assert_eq!(
        handle_other_elements(&mut tree, root, &["div"], &ctx),
        Ok(None)
    );
}

#[test]
fn paragraph_nested_inline_carry() {
    // oracle: etree.tostring(handle_paragraphs(etree.fromstring(s), TAG_CATALOG, Extractor())) for
    // s = '<p>a<hi>b<hi>c</hi>d</hi></p>': the same markup, and the source is left as
    // <done>a<done>b<done>c</done>d</done></done>
    let (processed, source) = paragraph("<p>a<hi>b<hi>c</hi>d</hi></p>");
    assert_eq!(processed, "<p>a<hi>b<hi>c</hi>d</hi></p>");
    assert_eq!(source, "<done>a<done>b<done>c</done>d</done></done>");
    // oracle: the same for '<p>a<hi>b<hi>c</hi>d</hi>e</p>'
    assert_eq!(
        paragraph("<p>a<hi>b<hi>c</hi>d</hi>e</p>").0,
        "<p>a<hi>b<hi>c</hi>d</hi>e</p>"
    );
    // oracle: the same for '<p>a<ref target="t">r<hi>x</hi></ref>b</p>' with {*TAG_CATALOG, "ref"}
    let mut tags = TAG_CATALOG.to_vec();
    tags.push("ref");
    assert_eq!(
        paragraph_in("<p>a<ref target=\"t\">r<hi>x</hi></ref>b</p>", &tags).0,
        "<p>a<ref target=\"t\">r<hi>x</hi></ref>b</p>"
    );
}

#[test]
fn paragraph_graphic_is_rebuilt_by_handle_image() {
    // oracle: etree.tostring(handle_paragraphs(etree.fromstring(s), {*TAG_CATALOG, "graphic"},
    // Extractor())) for s = '<p>a<graphic src="x.jpg" class="c"/>b</p>': the image is rebuilt with
    // src only and keeps its tail; the source is left as <done>a<done src="x.jpg" class="c"/>b</done>
    let mut tags = TAG_CATALOG.to_vec();
    tags.push("graphic");
    let (processed, source) = paragraph_in("<p>a<graphic src=\"x.jpg\" class=\"c\"/>b</p>", &tags);
    assert_eq!(processed, "<p>a<graphic src=\"x.jpg\"/>b</p>");
    assert_eq!(source, "<done>a<done src=\"x.jpg\" class=\"c\"/>b</done>");
}
