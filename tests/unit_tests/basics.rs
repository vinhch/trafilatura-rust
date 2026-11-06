use super::*;

/// A `<p>` element in a tree of its own, with the given text.
fn element_with_text(text: &str) -> (Tree, NodeId) {
    let mut tree = parse_fragment("<p></p>", &ParseLimits::default(), &Call::default()).unwrap();
    let p = tree.children(tree.root()).next().unwrap();
    tree.node_mut(p).text = Some(text.to_string());
    (tree, p)
}

/// `extract(content, **kw)` on bytes.
#[cfg(feature = "decompress")]
fn extract_bytes(content: &[u8], kw: &Kw) -> Option<String> {
    let input = match &kw.url {
        Some(url) => Input::bytes(content).url(url),
        None => Input::bytes(content),
    };
    or_none(kw.options().build().extract(&input, &Call::default()))
}

/// `extract(html, output_format="json", with_metadata=True, record_id=..., **kw)`, through the
/// internal JSON format.
fn extract_json_with_metadata(html: &str, kw: &Kw, record_id: Option<&str>) -> Option<String> {
    internal_text(html, kw, Format::Json, true, record_id)
}

/// The output of the internal `format` for `html` with the settings of `kw`, the text of its document.
fn internal_text(
    html: &str,
    kw: &Kw,
    format: Format,
    with_metadata: bool,
    record_id: Option<&str>,
) -> Option<String> {
    internal_document(
        Source::Input(HtmlInput::Str(html)),
        kw,
        format,
        with_metadata,
        record_id,
    )?
    .text
}

// The cache assertions at the end of upstream's `test_trim` are not ported: `meta.py` is not
// ported (module map). `sanitize(None)` has no Rust form: the argument is `&str`.
#[test]
fn test_trim() {
    assert_eq!(trim("\tTest  "), "Test");
    assert_eq!(trim("\t\tTest  Test\r\n"), "Test Test");
    let (tree, p) = element_with_text("Test Text");
    assert!(!textfilter(&tree, p));
    let (tree, p) = element_with_text("Instagram");
    assert!(textfilter(&tree, p));
    let (tree, p) = element_with_text("\t\t");
    assert!(textfilter(&tree, p));
    // non-breaking spaces
    assert_eq!(
        sanitize("Test&nbsp;Text", false, false).as_deref(),
        Some("Test Text")
    );
}

// Not ported: `load_html(123)` and `extract(None, ...)` (the input is typed), `core.Extractor(source=)`
// and the `output_format="xyz"` raises (no such option; `Format` is an enum), `_RespLike` (no
// response objects). The XML line is `test_input_xml_output`. The other lines of upstream's test
// that touch encoding and repair are `test_input_*` and `test_repair_*` in `src/utils/tests/input.rs`.
#[test]
fn test_input() {
    let options = default_options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);

    // XML-illegal chars are stripped pre-parse, so `baseline` and `extract` see clean text
    let page = format!(
        "<html><body><article>{}</article></body></html>",
        "<p>Long enough article paragraph\x1d for baseline\u{ffff} to trigger.</p>".repeat(3)
    );
    let (_, _, _, length) =
        baseline(HtmlInput::Str(&page), &ParseLimits::default(), &call).unwrap();
    assert!(length > 0);
    // fast=True upstream
    assert!(extract(&page, &Kw::new()).is_some());

    // GZip
    #[cfg(feature = "decompress")]
    if let Some(gzipped) = resource("webpage.html.gz") {
        assert!(
            extract_bytes(&gzipped, &Kw::new())
                .unwrap()
                .contains("Long story short,")
        );
    }

    // unicode normalization
    assert_ne!(normalize_unicode("A\u{308}ffin"), "A\u{308}ffin");
    let result = extract("<html><body><p>A\u{308}ffin</p></body></html>", &Kw::zero()).unwrap();
    assert_ne!(result, "A\u{308}ffin");
    assert_eq!(result, "Äffin");

    // output format
    let document = Options::new()
        .format(ApiFormat::Python)
        .build()
        .bare_extraction(&Input::text("<html><body><p>ABC</p></body></html>"), &call)
        .unwrap();
    assert_eq!(document.text.as_deref(), Some("ABC"));

    // text elements
    let mut tree = xml_tree("<p>text</p>");
    let root = tree.root();
    let processed = handle_textelem(&mut tree, root, &[], &ctx)
        .unwrap()
        .unwrap();
    assert_eq!(tree.node(processed).text.as_deref(), Some("text"));
    let mut tree = xml_tree("<unexpected>text</unexpected>");
    let root = tree.root();
    assert_eq!(handle_textelem(&mut tree, root, &[], &ctx), Ok(None));
}

