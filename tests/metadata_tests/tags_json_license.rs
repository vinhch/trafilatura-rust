use super::*;

/// `extract_title(html.fromstring(html))`.
fn title_of(html: &str) -> Option<String> {
    let (mut tree, root) = parse_html(html);
    extract_title(&mut tree, root, &Call::default()).unwrap()
}

#[test]
fn test_catstags() {
    // Test extraction of categories and tags
    assert_eq!(normalize_tags("   ").unwrap(), "");
    assert_eq!(normalize_tags(" 1 &amp; 2 ").unwrap(), "1 & 2");

    let cases: [(&str, &str, Vec<&str>); 7] = [
        (
            r#"<html><body><p class="entry-categories"><a href="https://example.org/category/cat1/">Cat1</a>, <a href="https://example.org/category/cat2/">Cat2</a></p></body></html>"#,
            "categories",
            vec!["Cat1", "Cat2"],
        ),
        (
            r#"<html><body><div class="postmeta"><a href="https://example.org/category/cat1/">Cat1</a></div></body></html>"#,
            "categories",
            vec!["Cat1"],
        ),
        (
            r#"<html><body><p class="entry-tags"><a href="https://example.org/tags/tag1/">Tag1</a>, <a href="https://example.org/tags/tag2/">Tag2</a></p></body></html>"#,
            "tags",
            vec!["Tag1", "Tag2"],
        ),
        (
            r#"<html><head><meta name="keywords" content="sodium, salt, paracetamol, blood, pressure, high, heart, &amp;quot, intake, warning, study, &amp;quot, medicine, dissolvable, cardiovascular" /></head></html>"#,
            "tags",
            vec![
                "sodium, salt, paracetamol, blood, pressure, high, heart, intake, warning, study, medicine, dissolvable, cardiovascular",
            ],
        ),
        // plural "/categories/" in the href must be recognised
        (
            r#"<html><body><p class="entry-categories"><a href="https://example.org/categories/cat1/">Cat1</a></p></body></html>"#,
            "categories",
            vec!["Cat1"],
        ),
        // article:tag meta -> tags
        (
            r#"<html><head><meta property="article:tag" content="Sports"/></head><body/></html>"#,
            "tags",
            vec!["Sports"],
        ),
        // category fallback via article:section meta (no category links)
        (
            r#"<html><head><meta property="article:section" content="Politics"/></head><body/></html>"#,
            "categories",
            vec!["Politics"],
        ),
    ];
    for (doc, attr, expected) in cases {
        let metadata = extract(doc);
        let actual = match attr {
            "categories" => metadata.categories,
            "tags" => metadata.tags,
            other => panic!("no field {other}"),
        };
        assert_eq!(actual, Some(owned(&expected)), "{doc}");
    }
}

#[test]
fn test_date_config() {
    // "A user date_config without 'max_date' must not raise, and must not be mutated."
    // Adaptation: a DateParams with max_date None, passed by reference, stands for the dict.
    let config = DateParams {
        original_date: true,
        extensive_search: false,
        max_date: None,
    };
    let before = config.clone();
    let doc = "<html><head><title>T</title></head><body><p>x</p></body></html>";
    let (mut tree, root) = parse_html(doc);
    let result = extract_metadata(
        &mut tree,
        root,
        None,
        Some(&config),
        false,
        &HashSet::new(),
        &Call::default(),
    );
    assert!(result.is_ok());
    assert_eq!(config, before);
}

#[test]
fn test_extract_title_fallbacks() {
    // Fallback titles are trimmed; nothing found returns None.
    assert_eq!(
        title_of("<html><body><h1>  A  </h1><h1>B</h1></body></html>").as_deref(),
        Some("A")
    );
    assert_eq!(title_of("<html><body><p>x</p></body></html>"), None);
    // regression #400: an empty single <h1> must not block a non-empty <h2>
    assert_eq!(
        title_of("<html><body><h1></h1><h2>Real Title</h2></body></html>").as_deref(),
        Some("Real Title")
    );
}

