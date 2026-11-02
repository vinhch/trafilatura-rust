use super::*;

// Reading a cfg file is upstream API (`use_config`, `settingsfile=`) that this crate does not offer,
// so the file-based assertions are not ported; `NEW_CONFIG` is built from the values of
// `newsettings.cfg`. The `extract_with_metadata` line is `test_config_loading_metadata_lines`.
#[test]
fn test_config_loading() {
    // a partial settings file: the key it sets overrides, the others keep their default
    let config = Config {
        min_extracted_size: 9999,
        ..Config::default()
    };
    let options = Extractor::new(
        config,
        Format::Txt,
        false,
        false,
        None,
        false,
        false,
        HashSet::new(),
        HashSet::new(),
    );
    assert_eq!(options.min_extracted_size, 9999);
    assert_eq!(options.min_output_size, Config::default().min_output_size);
    assert_eq!(options.min_output_size, 1);

    // the `settingsfile` argument is the same as a `config` here: newsettings.cfg sets
    // MIN_OUTPUT_SIZE and MIN_EXTRACTED_SIZE far above this doc's length
    let html =
        "<html><body><article><p>Short paragraph of content here.</p></article></body></html>";
    assert!(extract(html, &Kw::new()).is_some());
    assert!(extract(html, &Kw::new().config(new_config())).is_none());
}

// The metadata line of `test_config_loading`.
#[test]
fn test_config_loading_metadata_lines() {
    let html =
        "<html><body><article><p>Short paragraph of content here.</p></article></body></html>";
    assert!(extract_with_metadata(html, &Kw::new().config(new_config()), None).is_none());
}

// The first assert uses `method="html"`, which writes the same as the XML method on this input.
// The metadata lines are `test_html_conversion_metadata_lines`.
#[test]
fn test_html_conversion() {
    let source = r##"<xml>
    <list>
        <item>Item 1</item>
        <item>Item 2</item>
    </list>
    <p>Text</p>
    <head rend="h1">Heading 1</head>
    <head rend="h2">Heading 2</head>
    <head>No attribute</head>
    <hi rend="#i">Italic</hi>
    <hi rend="#b">Bold</hi>
    <hi>No rend</hi>
    <ref target="https://example.com">Link</ref>
    <ref>No href</ref>
</xml>"##;
    let mut tree = xml_tree(source);
    let root = tree.root();
    let copied = tree.copy(root);
    let html_tree = convert_to_html(&mut tree, copied, &Call::default()).unwrap();
    let expected_html = r#"<html><body>
    <ul>
        <li>Item 1</li>
        <li>Item 2</li>
    </ul>
    <p>Text</p>
    <h1>Heading 1</h1>
    <h2>Heading 2</h2>
    <h3>No attribute</h3>
    <i>Italic</i>
    <strong>Bold</strong>
    <i>No rend</i>
    <a href="https://example.com">Link</a>
    <a href="">No href</a>
</body></html>"#;
    assert_eq!(tree.tostring(html_tree, false, false), expected_html);

    let html = "<html><body><article><h1>Title</h1><p>Text.</p></article></body></html>";
    let expected_html = "<html>
  <body>
    <h1>Title</h1>
    <p>Text.</p>
  </body>
</html>";
    assert_eq!(
        extract(html, &Kw::zero().html()).as_deref(),
        Some(expected_html)
    );

    // end-to-end: an image reaches HTML output as <img> (not <graphic>)
    let doc = r#"<html><body><article><p>Body text here.</p><img src="pic.jpg" alt="a"/></article></body></html>"#;
    let out = extract(doc, &Kw::zero().html().images(true)).unwrap();
    assert!(out.contains(r#"<img src="pic.jpg" alt="a"/>"#));
    assert!(!out.contains("<graphic"));
}

// The metadata lines of `test_html_conversion`.
#[test]
fn test_html_conversion_metadata_lines() {
    let html = "<html><body><article><h1>Title 1</h1><p>Text.</p></article></body></html>";
    let expected_html = r#"<html>
  <head>
    <meta name="title" content="Title 1"/>
    <meta name="fingerprint" content="f6fd180b8fbe3670"/>
  </head>
  <body>
    <h1>Title 1</h1>
    <p>Text.</p>
  </body>
</html>"#;
    assert_eq!(
        extract_with_metadata(html, &Kw::zero().html(), None).as_deref(),
        Some(expected_html)
    );

    // list-valued metadata (tags, categories) is joined as in the XML output
    let html = r#"<html><head><meta name="keywords" content="k1"/><meta property="article:tag" content="k2"/></head><body><article><h1>Title 1</h1><p>Text.</p></article></body></html>"#;
    assert!(
        extract_with_metadata(html, &Kw::zero().html(), None)
            .unwrap()
            .contains(r#"<meta name="tags" content="k1;k2"/>"#)
    );
}

#[test]
fn test_tree_cleaning_recall_keeps_paragraphs() {
    // Recall undoes the deletions when they would remove every paragraph.
    let call = Call::default();
    let mut options = default_options();
    let has_paragraph = |options: &Extractor, doc: &str, cleaned: &[&str]| {
        let (mut tree, root) = lxml_fromstring(doc);
        let ctx = ctx_for(options, &call);
        let root = tree_cleaning_with(&mut tree, root, &ctx, cleaned, MANUALLY_STRIPPED).unwrap();
        tree.find(root, "p").is_some()
    };
    let doc = "<html><body><aside><p>only text</p></aside></body></html>";
    assert!(!has_paragraph(&options, doc, MANUALLY_CLEANED));
    options.focus = Focus::Recall;
    assert!(has_paragraph(&options, doc, MANUALLY_CLEANED));
    // a user-added "p" deletes the paragraphs themselves
    // (upstream monkeypatches MANUALLY_CLEANED to ["p"])
    let doc = "<html><body><div><p>only text</p></div></body></html>";
    assert!(has_paragraph(&options, doc, &["p"]));
}

#[test]
fn test_convert_tags_links_off_keeps_contained_links() {
    // Without links, only links inside div, li, p (or tables) stay as ref, the tree root does not count.
    let doc = "<div><a>root</a><span><a>loose</a></span><p><a>para</a></p><table><tr><td><a>cell</a></td></tr></table></div>";
    let call = Call::default();
    let mut options = default_options();
    options.links = false;
    options.tables = true;
    let ref_texts = |options: &Extractor| {
        let (mut tree, root) = lxml_fromstring(doc);
        convert_tags(&mut tree, root, &ctx_for(options, &call), None).unwrap();
        tree.iter(root, &["ref"])
            .map(|link| tree.node(link).text.clone().unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(ref_texts(&options), ["para", "cell"]);
    options.tables = false;
    assert_eq!(ref_texts(&options), ["para"]);
}
