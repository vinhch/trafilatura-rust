use super::*;

#[test]
fn id_and_fingerprint_only_outside_txt() {
    let main = main_text();
    let page = main_page(COMMENT_DIV);
    // oracle: d = core._internal_extraction(page, record_id="7", options=Extractor(output_format=f,
    //   fast=True, dedup=False)) inside html5_parsers(); d.text, d.id, d.fingerprint is None
    // (format, text, id, fingerprint set)
    let rows = [
        (
            Format::Txt,
            format!("{main}\nOne comment here\nSecond reply here"),
            None,
            false,
        ),
        (
            Format::Markdown,
            format!("{main}\n\nOne comment here\n\nSecond reply here"),
            None,
            false,
        ),
        (
            Format::Html,
            format!("<html>\n  <body>\n    <p>{main}</p>\n  </body>\n</html>"),
            Some("7"),
            true,
        ),
    ];
    for (format, text, id, fingerprint) in rows {
        let doc = internal_page(&page, Some("7"), &bare_options(format, true)).unwrap();
        assert_eq!(doc.text.as_deref(), Some(text.as_str()), "{format:?}");
        assert_eq!(doc.id.as_deref(), id, "{format:?}");
        assert_eq!(doc.fingerprint.is_some(), fingerprint, "{format:?}");
        assert_eq!(doc.raw_text.as_deref(), Some(main.as_str()), "{format:?}");
    }
    // oracle: core._internal_extraction(page, options=Extractor(output_format="html", fast=True,
    //   dedup=False)): d.id is None and d.fingerprint is not None
    let doc = internal_page(&page, None, &bare_options(Format::Html, true)).unwrap();
    assert_eq!(doc.id, None);
    assert!(doc.fingerprint.is_some());
}

#[test]
fn caller_tree_untouched() {
    let page = main_page(COMMENT_DIV);
    let tree = load(&page);
    let root = tree.root();
    let before = tree.tostring(root, false, true);
    let options = bare_options(Format::Python, true);
    let from_tree = bare(Source::Tree(&tree, root), &options).unwrap();
    // oracle: before = etree.tostring(tree); core.bare_extraction(tree, options=o) inside
    //   html5_parsers(); etree.tostring(tree) == before is True
    assert_eq!(tree.tostring(root, false, true), before);
    // oracle: the Document of a tree source equals that of the string source: text, comments
    let from_input = bare_page(&page, &options).unwrap();
    assert_eq!(from_tree.text, from_input.text);
    assert_eq!(
        from_tree.comments.as_deref(),
        Some("One comment here\nSecond reply here")
    );
    assert_eq!(body_xml(&from_tree), body_xml(&from_input));
}

#[test]
fn deep_caller_tree_on_small_stack() {
    // oracle: tree = html.fromstring("<html><body><p>root text</p></body></html>"); node = the
    //   body; 3000 times node = SubElement(node, "div"); node.text = DEEP; core.bare_extraction(tree,
    //   options=Extractor(output_format=f, fast=True, dedup=False) with min_output_size and
    //   min_extracted_size 0): d.text, etree.tostring(d.body) (depth 2: the body and its paragraphs)
    const DEEP: &str =
        "Deep text long enough to be kept by the extractor, appearing at the very bottom.";
    let outcome = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            let mut tree = Tree::new(LocalName::from("html"), Ns::Html, Vec::new());
            let root = tree.root();
            let body = tree.new_element("body");
            tree.append(root, body);
            let p = element(&mut tree, "p", Some("root text"), None);
            tree.append(body, p);
            let mut node = body;
            for _ in 0..3000 {
                let div = tree.new_element("div");
                tree.append(node, div);
                node = div;
            }
            tree.node_mut(node).text = Some(DEEP.to_owned());
            let mut options = bare_options(Format::Python, true);
            options.min_output_size = 0;
            options.min_extracted_size = 0;
            let doc = bare(Source::Tree(&tree, root), &options).unwrap();
            let mut txt = bare_options(Format::Txt, true);
            txt.min_output_size = 0;
            txt.min_extracted_size = 0;
            let call = Call::default();
            let ctx = Ctx {
                options: Cow::Borrowed(&txt),
                call: &call,
            };
            let rendered = internal_extraction(
                Source::Tree(&tree, root),
                None,
                &ctx,
                &ParseLimits::default(),
            )
            .unwrap();
            (body_xml(&doc), doc.text, rendered.text)
        })
        .unwrap()
        .join()
        .unwrap();
    let expected_text = format!("root text\n{DEEP}");
    assert_eq!(
        outcome.0,
        format!("<body><p>root text</p><p>{DEEP}</p></body>")
    );
    assert_eq!(outcome.1.as_deref(), Some(expected_text.as_str()));
    // oracle: core.extract(tree, options=Extractor(output_format="txt", ...)) == d.text
    assert_eq!(outcome.2.as_deref(), Some(expected_text.as_str()));
}

#[test]
fn prepare_tree_copies_backup_only_when_asked() {
    const PAGE: &str = "<html><body><article><p>Main text of the article</p><script>var a = 1;</script></article></body></html>";
    let options = cascade_options(false, false, false);
    let call = Call::default();
    let ctx = Ctx {
        options: Cow::Borrowed(&options),
        call: &call,
    };
    let prepare = |keep_backup: bool| {
        let mut tree = load(PAGE);
        let root = tree.root();
        let (cleaned, backup) = prepare_tree(&mut tree, root, &ctx, None, keep_backup).unwrap();
        let dump = crate::dom::dump_from(&tree, cleaned, crate::dom::DumpMode::Exact).0;
        (tree.len(), backup.is_some(), dump)
    };
    let (kept_len, kept, kept_dump) = prepare(true);
    let (dropped_len, dropped, dropped_dump) = prepare(false);
    assert!(kept);
    assert!(!dropped);
    assert!(kept_len > dropped_len);
    assert_eq!(kept_dump, dropped_dump);
}