#[test]
fn test_input_xml_output() {
    // output format
    assert!(
        extract_xml("<html><body><p>ABC</p></body></html>", &Kw::new())
            .unwrap()
            .contains("ABC")
    );
}

#[test]
fn test_exotic_tags() {
    // the TEI line is `test_exotic_tags_tei_output`
    let options = default_options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    // quotes
    let mut tree = xml_tree("<quote/>");
    let root = tree.root();
    assert_eq!(handle_quotes(&mut tree, root, &ctx), Ok(None));
    let mut tree = xml_tree("<table/>");
    let root = tree.root();
    assert_eq!(handle_table(&mut tree, root, TAG_CATALOG, &ctx), Ok(None));
    // p within p, and a last <lb> deleted
    let mut tree = xml_tree("<p>1st part.<p>2nd part.</p><lb/></p>");
    let root = tree.root();
    let converted = handle_paragraphs(&mut tree, root, &["p"], &ctx)
        .unwrap()
        .unwrap();
    assert_eq!(
        tree.tostring(converted, false, true),
        "<p>1st part. 2nd part.</p>"
    );
    // misformed HTML declaration
    let html = r#"<!DOCTYPE HTML PUBLIC "-//W3C//DTD HTML 4.01 Transitional//EN" 2012"http://www.w3.org/TR/html4/loose.dtd"><html><head></head><body><p>ABC</p></body></html>"#;
    assert!(extract(html, &Kw::zero()).unwrap().contains("ABC"));
    // naked div with <lb>
    assert!(
        extract(
            "<html><body><main><div>1.<br/>2.<br/>3.<br/></div></main></body></html>",
            &Kw::zero()
        )
        .unwrap()
        .contains("1.\n2.\n3.")
    );
    // HTML5: <details>
    let html = "<html><body><article><details><summary>Epcot Center</summary><p>Epcot is a theme park at Walt Disney World Resort featuring exciting attractions, international pavilions, award-winning fireworks and seasonal special events.</p></details></article></body></html>";
    let result = extract(html, &Kw::zero()).unwrap();
    assert!(result.contains("Epcot Center"));
    assert!(result.contains("award-winning fireworks"));
    // deviation: D-1, upstream asserts these two again with `fast=False`

    // edge cases
    let html = r#"<!DOCTYPE html>
<html>
  <head>
    <meta charset="UTF-8">
    <title>A weird bug</title>
  </head>
  <body>
      <div>
        <h1>Lorem ipsum dolor sit amet, consectetur adipiscing elit.</h1>
        <h2>Sed et interdum lectus.</h2>
        <p>Quisque molestie nunc eu arcu condimentum fringilla.</p>
        <!-- strong can be changed to b, em, i, u, or kbd -->
        <strong><a></a></strong>
        <h2>Aliquam eget interdum elit, id posuere ipsum.</h2>
        <p>Phasellus lectus erat, hendrerit sed tortor ac, dignissim vehicula metus.<br/></p>
      </div>
  </body>
</html>"#;
    let everything = Kw::new().formatting(true).links(true).images(true);
    assert!(extract(html, &everything).is_some());

    let html = r#"<!DOCTYPE html>
<html>
  <head>
    <meta charset="UTF-8">
    <title>A weird bug</title>
  </head>
  <body>
    <div id="content">
      <h1>A header</h1>
      <h2>Very specific bug so odd</h2>
      <h3>Nested header</h3>
      <p>Some "hyphenated-word quote" followed by a bit more text line.</p>
      <em><p>em improperly wrapping p here</p></em>
      <p>Text here<br/></p>
      <h3>More articles</h3>
    </div>
  </body>
</html>"#;
    for kw in [
        everything.clone(),
        everything.clone().focus(Focus::Precision),
        everything.focus(Focus::Recall),
    ] {
        let result = extract(html, &kw).unwrap();
        assert!(result.contains("em improperly wrapping p here"));
        assert!(result.ends_with("Text here"));
    }

    // comments
    assert!(
        extract(
            r#"<html><body><article><p>text</p><div class="comments"><p>comment</p></div></article></body></html>"#,
            &Kw::zero().comments(true)
        )
        .unwrap()
        .ends_with("\ncomment")
    );
}

