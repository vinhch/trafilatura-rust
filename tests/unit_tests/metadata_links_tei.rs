use super::*;

/// `extract(html, url, output_format="xmltei", **kw)`.
fn extract_xmltei(html: &str, kw: &Kw) -> Option<String> {
    xmltei_output(Source::Input(HtmlInput::Str(html)), kw, None)
}

/// `extract(html.fromstring(html), output_format="xml", with_metadata=True, **kw)`.
fn extract_xml_with_metadata_tree(html: &str, kw: &Kw) -> Option<String> {
    let (tree, root) = lxml_fromstring(html);
    internal_document(Source::Tree(&tree, root), kw, Format::Xml, true, None)
        .map(|document| document.text.unwrap_or_default())
}

/// `xml.validate_tei(etree.fromstring(xml))`. The validator is not ported (D-27), so the
/// assertions that use it are in the `#[ignore = "D-27"]` test.
fn validate_tei(_xml: &str) -> bool {
    false
}

/// `bare_extraction(html, with_metadata=True, record_id=..., **kw)`: the document with its metadata.
fn metadata_document(html: &str, kw: &Kw, record_id: Option<&str>) -> Option<Document> {
    or_none(
        kw.options()
            .with_metadata(true)
            .build()
            .bare_extraction(&input_of(html, kw, record_id), &Call::default()),
    )
}

/// The document of `extract_with_metadata(html.fromstring(html), output_format="xml", **kw)`, which
/// upstream returns for the XML output; its `raw_text` and `fingerprint` are read from it.
fn extract_xml_document(html: &str, kw: &Kw) -> Option<Document> {
    let (tree, root) = lxml_fromstring(html);
    internal_document(Source::Tree(&tree, root), kw, Format::Xml, true, None)
}

/// `handle_image(html.fromstring(html))`: the tree and the new element, `None` for no image.
fn image_of(html: &str) -> Option<(Tree, NodeId)> {
    let (mut tree, img) = lxml_fromstring(html);
    let image = handle_image(&mut tree, img, None).unwrap()?;
    Some((tree, image))
}

/// `tree.find(".//{parent}/{child}") is not None`.
fn has_child_path(tree: &Tree, root: NodeId, parent: &str, child: &str) -> bool {
    tree.iterdescendants(root, &[child])
        .any(|node| tree.node(node).parent().and_then(|up| tree.tag(up)) == Some(parent))
}

/// `sanitize(etree.tostring(remove_empty_elements(strip_double_tags(tree)))).replace("\n", "")`.
fn strip_double_tags_then_clean(tree: &mut Tree, root: NodeId, call: &Call<'_>) -> String {
    strip_double_tags(tree, root, call).unwrap();
    remove_empty_elements(tree, root, call).unwrap();
    sanitize(&tree.tostring(root, false, false), false, false)
        .unwrap()
        .replace('\n', "")
}

/// The source of the last block of upstream's `test_tei`: elements with the same tag nested in an
/// element of `NESTING_WHITELIST`.
const WHITELISTED_NESTING: &str = "<html>
        <body>
            <div>
                <p>
                  <list>
                    <item>
                        <p>text</p>
                    </item>
                  </list>
                </p>
                <p>
                    <table>
                      <row>
                        <cell>
                          <p>text1</p>
                         </cell>
                      </row>
                    </table>
                </p>
                <p>
                    <note>
                      <p>text2</p>
                    </note>
                </p>
                <p>
                    <quote>
                        <p>text3</p>
                    </quote>
                </p>
                <p>
                    <figure>
                        <p>text4</p>
                    </figure>
                </p>
            </div>
        </body>
    </html>";

/// `[tree.find(f".//{parent_tag}/p") is not None for parent_tag in [...]]`.
fn whitelisted_parents_with_p(tree: &Tree, root: NodeId) -> Vec<bool> {
    ["item", "cell", "quote", "note", "figure"]
        .iter()
        .map(|parent_tag| has_child_path(tree, root, parent_tag, "p"))
        .collect()
}

