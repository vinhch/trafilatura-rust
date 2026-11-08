use super::*;

#[test]
fn test_normalize_authors() {
    // Test if author names are normalized correctly.
    assert_eq!(authors(None, "John Doe").as_deref(), Some("John Doe"));
    assert_eq!(authors(None, "abc").as_deref(), Some("Abc"));
    assert_eq!(
        authors(None, "Steve Steve 123").as_deref(),
        Some("Steve Steve")
    );
    assert_eq!(
        authors(None, "By Steve Steve").as_deref(),
        Some("Steve Steve")
    );
    // no usable name -> returns current
    assert_eq!(authors(None, "123"), None);
    assert_eq!(
        authors(Some("Alice; Bob"), "John Doe").as_deref(),
        Some("Alice; Bob; John Doe")
    );
    assert_eq!(
        authors(Some("Alice; Bob"), "john.doe@example.com").as_deref(),
        Some("Alice; Bob")
    );
    // The escape inputs are built with concat! so the source holds no literal backslash-u sequence.
    assert_eq!(
        authors(None, concat!("\\", "u00e9tienne")).as_deref(),
        Some("Étienne")
    );
    // invalid \u escape must not raise
    assert_eq!(
        authors(None, concat!("Jane", "\\", "uZZ Doe")).as_deref(),
        Some(concat!("Jane", "\\", "uZZ Doe"))
    );
    assert_eq!(authors(None, "&#233;tienne").as_deref(), Some("Étienne"));
    assert_eq!(
        authors(None, "Alice &amp; Bob").as_deref(),
        Some("Alice; Bob")
    );
    assert_eq!(
        authors(None, "<b>John Doe</b>").as_deref(),
        Some("John Doe")
    );
    assert_eq!(authors(None, "John 😊 Doe").as_deref(), Some("John Doe"));
    assert_eq!(
        authors(None, "words by John Doe").as_deref(),
        Some("John Doe")
    );
    assert_eq!(authors(None, "John Doe123").as_deref(), Some("John Doe"));
    assert_eq!(authors(None, "John_Doe").as_deref(), Some("John Doe"));
    assert_eq!(authors(None, "John Doe* ").as_deref(), Some("John Doe"));
    assert_eq!(
        authors(None, "John Doe of John Doe").as_deref(),
        Some("John Doe")
    );
    assert_eq!(
        authors(None, "John Doe — John Doe").as_deref(),
        Some("John Doe")
    );
    assert_eq!(
        authors(None, r#"John "The King" Doe"#).as_deref(),
        Some("John  Doe")
    );
    // dedup keeps the fullest form regardless of order
    assert_eq!(
        authors(Some("Smith"), "John Smith").as_deref(),
        Some("John Smith")
    );
    assert_eq!(
        authors(Some("John Smith"), "Smith").as_deref(),
        Some("John Smith")
    );
}

#[test]
fn test_normalize_json() {
    // Test JSON string normalization (escapes, surrogates, HTML).
    assert_eq!(
        normalize_json(concat!("Test ", "\\", "nthis")).unwrap(),
        "Test this"
    );
    assert_eq!(
        normalize_json(concat!(
            "\\",
            "uD800",
            "\\",
            "uDC00Hello",
            "\\",
            "uDBFF",
            "\\",
            "uDFFF"
        ))
        .unwrap(),
        "Hello"
    );
    assert_eq!(normalize_json("Test 【ABC】").unwrap(), "Test 【ABC】");
    assert_eq!(
        normalize_json("Seán Federico O'Murchú").unwrap(),
        "Seán Federico O'Murchú"
    );
}

#[test]
fn test_extract_json_shapes() {
    // JSON-LD container shapes and process_parent field-extraction branches.
    // @graph wrapper
    let metadata = extract_json_of(
        r#"[{"@context": "https://schema.org", "@graph": [{"@type": "Article", "author": {"name": "Jane Doe"}}]}]"#,
    );
    assert_eq!(metadata.author.as_deref(), Some("Jane Doe"));
    // list-valued articleSection
    let metadata =
        process_parent_of(r#"[{"@type": "Article", "articleSection": ["News", "Politics"]}]"#);
    assert_eq!(
        metadata.categories,
        Some(vec!["News".to_owned(), "Politics".to_owned()])
    );
    // a Person with a plain-string name yields the author
    let metadata = process_parent_of(r#"[{"@type": "Person", "name": "Jane Doe"}]"#);
    assert_eq!(metadata.author.as_deref(), Some("Jane Doe"));
    // a longer organization name supersedes a shorter sitename
    let metadata = process_parent_of(
        r#"[{"@type": "Organization", "name": "AB"}, {"@type": "Organization", "name": "ABCD Corp"}]"#,
    );
    assert_eq!(metadata.sitename.as_deref(), Some("ABCD Corp"));
}

#[test]
fn test_json_metadata_robustness() {
    // Malformed/edge JSON-LD shapes must not raise and should extract where possible.
    let call = Call::default();
    let author_1 = JSON_AUTHOR_1.as_ref().expect("JSON_AUTHOR_1 compiles");
    let author_2 = JSON_AUTHOR_2.as_ref().expect("JSON_AUTHOR_2 compiles");
    // "names" key leaves the first author regex group empty
    assert_eq!(
        extract_json_author(r#"{"author":{"names":"Jane Doe"}}"#, author_1, &call)
            .unwrap()
            .as_deref(),
        Some("Jane Doe")
    );
    assert_eq!(
        extract_json_author(r#"{"author":{"name":"Jane Doe"}}"#, author_1, &call)
            .unwrap()
            .as_deref(),
        Some("Jane Doe")
    );
    assert_eq!(
        extract_json_author(r#"{"Person":{"names":"Jane Doe"}}"#, author_2, &call)
            .unwrap()
            .as_deref(),
        Some("Jane Doe")
    );
    // a single-word match (no space) is skipped
    assert_eq!(
        extract_json_author(r#"{"author":{"name":"Cher"}}"#, author_1, &call).unwrap(),
        None
    );
    // publisher as a bare string
    assert_eq!(
        process_parent_of(r#"[{"publisher": "the name of co"}]"#).sitename,
        None
    );
    assert_eq!(
        process_parent_of(r#"[{"publisher": {"name": "Acme"}}]"#)
            .sitename
            .as_deref(),
        Some("Acme")
    );
    // Person.name as a list must not raise
    assert_eq!(
        process_parent_of(r#"[{"@type": "Person", "name": ["Jane", "Doe"]}]"#).author,
        None
    );
    // article author as a list of plain strings must be extracted, not dropped
    assert_eq!(
        process_parent_of(r#"[{"@type": "Article", "author": ["John Doe", "Jane Smith"]}]"#)
            .author
            .as_deref(),
        Some("John Doe; Jane Smith")
    );
    // malformed JSON-LD (non-dict items) is swallowed by extract_metadata, not raised
    assert!(
        common::metadata(
            r#"<html><body><script type="application/ld+json">[123]</script></body></html>"#,
            None,
            true,
            &[]
        )
        .is_some()
    );
}

#[test]
fn test_json_author_type_arrays() {
    // Extract Person authors regardless of whether their type is a string or an array.
    let rows: [(&str, Option<&str>); 9] = [
        (r#"{"name": "Jane Doe"}"#, Some("Jane Doe")),
        (
            r#"{"@type": "Person", "name": "Jane Doe"}"#,
            Some("Jane Doe"),
        ),
        (
            r#"{"@type": ["Person"], "name": "Jane Doe"}"#,
            Some("Jane Doe"),
        ),
        (
            r#"{"@type": ["Thing", "Person"], "name": "Jane Doe"}"#,
            Some("Jane Doe"),
        ),
        (
            r#"{"@type": ["Person", "Thing"], "name": "Jane Doe"}"#,
            Some("Jane Doe"),
        ),
        (r#"{"@type": "Organization", "name": "Example News"}"#, None),
        (
            r#"{"@type": ["Organization"], "name": "Example News"}"#,
            None,
        ),
        (r#"{"@type": [], "name": "Jane Doe"}"#, None),
        (r#"{"@type": null, "name": "Jane Doe"}"#, None),
    ];
    for as_array in [false, true] {
        for (author, expected) in rows {
            let author = if as_array {
                format!("[{author}]")
            } else {
                author.to_owned()
            };
            let metadata = extract(&format!(
                r#"<html><head><script type="application/ld+json">{{"@context": "https://schema.org", "@type": "NewsArticle", "author": {author}}}</script></head></html>"#
            ));
            assert_eq!(metadata.author.as_deref(), expected, "author {author}");
        }
    }
}

#[test]
fn test_json_article_type_arrays() {
    // Extract article metadata when an unrecognized type precedes the article type.
    let article_types: [(&str, &str); 6] = [
        (r#""NewsArticle""#, "newsarticle"),
        (r#"["NewsArticle"]"#, "newsarticle"),
        (r#"["NewsArticle", "Thing"]"#, "newsarticle"),
        (r#"["Thing", "NewsArticle"]"#, "newsarticle"),
        (r#"["CreativeWork", "Article"]"#, "article"),
        (r#"["Thing", "nEwSaRtIcLe"]"#, "newsarticle"),
    ];
    for author_type in [r#""Person""#, r#"["Person"]"#] {
        for (article_type, pagetype) in article_types {
            let metadata = extract(&format!(
                r#"<html><head><script type="application/ld+json">{{"@context": "https://schema.org", "@type": {article_type}, "author": {{"@type": {author_type}, "name": "Jane Doe"}}, "publisher": {{"@type": "Organization", "name": "Example News"}}, "headline": "Example headline", "articleSection": "News"}}</script></head></html>"#
            ));
            assert_eq!(
                metadata.author.as_deref(),
                Some("Jane Doe"),
                "{article_type}"
            );
            assert_eq!(
                metadata.pagetype.as_deref(),
                Some(pagetype),
                "{article_type}"
            );
            assert_eq!(metadata.sitename.as_deref(), Some("Example News"));
            assert_eq!(metadata.title.as_deref(), Some("Example headline"));
            assert_eq!(metadata.categories, Some(vec!["News".to_owned()]));
        }
    }
}

#[test]
fn test_json_node_type_arrays() {
    // Recognize supported node types without changing precedence among known types.
    type Row = (
        &'static str,
        Option<&'static str>,
        Option<&'static str>,
        Option<&'static str>,
    );
    let rows: [Row; 17] = [
        (r#""Person""#, Some("Example Name"), None, None),
        (r#"["Person"]"#, Some("Example Name"), None, None),
        (r#"["Thing", "Person"]"#, Some("Example Name"), None, None),
        (
            r#"["Thing", "Organization"]"#,
            None,
            None,
            Some("Example Name"),
        ),
        (
            r#"["Thing", "WebSite"]"#,
            None,
            Some("website"),
            Some("Example Name"),
        ),
        (r#"["Thing", "FAQPage"]"#, None, Some("faqpage"), None),
        (r#"["Person", "Article"]"#, Some("Example Name"), None, None),
        (
            r#"["Article", "Person"]"#,
            Some("Nested Author"),
            Some("article"),
            None,
        ),
        (
            r#"["WebPage", "NewsArticle"]"#,
            None,
            Some("webpage"),
            Some("Example Name"),
        ),
        (
            r#"["NewsArticle", "WebPage"]"#,
            Some("Nested Author"),
            Some("newsarticle"),
            None,
        ),
        (
            r#"["TechArticle", "Article"]"#,
            None,
            Some("techarticle"),
            None,
        ),
        (
            r#"["NewsArticle", null]"#,
            Some("Nested Author"),
            Some("newsarticle"),
            None,
        ),
        (r#"["Thing", "Unknown"]"#, None, None, None),
        (r#"["Thing", null]"#, None, None, None),
        (r#""Thing""#, None, None, None),
        (r#"[]"#, None, None, None),
        (r#"null"#, None, None, None),
    ];
    for (node_type, author, pagetype, sitename) in rows {
        let metadata = extract(&format!(
            r#"<html><head><script type="application/ld+json">{{"@context": "https://schema.org", "@type": {node_type}, "name": "Example Name", "author": {{"@type": "Person", "name": "Nested Author"}}}}</script></head></html>"#
        ));
        assert_eq!(
            (
                metadata.author.as_deref(),
                metadata.pagetype.as_deref(),
                metadata.sitename.as_deref()
            ),
            (author, pagetype, sitename),
            "@type {node_type}"
        );
    }
}

// Adaptation: upstream counts process_parent calls through a monkeypatch of the module attribute,
// which has no Rust analog. The call count is checked through the check counter in
// json_metadata::tests::flat_list_is_processed_once, so this test checks the list result only.
#[test]
fn test_extract_json_processes_list_once() {
    // A flat list of @context objects is processed once, not once per item.
    let metadata = extract_json_of(
        r#"[{"@context": "https://schema.org", "@type": "NewsArticle", "headline": "A", "author": {"name": "Jane Doe"}}, {"@context": "https://schema.org", "@type": "NewsArticle", "headline": "B", "author": {"name": "Jane Doe"}}]"#,
    );
    assert_eq!(metadata.author.as_deref(), Some("Jane Doe"));
}