#[test]
fn test_exotic_tags_tei_output() {
    // `load_mock_page("http://exotic_tags", tei_output=True)`: the page is read from the checkout
    let Some(page) = resource_text("exotic_tags.html") else {
        return;
    };
    let result = extract_xmltei_with_id(&page, "http://exotic_tags", "0000").unwrap();
    assert!(result.contains("Teletype text"));
    assert!(result.contains("My new car is silver."));
}

#[test]
fn test_formatting() {
    // upstream's `ZERO_CONFIG` changes nothing here: `handle_formatting` reads no size option
    let options = default_options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    // handle formatting tails
    let mut tree = xml_tree("<hi>Here is the text.</hi>");
    let element = tree.root();
    tree.node_mut(element).tail = Some("And a tail.".to_string());
    let converted = handle_formatting(&mut tree, element, &ctx).unwrap();
    assert_eq!(
        tree.tostring(converted, false, true),
        "<p><hi>Here is the text.</hi>And a tail.</p>"
    );
    // no wrapper inside a protected parent
    let mut tree = xml_tree("<item><hi>Here is the text.</hi></item>");
    let element = tree.first_child(tree.root()).unwrap();
    let kept = handle_formatting(&mut tree, element, &ctx).unwrap();
    assert_eq!(
        tree.tostring(kept, false, true),
        "<hi>Here is the text.</hi>"
    );

    // the `extract` lines: upstream passes `html.fromstring(...)` trees, as `extract_tree` does
    // titles as markdown
    let html = "<html><body><article><h3>Title</h3><p><b>This here is in bold font.</b>Non-bold here</p></article></body></html>";
    let result = extract_tree(html, &Kw::zero().formatting(true)).unwrap();
    assert_eq!(
        result,
        "### Title\n\n**This here is in bold font.**Non-bold here"
    );
    assert_eq!(extract(html, &Kw::zero().markdown()).unwrap(), result);
    let document = Options::new()
        .format(ApiFormat::Markdown)
        .config(zero_config())
        .build()
        .bare_extraction(&Input::text(html), &call)
        .unwrap();
    assert!(
        document
            .body_tree()
            .tostring(document.body(), false, false)
            .contains(r##"<hi rend="#b">"##)
    );
    // space between paragraphs
    let html = "<html><body><article><h3>Title</h3><p>Paragraph 1</p><p>Paragraph 2</p></article></body></html>";
    let result = extract_tree(html, &Kw::zero().formatting(true)).unwrap();
    assert!(result.ends_with("Paragraph 1\n\nParagraph 2"));
    // code sections
    let html = "<html><body><article><h3>Title</h3><p>Here is a code sample:</p><code>import trafilatura</code></p></article></body></html>";
    assert_eq!(
        extract_tree(html, &Kw::zero().formatting(true)).unwrap(),
        "### Title\n\nHere is a code sample:\n\n`import trafilatura`"
    );
    let html = r#"<html><body><article><h3>Title</h3><p>Here is a code sample:</p><code><span>import</span> <span>something</span><br/>something.run("somewhere")</code><p>Sometimes code is wrapped using <code>pre</code> and <code>code</code>:</p><pre><code>import trafilatura
trafilatura.extract("")</code></pre><p>Less often code is wrapped using just <code>pre</code>:</p><pre>
trafilatura.extract("")</pre></article></body></html>"#;
    // deviation: D-2, html5 drops the newline right after `<pre>`, which libxml2 keeps and upstream's
    // fenced last block needs; the last block is inline code here; oracle: extract(<html5
    // fromstring>(html), include_formatting=True, config=ZERO_CONFIG) with fast=True
    assert_eq!(
        extract_tree(html, &Kw::zero().formatting(true)).unwrap(),
        r#"### Title

Here is a code sample:

```
import something
something.run("somewhere")
```
Sometimes code is wrapped using `pre` and `code`:

```
import trafilatura
trafilatura.extract("")
```
Less often code is wrapped using just `pre`:

`trafilatura.extract("")`"#
    );
    // wild div
    assert_eq!(
        extract_tree(
            "<html><body><article><div><strong>Wild text</strong></div></article></body></html>",
            &Kw::zero()
        )
        .unwrap(),
        "Wild text"
    );
    // links
    // deviation: D-1, upstream asserts "Link text" (`fast=False`: the fallback keeps the link text);
    // oracle: extract(html.fromstring(html), config=ZERO_CONFIG) with fast=True
    assert_eq!(
        extract_tree(
            r#"<html><body><p><a href="">Link text</a></p></body></html>"#,
            &Kw::zero()
        )
        .unwrap(),
        ""
    );
    // line-breaks
    assert_eq!(
        extract_tree("<html><body><p><br/></p></body></html>", &Kw::zero()).unwrap(),
        ""
    );
    assert_eq!(
        extract_tree(
            "<html><body><p><br/>Here is the text.</p></body></html>",
            &Kw::zero()
        )
        .unwrap(),
        "Here is the text."
    );
    // lists with links
    // deviation: D-1, upstream asserts the markdown list ("- Number 0\n- Number [1](test.html)\n...");
    // oracle: extract(html.fromstring(html), output_format="markdown", include_links=True,
    // config=ZERO_CONFIG) with fast=True
    let html = r#"<html><body><article>
        <ul>
            <li>Number 0</li>
            <li>Number <a href="test.html">1</a></li>
            <li><a href="test.html">Number 2</a> n2</li>
            <li>Number 3</li>
            <li><p>Number 4</p> n4</li>
        </ul>
        Test</article></body></html>
    "#;
    assert_eq!(
        extract_tree(html, &Kw::zero().markdown().links(true)).unwrap(),
        ""
    );
    // XML and Markdown formatting within <p>-tag
    let html = r#"<html><body><p><b>bold</b>, <i>italics</i>, <tt>tt</tt>, <strike>deleted</strike>, <u>underlined</u>, <a href="test.html">link</a> and additional text to bypass detection.</p></body></html>"#;
    let plain =
        "bold, italics, tt, deleted, underlined, link and additional text to bypass detection.";
    let marked = "**bold**, *italics*, `tt`, ~~deleted~~, <u>underlined</u>, link and additional text to bypass detection.";
    for (kw, expected) in [
        (Kw::zero().formatting(false), plain.to_string()),
        (Kw::zero().formatting(true), marked.to_string()),
        (
            Kw::zero().format(ApiFormat::Txt).formatting(true),
            marked.to_string(),
        ),
        (
            Kw::zero().formatting(true).links(true),
            marked.replace(" link ", " [link](test.html) "),
        ),
    ] {
        assert_eq!(extract_tree(html, &kw).unwrap(), expected);
    }
    // line-break following formatting
    let html = "<html><body><article><p><strong>Staff Review of the Financial Situation</strong><br>Domestic financial conditions remained accommodative over the intermeeting period.</p></article></body></html>";
    assert_eq!(
        extract_tree(html, &Kw::zero()).unwrap(),
        "Staff Review of the Financial Situation\nDomestic financial conditions remained accommodative over the intermeeting period."
    );
    // code in a pre block
    // deviation: D-1, upstream asserts "python code below:\n```\n..." (`fast=False`: the fallback
    // keeps the text before the block); oracle: extract(html.fromstring(html),
    // output_format="markdown", include_formatting=True, config=ZERO_CONFIG) with fast=True. The
    // upstream test asserts it twice.
    let html = "
    <html><head><body><article>python code below:
<pre><code>
def test:
    print('hello')
    print('world')
    </code></pre>
    </article></body></html>
    ";
    assert_eq!(
        extract_tree(html, &Kw::zero().markdown().formatting(true)).unwrap(),
        "```\ndef test:\n    print('hello')\n    print('world')\n    \n```"
    );
}

// The XML lines of `test_formatting`.
#[test]
fn test_formatting_xml_tree() {
    let xml = |html: &str, kw: Kw| extract_xml_tree(html, &kw).unwrap();
    // trailing <lb>
    let result = xml(
        "<html><body><p>This here is the text.<br/></p></body></html>",
        Kw::zero(),
    );
    assert!(!result.contains("lb"));
    // simple formatting
    let result = xml(
        "<html><body><p><b>This here is in bold font.</b></p></body></html>",
        Kw::zero().formatting(true),
    );
    assert!(result.contains(r##"<hi rend="#b">This here is in bold font.</hi>"##));
    // nested
    let result = xml(
        "<html><body><p><b>This here is in bold and <i>italic</i> font.</b></p></body></html>",
        Kw::zero().formatting(true),
    );
    assert!(result.contains(
        r##"<hi rend="#b">This here is in bold and <hi rend="#i">italic</hi> font.</hi>"##
    ));
    // empty
    let result = xml(
        "<html><body><p><b><i></i></b></p></body></html>",
        Kw::zero().formatting(true),
    );
    assert!(result.contains("<main/>"));
    // wild div
    let result = xml(
        "<html><body><article><div><strong>Wild text</strong></div></article></body></html>",
        Kw::zero().formatting(true),
    );
    assert!(result.contains("<p>"));
    assert!(result.contains(r##"<hi rend="#b">Wild text</hi>"##));
    // empty elements
    // deviation: D-1, upstream asserts "<main>\n    <p>There is text here.</p>\n  </main>" (`fast=False`);
    // oracle: extract(html.fromstring(html), output_format="xml", config=ZERO_CONFIG) with fast=True
    let result = xml(
        "<html><body><div>\t\n</div><div>There is text here.</div></body></html>",
        Kw::zero(),
    );
    assert!(result.contains("<main/>"));
    // lists with links
    let result = xml(
        r#"<html><body><article><ul><li>Number 1</li><li>Number <a href="test.html">2</a></li><li>Number 3</li><p>Test</p></article></body></html>"#,
        Kw::zero().links(true),
    );
    assert!(result.contains(r#"<item>Number <ref target="test.html">2</ref></item>"#));
    // XML formatting within <p>-tag
    let html = r#"<html><body><p><b>bold</b>, <i>italics</i>, <tt>tt</tt>, <strike>deleted</strike>, <u>underlined</u>, <a href="test.html">link</a> and additional text to bypass detection.</p></body></html>"#;
    let xml_p = |link: &str| {
        format!(
            r##"<p><hi rend="#b">bold</hi>, <hi rend="#i">italics</hi>, <hi rend="#t">tt</hi>, <del rend="overstrike">deleted</del>, <hi rend="#u">underlined</hi>, {link} and additional text to bypass detection.</p>"##
        )
    };
    for (links, link) in [
        (false, "link"),
        (true, r#"<ref target="test.html">link</ref>"#),
    ] {
        let result = xml(html, Kw::zero().formatting(true).links(links));
        assert!(result.contains(&xml_p(link)));
    }
    // double <p>-elems
    let result = xml(
        "<html><body><p>AAA, <p>BBB</p>, CCC.</p></body></html>",
        Kw::zero().formatting(true).links(true),
    );
    assert!(result.contains("AAA"));
    assert!(result.contains("BBB"));
    assert!(result.contains("CCC"));
    // title with formatting
    let result = xml(
        r#"<html><body><article><h4 id="1theinoperator">1) The <code>in</code> Operator</h4><p>The easiest way to check if a Python string contains a substring is to use the <code>in</code> operator. The <code>in</code> operator is used to check data structures for membership in Python. It returns a Boolean (either <code>True</code> or <code>False</code>) and can be used as follows:</p></article></body></html>"#,
        Kw::zero().formatting(true),
    );
    assert!(result.contains(r#"<head rend="h4">1) The <code>in</code> Operator</head>"#));
    assert!(result.contains("<p>The easiest way to check if a Python string contains a substring is to use the <code>in</code> operator. The <code>in</code> operator is used to check data structures for membership in Python. It returns a Boolean (either <code>True</code> or <code>False</code>) and can be used as follows:</p>"));
    let result = xml(
        "<html><body><table><td><p>Sjätte <nobr>AP-fonden</nobr></p></td></table></body></html>",
        Kw::zero().tables(true),
    );
    assert!(result.contains("AP-fonden"));
}

// The record ID lines of `test_formatting`, the JSON `id` included.
#[test]
fn test_formatting_record_id() {
    // the record ID and fingerprint are in the header, as in the other formats
    let meta_string = "<html><head><title>Test</title></head><body><p>ABC.</p></body></html>";
    let json_result: serde_json::Value = serde_json::from_str(
        &extract_json_with_metadata(meta_string, &Kw::zero(), Some("doc-1")).unwrap(),
    )
    .unwrap();
    for kw in [Kw::zero().markdown(), Kw::zero()] {
        let meta_result = extract_with_metadata(meta_string, &kw, Some("doc-1")).unwrap();
        assert!(meta_result.contains(&format!(
            "\nfingerprint: {}\nid: doc-1\n",
            json_result["fingerprint"].as_str().unwrap()
        )));
    }
}

/// A snippet parsed as one element, as `etree.fromstring` gives it: the tree and that element.
fn element(source: &str) -> (Tree, NodeId) {
    let tree = parse_fragment(source, &ParseLimits::default(), &Call::default()).unwrap();
    let root = tree.children(tree.root()).next().unwrap();
    (tree, root)
}

// Upstream's `test_xmltocsv`.
#[test]
fn test_xmltocsv() {
    let call = Call::default();
    let mut doc = Document::empty();
    let (tree, xml) = element("<xml/>");
    doc.set_body(&tree, xml);
    doc.set_comments_body(&tree, xml);
    assert_eq!(
        xml_to_csv(&mut doc, false, &call).unwrap(),
        "null\tnull\tnull\tnull\tnull\tnull\tnull\tnull\tnull\tnull\tnull\r\n"
    );

    doc.title = Some("Test title".to_owned());
    doc.url = Some("https://example.org".to_owned());
    doc.hostname = Some("example.org".to_owned());
    doc.id = Some("1".to_owned());
    doc.license = Some("CC BY-SA".to_owned());
    doc.image = Some("https://example.org/image.jpg".to_owned());
    doc.pagetype = Some("article".to_owned());
    let (tree, p) = element("<p>Test text</p>");
    doc.set_body(&tree, p);
    let (tree, p) = element("<p>Test comment</p>");
    doc.set_comments_body(&tree, p);
    assert_eq!(
        xml_to_csv(&mut doc, false, &call).unwrap(),
        "https://example.org\t1\tnull\texample.org\tTest title\thttps://example.org/image.jpg\tnull\tTest text\tTest comment\tCC BY-SA\tarticle\r\n"
    );

    let mystring = "<html><body><p>ÄÄÄÄÄÄÄÄÄÄÄÄÄÄ</p></body></html>";
    let kw = Kw::zero();
    assert!(internal_text(mystring, &kw, Format::Csv, false, None).is_some());
    assert!(
        internal_text(mystring, &kw.comments(false), Format::Csv, false, None)
            .unwrap()
            .ends_with("\tnull\r\n")
    );
}

// Upstream's `test_tojson`; it checks substrings only, the exact strings are in `output_format_cases`
// and `xml::tests::formats`.
#[test]
fn test_tojson() {
    let mystring = "<html><body><p>ÄÄÄÄÄÄÄÄÄÄÄÄÄÄ</p></body></html>";
    let kw = Kw::zero();
    let result = internal_text(mystring, &kw, Format::Json, false, None).unwrap();
    assert!(result.contains('Ä'));
    assert!(result.ends_with('}'));
    let result = extract_json_with_metadata(mystring, &kw, None).unwrap();
    assert!(result.ends_with('}'));
    assert!(result.contains("\"fingerprint\":"));
    assert!(result.contains("\"language\":"));
    assert!(
        internal_text(mystring, &kw.comments(false), Format::Json, false, None)
            .unwrap()
            .ends_with('}')
    );
}

// The Markdown metadata lines of `test_formatting`; the JSON lines are in `test_formatting_record_id`.
#[test]
fn test_formatting_metadata_lines() {
    let meta_string = "<html><head><title>Test</title></head><body><p>ABC.</p></body></html>";
    let meta_result = extract_with_metadata(meta_string, &Kw::zero().markdown(), None).unwrap();
    assert!(meta_result.starts_with("---\ntitle: Test\nfingerprint: "));
    assert!(meta_result.ends_with("---\nABC."));
}

#[test]
fn test_subelement_text_kept() {
    // Text of nested and unexpected subelements stays in place.
    let kw = Kw::zero();
    // a container's tail follows its children
    assert!(
        md_inline("<ul><li><div>A <b>B</b> C</div> tail</li></ul>", kw.clone())
            .contains("**B** C tail")
    );
    // unexpected tags in a paragraph keep their text and tail
    assert!(md_inline("<p>Hello <foo>bar</foo> world</p>", kw.clone()).contains("Hello bar world"));
    // every line break in a flattened hi keeps its space
    assert!(md_inline("<p>A <b>b<br>c<br>d</b> e</p>", kw.clone()).contains("A **b c d** e"));
    assert!(
        md_inline(
            "<blockquote><p>Hello <foo>bar</foo> world</p></blockquote>",
            kw.clone()
        )
        .contains("bar world")
    );
    // a list in a cell keeps its tail, dropped or not
    for recall in [false, true] {
        let table = "<table><tr><td>a<ul><li>x</li></ul>after</td><td>b</td></tr></table>";
        let kw = if recall {
            kw.clone().focus(Focus::Recall)
        } else {
            kw.clone()
        };
        assert!(table_md(table, kw).unwrap().contains("after"), "{recall}");
    }
    // a whitespace-only item is dropped, numbering stays continuous
    assert!(
        md_inline(
            "<ol><li>a</li><li> <b> </b> </li><li>b</li></ol><p>after</p>",
            kw
        )
        .contains("1. a\n2. b")
    );
}

#[test]
fn test_subelement_tails_unit() {
    // Whitespace-only tails are not moved, consumed children are not merged back.
    let options = default_options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let mut tree = xml_tree("<list><item><div>A <hi>B</hi></div>\n\t </item></list>");
    let root = tree.root();
    let item = handle_lists(&mut tree, root, &ctx).unwrap().unwrap();
    let hi = tree.find(item, "hi").unwrap();
    assert_eq!(tree.node(hi).tail, None);
    let mut tree = xml_tree("<p>a <done>old</done> b</p>");
    let root = tree.root();
    let paragraph = handle_paragraphs(&mut tree, root, TAG_CATALOG, &ctx)
        .unwrap()
        .unwrap();
    assert!(!tree.itertext(paragraph).collect::<String>().contains("old"));
    // a teaser heading whose lead was consumed earlier is not emitted again (lanacion)
    let mut tree = xml_tree("<head><done>Lead. </done>Teaser</head>");
    let root = tree.root();
    assert_eq!(handle_titles(&mut tree, root, &ctx), Ok(None));
}

#[test]
fn test_markdown_metadata_yaml_safe() {
    // Markdown metadata header must stay valid YAML for special values (GH #814). Scalar
    // rendering: plain when safe, double-quoted (json) otherwise.
    assert_eq!(yaml_scalar("Indu K Murthy"), "Indu K Murthy");
    assert_eq!(
        yaml_scalar("https://example.com/a:b"),
        "https://example.com/a:b"
    );
    assert_eq!(yaml_scalar("élan vital"), "élan vital");
    assert_eq!(yaml_scalar("COP30: a guide"), r#""COP30: a guide""#);
    assert_eq!(yaml_scalar("#1 ranking"), r##""#1 ranking""##);
    assert_eq!(yaml_scalar("&launch"), r#""&launch""#);
    assert_eq!(yaml_scalar("true"), r#""true""#);
    assert_eq!(yaml_scalar("2024"), r#""2024""#);
    assert_eq!(yaml_scalar("[draft]"), r#""[draft]""#);
    assert_eq!(yaml_scalar(r#"say "hi": now"#), r#""say \"hi\": now""#);
    // The round trip through a real YAML parser is not ported: the crate has no YAML reader.
}

// The end-to-end lines of `test_markdown_metadata_yaml_safe`, minus the YAML parse.
#[test]
fn test_markdown_metadata_yaml_safe_lines() {
    // end-to-end: a title with ": " used to emit invalid YAML, now it is quoted
    let html = concat!(
        "<html><head><title>COP30: a beginner\u{2019}s guide</title>",
        r#"<meta name="author" content="Indu K Murthy"/></head>"#,
        "<body><article><p>Some body text with enough words to be extracted.</p>",
        "</article></body></html>"
    );
    let result = extract_with_metadata(html, &Kw::zero().markdown(), None).unwrap();
    assert!(result.contains("title: \"COP30: a beginner\u{2019}s guide\""));
    // oracle: extract(html, output_format="markdown", config=ZERO_CONFIG, with_metadata=True) with
    // fast=True gives the header lines `title: "COP30: a beginner's guide"` (U+2019 for the
    // apostrophe, as in the title above) and `author: Indu K Murthy`
    assert!(result.contains("author: Indu K Murthy"));
}