#[test]
fn test_extract_with_metadata() {
    // oracle: extract_with_metadata(my_document, output_format="txt", include_formatting=True,
    // fast=True, url=url); the XML lines are in `test_extract_with_metadata_xml_output`
    let url = "http://aa.bb/cc.html";
    let html =
        "<html><head></head><body><article><p>AAA, <p>BBB</p>, CCC.</p></article></body></html>";
    let kw = Kw::new()
        .format(ApiFormat::Python)
        .formatting(true)
        .url(url);
    let parsed = metadata_document(html, &kw, None).unwrap();
    let content = parsed.text.clone().unwrap_or_default();
    assert!(content.contains("AAA"));
    assert!(content.contains("BBB"));
    assert!(content.contains("CCC"));
    assert_eq!(parsed.url.as_deref(), Some(url));
    assert_eq!(parsed.date, None);
    assert_eq!(parsed.title, None);

    let html = "<html><head><title>title</title></head><body><article><div>May 24, 2021</div><p>AAA, <p>BBB</p>, CCC.</p></article></body></html>";
    let parsed = metadata_document(html, &kw, None).unwrap();
    let content = parsed.text.clone().unwrap_or_default();
    assert!(content.contains("AAA"));
    assert!(content.contains("BBB"));
    assert!(content.contains("CCC"));
    assert_eq!(parsed.url.as_deref(), Some(url));
    assert_eq!(parsed.date.as_deref(), Some("2021-05-24"));
    assert_eq!(parsed.title.as_deref(), Some("title"));

    // oracle: extract_with_metadata(my_document, output_format="python") raises ValueError
    assert_eq!(
        Kw::new()
            .format(ApiFormat::Python)
            .options()
            .build()
            .extract(&Input::text(html), &Call::default()),
        Err(ExtractError::Rejected(Rejection::InvalidValue))
    );

    // oracle: extract_with_metadata(my_document, target_language="en", fast=True) is None
    let html = r#"<html><head><meta http-equiv="content-language" content="es"></head><body><article><p>AAA, <p>BBB</p>, CCC.</p></article></body></html>"#;
    assert_eq!(
        Options::new()
            .with_metadata(true)
            .target_language("en")
            .build()
            .bare_extraction(&Input::text(html), &Call::default())
            .err(),
        Some(ExtractError::Rejected(Rejection::WrongHtmlLang))
    );
}

// The XML lines of `test_extract_with_metadata`.
#[test]
fn test_extract_with_metadata_xml_output() {
    // upstream's `my_document`: the title and the date feed the record's fingerprint
    let html = r#"<html>
        <head><title>title</title></head>
        <body>
        <article>
        <div>May 24, 2021</div>
        <p>AAA, <p>BBB</p>, CCC.</p>
        </article>
        </body>
        </html>
    "#;
    // Upstream's assertion: extract_with_metadata(my_document, output_format="xml", config=ZERO_CONFIG)
    let parsed = extract_xml_document(html, &Kw::zero()).unwrap();
    assert_eq!(parsed.raw_text.as_deref(), Some("AAA, BBB , CCC."));
    assert_eq!(parsed.fingerprint.as_deref(), Some("ee7d2fb6fcf2837d"));
    let content = parsed.text.unwrap_or_default();
    assert!(content.contains("AAA"));
    assert!(content.contains("BBB"));
    assert!(content.contains("CCC"));
}