#[test]
fn test_json_minify() {
    // regression #854: an escaped backslash must not strip whitespace from adjacent JSON-LD fields.
    // Adaptation: JSON_MINIFY is a LazyLock of an optional regex, so the test unwraps it.
    let minified = JSON_MINIFY
        .as_ref()
        .expect("JSON_MINIFY compiles")
        .replace_all(
            r#"{"author": "Curtis Heyen", "keywords": ["fatal stabbing\\", "homicide"]}"#,
            "$1",
        );
    assert!(minified.contains(r#""Curtis Heyen""#));
}

#[test]
fn test_extract_meta_json_control_char() {
    // regression: a non-whitespace control character (not neutralized by normalize_json's trim,
    // unlike \n, \r and \t) must not make the JSON-LD parse fail and fall back to the crude regex
    // extractor, which cannot see past the first (non-article) @graph node.
    let payload = "{\"@context\": \"https://schema.org\", \"@graph\": [{\"@type\": \"Organization\", \"name\": \"Acme Corp\"}, {\"@type\": \"Article\", \"headline\": \"Graph based headline\", \"description\": \"Body text with a stray control char\x01 inside it.\"}]}";
    let doc = format!(
        "<html><head><script type=\"application/ld+json\">{payload}</script></head><body></body></html>"
    );
    let (tree, root) = parse_html(&doc);
    let mut metadata = blank_document();
    extract_meta_json(&tree, root, &mut metadata, &Call::default()).unwrap();
    assert_eq!(metadata.pagetype.as_deref(), Some("article"));
    assert_eq!(metadata.title.as_deref(), Some("Graph based headline"));
}

#[test]
fn test_extract_json_multiple_blocks() {
    // regression: a flat JSON-LD object must not short-circuit the loop and drop later @graph blocks.
    let schema = json_of(
        r#"[{"@context": "https://schema.org", "@type": "WebPage", "name": "Flat object"}, {"@context": "https://schema.org", "@graph": [{"@type": "Article", "headline": "Graph Headline"}]}]"#,
    );
    let mut metadata = blank_document();
    extract_json(&schema, schema.root(), &mut metadata, &Call::default()).unwrap();
    assert_eq!(metadata.title.as_deref(), Some("Graph Headline"));
}

#[test]
fn test_extract_json_null_author() {
    // regression: "author": null must not raise -- process_parent's author loop expects an
    // iterable, not None.
    let schema = json_of(
        r#"{"@context": "https://schema.org", "@type": "Article", "headline": "Some Headline", "author": null}"#,
    );
    let mut metadata = blank_document();
    extract_json(&schema, schema.root(), &mut metadata, &Call::default()).unwrap();
    assert_eq!(metadata.title.as_deref(), Some("Some Headline"));
    assert_eq!(metadata.author, None);
}

#[test]
fn test_extract_json_parse_error() {
    // Crude-regex fallback for malformed JSON-LD extracts each field; comma-publisher is rejected.
    let blob = r#"{"@type":"Article","author":{"name":"Jane Doe"},"publisher":{"@type":"Organization","name":"Example Publisher"},"articleSection":"World News","headline":"Big Story",} BROKEN"#;
    let mut m = blank_document();
    extract_json_parse_error(blob, &mut m, &Call::default()).unwrap();
    assert_eq!(m.author.as_deref(), Some("Jane Doe"));
    assert_eq!(m.pagetype.as_deref(), Some("article"));
    assert_eq!(m.sitename.as_deref(), Some("Example Publisher"));
    assert_eq!(m.categories, Some(owned(&["World News"])));
    assert_eq!(m.title.as_deref(), Some("Big Story"));

    let mut m = blank_document();
    extract_json_parse_error(
        r#"{"publisher":{"name":"Acme, Inc"}} X"#,
        &mut m,
        &Call::default(),
    )
    .unwrap();
    assert_eq!(m.sitename, None);
}

#[test]
fn test_process_parent_keeps_good_sitename() {
    // regression: a bare publisher dict must not clobber an already-plausible sitename.
    let mut metadata = blank_document();
    metadata.sitename = Some("A Long Established Site Name".to_owned());
    let json = json_of(r#"[{"publisher": {"@type": "Organization", "name": "Pub"}}]"#);
    let parent: Vec<_> = json.items(json.root()).collect();
    process_parent(&json, &parent, &mut metadata, &Call::default()).unwrap();
    assert_eq!(
        metadata.sitename.as_deref(),
        Some("A Long Established Site Name")
    );
}

// Adaptation: upstream parametrizes over the four rel values; here they are one loop.
#[test]
fn test_license_rel_tokens() {
    for rel in [
        "license noopener",
        "noopener license noreferrer",
        "LICENSE",
        "\tlicense\nnoopener",
    ] {
        let metadata = extract(&format!(
            r#"<html><body><a href="https://example.org/terms" rel="{rel}">Publication terms</a></body></html>"#
        ));
        assert_eq!(
            metadata.license.as_deref(),
            Some("Publication terms"),
            "rel={rel:?}"
        );
    }
}

#[test]
fn test_license_rel_requires_complete_token() {
    let metadata = extract(
        r#"<html><body><a href="https://example.org/terms" rel="not-license">Publication terms</a></body></html>"#,
    );
    assert_eq!(metadata.license, None);
}

#[test]
fn test_license() {
    // Test extraction of CC licenses
    // a rel
    let metadata = extract(
        r#"<html><body><p><a href="https://creativecommons.org/licenses/by-sa/4.0/" rel="license">CC BY-SA</a></p></body></html>"#,
    );
    assert_eq!(metadata.license.as_deref(), Some("CC BY-SA 4.0"));
    let metadata = extract(
        r#"<html><body><p><a href="https://licenses.org/unknown" rel="license">Unknown</a></p></body></html>"#,
    );
    assert_eq!(metadata.license.as_deref(), Some("Unknown"));
    // footer
    let metadata = extract(
        r#"<html><body><footer><a href="https://creativecommons.org/licenses/by-sa/4.0/">CC BY-SA</a></footer></body></html>"#,
    );
    assert_eq!(metadata.license.as_deref(), Some("CC BY-SA 4.0"));
    // footer: netzpolitik.org
    let metadata = extract(
        r#"<html><body>
<div class="footer__navigation">
<p class="footer__licence">
            <strong>Lizenz: </strong>
            Die von uns verfassten Inhalte stehen, soweit nicht anders vermerkt, unter der Lizenz
            <a href="http://creativecommons.org/licenses/by-nc-sa/4.0/">Creative Commons BY-NC-SA 4.0.</a>
        </p>
    </div>
</body></html>"#,
    );
    assert_eq!(metadata.license.as_deref(), Some("CC BY-NC-SA 4.0"));
    // this is not a license. Adaptation: the literal tabs of upstream's source are \t escapes.
    let metadata = extract(
        "<html><body><footer class=\"entry-footer\">\n\t<span class=\"cat-links\">Posted in <a href=\"https://sallysbakingaddiction.com/category/seasonal/birthday/\" rel=\"category tag\">Birthday</a></span>\n\t</footer></body></html>",
    );
    assert_eq!(metadata.license, None);
    // this is a license
    let metadata = extract(
        "<html><body><footer class=\"entry-footer\">\n\t<span>The license is <a href=\"https://example.org/1\">CC BY-NC</a></span>\n\t</footer></body></html>",
    );
    assert_eq!(metadata.license.as_deref(), Some("CC BY-NC"));
    // license text wrapped in nested markup (text_content, not .text)
    let metadata = extract(
        r#"<html><body><footer><a href="/x"><span>CC BY-SA 4.0</span></a></footer></body></html>"#,
    );
    assert_eq!(metadata.license.as_deref(), Some("CC BY-SA 4.0"));
    // a rel=license link with no href/text cue yields no license
    let metadata = extract(r#"<html><body><p><a rel="license" href="/x"></a></p></body></html>"#);
    assert_eq!(metadata.license, None);
}

#[test]
fn test_images() {
    // Image extraction from meta SEO tags
    let cases = [
        (
            r#"<html><head><meta property="image" content="https://example.org/example.jpg"></html>"#,
            Some("https://example.org/example.jpg"),
        ),
        (
            r#"<html><head><meta property="og:image:url" content="example.jpg"></html>"#,
            Some("example.jpg"),
        ),
        (
            r#"<html><head><meta property="og:image" content="https://example.org/example-opengraph.jpg" /><body/></html>"#,
            Some("https://example.org/example-opengraph.jpg"),
        ),
        (
            r#"<html><head><meta property="twitter:image" content="https://example.org/example-twitter.jpg"></html>"#,
            Some("https://example.org/example-twitter.jpg"),
        ),
        (
            r#"<html><head><meta property="twitter:image:src" content="example-twitter.jpg"></html>"#,
            Some("example-twitter.jpg"),
        ),
        (
            r#"<html><head><meta name="twitter:image" content="https://example.org/example-name.jpg"></html>"#,
            Some("https://example.org/example-name.jpg"),
        ),
        (
            r#"<html><head><meta name="robots" content="index, follow, max-image-preview:large, max-snippet:-1, max-video-preview:-1" /></html>"#,
            None,
        ),
    ];
    for (doc, expected) in cases {
        assert_eq!(extract(doc).image.as_deref(), expected, "{doc}");
    }
}

/// The page of test_document_as_dict, shared with test_document_as_dict_metadata_lines, which needs
/// its date.
const DOCUMENT_AS_DICT_PAGE: &str = r#"
    <html>
    <head>
        <title>Test Title</title>
        <meta itemprop="author" content="Jenny Smith" />
        <meta property="og:url" content="https://example.org" />
        <meta itemprop="description" content="Description" />
        <meta property="og:published_time" content="2017-09-01" />
        <meta name="article:publisher" content="The Newspaper" />
        <meta property="image" content="https://example.org/example.jpg" />
    </head>
    <body>
        <p class="entry-categories">
        <a href="https://example.org/category/cat1/">Cat1</a>,
        <a href="https://example.org/category/cat2/">Cat2</a>
        </p>
        <p>
        <a href="https://creativecommons.org/licenses/by-sa/4.0/" rel="license"
            >CC BY-SA</a
        >
        </p>
    </body>
    </html>
    "#;

// Adaptation: upstream's as_dict is not ported; the same values are asserted on the fields. The
// date assertion is in test_document_as_dict_metadata_lines.
#[test]
fn test_document_as_dict() {
    // Tests that the dict serialization works and preserves data.
    let document = extract(DOCUMENT_AS_DICT_PAGE);
    assert_eq!(document.title.as_deref(), Some("Test Title"));
    assert_eq!(document.author.as_deref(), Some("Jenny Smith"));
    assert_eq!(document.url.as_deref(), Some("https://example.org"));
    assert_eq!(document.description.as_deref(), Some("Description"));
    assert_eq!(document.sitename.as_deref(), Some("The Newspaper"));
    assert_eq!(document.categories, Some(owned(&["Cat1", "Cat2"])));
    assert_eq!(document.license.as_deref(), Some("CC BY-SA 4.0"));
    assert_eq!(
        document.image.as_deref(),
        Some("https://example.org/example.jpg")
    );
}

#[test]
fn test_document_as_dict_metadata_lines() {
    assert_eq!(
        extract(DOCUMENT_AS_DICT_PAGE).date.as_deref(),
        Some("2017-09-01")
    );
}

#[test]
fn test_block_boundaries_not_fused() {
    // block children must not fuse on a minified page (GH #896); inline children must stay joined
    let title =
        extract("<html><body><h1><div>Kicker</div><div>Main headline</div></h1></body></html>");
    assert_eq!(title.title.as_deref(), Some("Kicker Main headline"));

    let cats = extract(
        r#"<html><body><h1>T</h1><p class="entry-categories"><a href="/category/local"><div>Local</div><div>News</div></a></p></body></html>"#,
    );
    // D-2: html5ever closes `<p>` on a `<div>` start tag, so the a is empty; upstream's libxml2 tree gives "Local News".
    assert_eq!(cats.categories, Some(Vec::new()));

    let lic = extract(
        r#"<html><body><h1>T</h1><footer><a rel="license" href="/legal"><div>Content</div><div>under our terms</div></a></footer></body></html>"#,
    );
    assert_eq!(lic.license.as_deref(), Some("Content under our terms"));

    let inline = extract("<html><body><h1>Hyper<span>link</span>ed</h1></body></html>");
    assert_eq!(inline.title.as_deref(), Some("Hyperlinked"));
}