#[test]
fn test_images() {
    // `is_image_file` is ported with `utils`; the XML line is `test_images_xml_output`
    // tag with attributes
    assert!(image_of(r#"<img src="test.jpg"/>"#).is_some());
    assert!(image_of(r#"<img data-src="test.jpg" alt="text" title="a title"/>"#).is_some());
    assert!(image_of(r#"<img other="test.jpg"/>"#).is_none());

    // HTML conversion
    let options = default_options();
    let call = Call::default();
    let mut tree = xml_tree("<graphic/>");
    let root = tree.root();
    assert_eq!(
        handle_textelem(&mut tree, root, &[], &ctx_for(&options, &call)),
        Ok(None)
    );

    // a data: URI is no image file
    assert!(
        image_of(
            r#"<img src="data:image/jpeg;base64,iVBORw0KGgoAAAANSUhEUgAAAAUAAAAFCAYAAACNbyblAAAAHElEQVQI12P4//8/w38GIAXDIBKE0DHxgljNBAAO9TXL0Y4OHwAAAABJRU5ErkJggg==" alt="text"></img>"#
        )
        .is_none()
    );

    // CNN example
    let (tree, myimage) = image_of(
        r#"<img class="media__image media__image--responsive" alt="Harry and Meghan last March, in their final royal engagement." data-src-mini="//cdn.cnn.com/cnnnext/dam/assets/210307091919-harry-meghan-commonwealth-day-small-169.jpg" data-src-xsmall="//cdn.cnn.com/cnnnext/dam/assets/210307091919-harry-meghan-commonwealth-day-medium-plus-169.jpg" data-src-small="//cdn.cnn.com/cnnnext/dam/assets/210307091919-harry-meghan-commonwealth-day-large-169.jpg" data-src-medium="//cdn.cnn.com/cnnnext/dam/assets/210307091919-harry-meghan-commonwealth-day-exlarge-169.jpg" data-src-large="//cdn.cnn.com/cnnnext/dam/assets/210307091919-harry-meghan-commonwealth-day-super-169.jpg" data-src-full16x9="//cdn.cnn.com/cnnnext/dam/assets/210307091919-harry-meghan-commonwealth-day-full-169.jpg" data-src-mini1x1="//cdn.cnn.com/cnnnext/dam/assets/210307091919-harry-meghan-commonwealth-day-small-11.jpg" data-demand-load="loaded" data-eq-pts="mini: 0, xsmall: 221, small: 308, medium: 461, large: 781" src="//cdn.cnn.com/cnnnext/dam/assets/210307091919-harry-meghan-commonwealth-day-exlarge-169.jpg" data-eq-state="mini xsmall small medium" data-src="//cdn.cnn.com/cnnnext/dam/assets/210307091919-harry-meghan-commonwealth-day-exlarge-169.jpg">"#,
    )
    .unwrap();
    assert!(tree.get(myimage, "alt").is_some());
    assert!(tree.get(myimage, "src").is_some());
    // modified CNN example: only the `data-src-*` attributes remain
    let (tree, myimage) = image_of(
        r#"<img class="media__image media__image--responsive" alt="Harry and Meghan last March, in their final royal engagement." data-src-mini="//cdn.cnn.com/cnnnext/dam/assets/210307091919-harry-meghan-commonwealth-day-small-169.jpg" data-src-xsmall="//cdn.cnn.com/cnnnext/dam/assets/210307091919-harry-meghan-commonwealth-day-medium-plus-169.jpg" data-src-small="//cdn.cnn.com/cnnnext/dam/assets/210307091919-harry-meghan-commonwealth-day-large-169.jpg" data-src-medium="//cdn.cnn.com/cnnnext/dam/assets/210307091919-harry-meghan-commonwealth-day-exlarge-169.jpg" data-src-large="//cdn.cnn.com/cnnnext/dam/assets/210307091919-harry-meghan-commonwealth-day-super-169.jpg" data-src-full16x9="//cdn.cnn.com/cnnnext/dam/assets/210307091919-harry-meghan-commonwealth-day-full-169.jpg" data-src-mini1x1="//cdn.cnn.com/cnnnext/dam/assets/210307091919-harry-meghan-commonwealth-day-small-11.jpg" data-demand-load="loaded" data-eq-pts="mini: 0, xsmall: 221, small: 308, medium: 461, large: 781">"#,
    )
    .unwrap();
    assert!(tree.get(myimage, "alt").is_some());
    assert!(tree.get(myimage, "src").unwrap().starts_with("http"));

    if let Some(page) = resource_text("http_sample.html") {
        assert!(
            !extract(&page, &Kw::new())
                .unwrap()
                .contains("![Example image](test.jpg)")
        );
        assert!(
            extract(&page, &Kw::new().images(true))
                .unwrap()
                .contains("![Example image](test.jpg)")
        );
    }

    let img = |body: &str, kw: Kw| extract_doc(body, false, &kw.images(true));
    let a_title_text = Some("![a title text](test.jpg)".to_string());
    assert_eq!(
        img(
            r#"<img data-src="test.jpg" alt="text" title="a title"/>"#,
            Kw::zero()
        ),
        a_title_text
    );
    assert_eq!(
        img(
            r#"<p><img data-src="test.jpg" alt="text" title="a title"/></p>"#,
            Kw::zero()
        ),
        a_title_text
    );
    assert_eq!(
        img(
            r#"<p><img other="test.jpg" alt="text" title="a title"/></p>"#,
            Kw::zero()
        )
        .as_deref(),
        Some("")
    );
    assert_eq!(
        img(
            r#"<div><p><img data-src="test.jpg" alt="text" title="a title"/></p></div>"#,
            Kw::zero()
        ),
        a_title_text
    );
    assert_eq!(
        img(
            r#"<div><p><img data-src-small="test.jpg" alt="text" title="a title"/></p></div>"#,
            Kw::zero()
        ),
        a_title_text
    );
    assert_eq!(
        img(
            r#"<div><p><img src="https://a.b/test.jpg" alt="text" title="a title"/></p></div>"#,
            Kw::zero()
        )
        .as_deref(),
        Some("![a title text](https://a.b/test.jpg)")
    );

    let url = "http://a.b/c/d.html";
    for (src, expected) in [
        ("//a.b/test.jpg", "![a title text](http://a.b/test.jpg)"),
        ("/a.b/test.jpg", "![a title text](http://a.b/a.b/test.jpg)"),
        (
            "./a.b/test.jpg",
            "![a title text](http://a.b/c/a.b/test.jpg)",
        ),
        (
            "../a.b/test.jpg",
            "![a title text](http://a.b/a.b/test.jpg)",
        ),
    ] {
        assert_eq!(
            img(
                &format!(r#"<div><p><img src="{src}" alt="text" title="a title"/></p></div>"#),
                Kw::zero().url(url)
            )
            .as_deref(),
            Some(expected),
            "{src}"
        );
    }
}

// The XML line of `test_images`.
#[test]
fn test_images_xml_output() {
    if let Some(page) = resource_text("http_sample.html") {
        assert!(
            extract_xml(&page, &Kw::zero().images(true))
                .unwrap()
                .contains(r#"<graphic src="test.jpg" title="Example image"/>"#)
        );
    }
}

#[test]
fn test_links() {
    let options = default_options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let mut tree = xml_tree("<ref/>");
    let root = tree.root();
    assert_eq!(handle_textelem(&mut tree, root, &[], &ctx), Ok(None));
    let (mut tree, link) = lxml_fromstring("<a href=\"testlink.html\">Test link text.</a>");
    assert!(handle_formatting(&mut tree, link, &ctx).is_some());
    // empty link
    assert!(
        extract_tree(
            "<html><body><p><a></a><b>Some text.</b></p></body></html>",
            &Kw::new()
        )
        .is_some()
    );
    // link with target
    let html = r#"<html><body><p><a href="testlink.html">Test link text.</a> This part of the text has to be long enough.</p></body></html>"#;
    assert!(
        !extract_tree(html, &Kw::new())
            .unwrap()
            .contains("testlink.html")
    );
    assert!(
        extract_tree(html, &Kw::zero().links(true))
            .unwrap()
            .contains(
                "[Test link text.](testlink.html) This part of the text has to be long enough."
            )
    );
    // relative link conversion
    assert!(
        extract_tree(
            html,
            &Kw::zero().url("https://www.example.com/").links(true)
        )
        .unwrap()
        .contains("[Test link text.](https://www.example.com/testlink.html) This part of the text has to be long enough.")
    );
    // malformed target or page URL, target kept as is
    let html = r#"<html><body><p><a href="http://[::1">Test link text.</a> This part of the text has to be long enough.</p></body></html>"#;
    for url in ["https://www.example.com/", "https://ex]ample.org/"] {
        assert!(
            extract_tree(html, &Kw::zero().url(url).links(true))
                .unwrap()
                .contains(
                    "[Test link text.](http://[::1) This part of the text has to be long enough."
                )
        );
    }
    // link without target
    let html = "<html><body><p><a>Test link text.</a> This part of the text has to be long enough.</p></body></html>";
    assert!(
        extract_tree(html, &Kw::zero().links(true))
            .unwrap()
            .contains("[Test link text.] This part of the text has to be long enough.")
    );
    if let Some(page) = resource_text("http_sample.html") {
        assert!(
            !extract(&page, &Kw::zero())
                .unwrap()
                .contains("testlink.html")
        );
        assert!(
            extract(&page, &Kw::zero().links(true))
                .unwrap()
                .contains("[link](testlink.html)")
        );
    }
    // link in p, length threshold
    let html = format!(
        "<html><body><article><p><a>f{}</a></p></article></body></html>",
        "abcd".repeat(20)
    );
    assert!(
        extract_tree(&html, &Kw::zero().focus(Focus::Balanced))
            .unwrap()
            .contains("abc")
    );
    assert_eq!(
        extract_tree(&html, &Kw::zero().focus(Focus::Precision)).unwrap(),
        ""
    );
}

// The XML lines of `test_links`.
#[test]
fn test_links_xml_output() {
    let html = "<html><body><article><a>Segment 1</a><h1><a>Segment 2</a></h1><p>Segment 3</p></article></body></html>";
    let result = extract_xml_tree(html, &Kw::zero().links(true)).unwrap();
    assert!(result.contains('1'));
    assert!(result.contains('2'));
    assert!(result.contains('3'));
    if let Some(page) = resource_text("http_sample.html") {
        assert!(
            extract_xml(&page, &Kw::zero().links(true))
                .unwrap()
                .contains(r#"<ref target="testlink.html">link</ref>"#)
        );
    }
    // test license link; the license attribute is metadata of the XML output
    let html = r#"<html><body><p>Test text under <a rel="license" href="">CC BY-SA license</a>.</p></body></html>"#;
    assert!(
        extract_xml_with_metadata_tree(html, &Kw::zero().links(true))
            .unwrap()
            .contains(r#"license="CC BY-SA license""#)
    );
}

// Upstream's `test_tei`: its extract lines are in `test_tei_tree`, its `validate_tei` lines
// in `test_tei_validation` (D-27).
#[test]
fn test_tei() {
    let call = Call::default();
    // merge double elements
    // D-2: upstream parses with libxml2, which closes the outer `p` at the second one and keeps the
    // `p` inside the `span`, so the result is `<p><span>content</span></p>`. html5 pops the open
    // `p` together with the `span` above it when the inner `p` starts, so the tree never holds that
    // nesting: the result is `<p>content</p>` without the `span`. The expected string is
    // upstream's own functions on the html5 tree; `test_tei_strip_double_tags_libxml2_nesting`
    // asserts the libxml2 nesting.
    // oracle: sanitize(etree.tostring(remove_empty_elements(strip_double_tags(parse_document(S))), encoding="unicode")).replace("\n", "") with S the html of this block
    let (mut tree, root) = lxml_fromstring(
        "<html>
        <body>
            <p><p>
              <span><p>content</p></span>
            </p></p>
        </body>
        </html>",
    );
    assert_eq!(
        strip_double_tags_then_clean(&mut tree, root, &call),
        "<html><body><p>content</p></body></html>"
    );

    let (mut tree, root) = lxml_fromstring(
        "
    <html>
        <body>
            <div>
                <div>
                    <p>
                        <p>text</p>
                    <p>
                </div>
            </div>
        </body>
    </html>
    ",
    );
    strip_double_tags(&mut tree, root, &call).unwrap();
    assert!(has_child_path(&tree, root, "div", "div"));
    assert!(!has_child_path(&tree, root, "p", "p"));

    let mut tree = xml_tree(
        "
    <html><body>
        <div>
            <p>text1<lb/>text2<p>text3</p><lb/>text4</p>
            <p>text5<p>text6</p></p>
        </div>
    </body></html>
    ",
    );
    let root = tree.root();
    strip_double_tags(&mut tree, root, &call).unwrap();
    assert!(!has_child_path(&tree, root, "p", "p"));

    let mut tree = xml_tree(
        "
    <html><body>
        <div>
            <p>text1<lb/>text2<p>text3</p><lb/>text4</p>
            <p>text5<p>text6<p>text7</p></p></p>
        </div>
    </body></html>
    ",
    );
    let root = tree.root();
    strip_double_tags(&mut tree, root, &call).unwrap();
    assert!(!has_child_path(&tree, root, "p", "p"));
    assert!(tree.tostring(root, false, false).contains("text7"));

    // nested elements with same tag not merged
    // D-2: upstream's libxml2 keeps each `p` around its `list`, `table`, `note`, `quote` and
    // `figure`, so all five `find` calls hit. html5 closes the open `p` at the inner `p` when only
    // unknown elements (`item`, `note`, `quote`) lie between, and at a `table` or `figure` start
    // tag: the `cell` and `figure` paths keep their `p` and the other three find none, before and
    // after the call. `test_tei_strip_double_tags_libxml2_nesting` asserts the nesting upstream
    // meant.
    // oracle: [t.find(f".//{tag}/p") is not None for tag in ("item", "cell", "quote", "note", "figure")] on parse_document(S), before and after strip_double_tags(t)
    let (mut tree, root) = lxml_fromstring(WHITELISTED_NESTING);
    let expected = [false, true, false, false, true];
    assert_eq!(whitelisted_parents_with_p(&tree, root), expected);
    strip_double_tags(&mut tree, root, &call).unwrap();
    assert_eq!(whitelisted_parents_with_p(&tree, root), expected);
}

/// The `validate_tei` lines of `test_tei`. The validator is not ported (D-27: the DTD parameter
/// entities need a validator the crate does not have), so these stay ignored.
#[test]
#[ignore = "D-27"]
fn test_tei_validation() {
    // open local resources to avoid redownloading at each run
    if let Some(page) = resource_text("httpbin_sample.html") {
        let result = extract_xmltei(&page, &Kw::new().url("mocked")).unwrap();
        assert!(validate_tei(&result));
        assert!(!validate_tei(&page));
    }
    if let Some(page) = resource_text("http_sample.html") {
        for kw in [
            Kw::new().url("mocked").comments(true),
            Kw::new().url("mocked").comments(false),
        ] {
            let result = extract_xmltei(&page, &kw).unwrap();
            assert!(validate_tei(&result));
        }
        // include ID in metadata
        let result = extract_xmltei_with_id(&page, "mocked", "0001").unwrap();
        assert!(validate_tei(&result));
    }
    let extracted = extract_xmltei_tree(
        "<html><head/><body><div><h2><p>text</p></h2></div></body></html>",
        &Kw::zero().url("mocked"),
    )
    .unwrap();
    assert!(validate_tei(&extracted));
}

// The `extract` lines of `test_tei`; `test_tei` itself holds the parts that need neither.
#[test]
fn test_tei_tree() {
    let kw = Kw::zero().url("mocked");
    let extracted = extract_xmltei_tree(
        "<html><body><article><h1>title</h1><h2>subtitle</h2><p>text</p></article></body></html>",
        &kw,
    )
    .unwrap();
    assert!(extracted.contains(r#"<ab rend="h1" type="header">title</ab>"#));
    assert!(extracted.contains(r#"<ab rend="h2" type="header">subtitle</ab>"#));
    let extracted = extract_xmltei_tree(
        "<html>
        <body><article>
            <h2><div>
              <p>content</p>
              <ul>
                <li>text1</li>
                <li>text2</li>
              </ul>
            </div></h2>
            <p>Following paragraph.</p>
        </article></body>
        </html>",
        &kw,
    )
    .unwrap();
    assert!(
        extracted
            .replace('\n', "")
            .contains(r#"<ab rend="h2" type="header">content<list rend="ul"><item>text1"#)
    );
}

// Blocks 1 and 5 of `test_tei` on the nesting libxml2 gives them.
#[test]
fn test_tei_strip_double_tags_libxml2_nesting() {
    let call = Call::default();
    // oracle: sanitize(etree.tostring(remove_empty_elements(strip_double_tags(etree.fromstring(S))), encoding="unicode")).replace("\n", ""), S being the first block's html with its outer `p` closed at the second one, as `lxml.html.fromstring` parses it
    let mut tree = xml_tree(
        "<html>
<body>
<p/><p>
<span><p>content</p></span>
</p>
</body>
</html>",
    );
    let root = tree.root();
    assert_eq!(
        strip_double_tags_then_clean(&mut tree, root, &call),
        "<html><body><p><span>content</span></p></body></html>"
    );

    // oracle: [t.find(f".//{tag}/p") is not None for tag in ("item", "cell", "quote", "note", "figure")] on etree.fromstring(S), before and after strip_double_tags(t), S being `WHITELISTED_NESTING`
    let mut tree = xml_tree(WHITELISTED_NESTING);
    let root = tree.root();
    assert_eq!(whitelisted_parents_with_p(&tree, root), [true; 5]);
    strip_double_tags(&mut tree, root, &call).unwrap();
    assert_eq!(whitelisted_parents_with_p(&tree, root), [true; 5]);
}
