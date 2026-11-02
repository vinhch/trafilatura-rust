#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Port of upstream `tests/metadata_tests.py` (trafilatura 2.3.1). Each test keeps its upstream name
//! and its cases in upstream order; an adaptation is commented where the test differs.

#[allow(dead_code)] // the helpers this binary does not call belong to the other test binaries
mod common;

use std::collections::HashSet;

use common::{blank_document, extract, json_of, parse_html};

use trafilatura_rust::__internals::{
    Config, DateParams, Extractor, Format, JSON_MINIFY, NodeId, Selector, Tree, check_authors,
    extract_json, extract_json_parse_error, extract_meta_json, extract_metadata, extract_metainfo,
    extract_title, extract_url, normalize_tags, process_parent,
};
use trafilatura_rust::{Call, ExtractError};

fn owned(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

/// `extract_url(html.fromstring(html))`.
fn url_of(html: &str) -> Option<String> {
    let (tree, root) = parse_html(html);
    extract_url(&tree, root, None, &Call::default()).unwrap()
}

/// `extract_title(html.fromstring(html))`.
fn title_of(html: &str) -> Option<String> {
    let (mut tree, root) = parse_html(html);
    extract_title(&mut tree, root, &Call::default()).unwrap()
}

/// `.//p[@class]`: the `p` elements with a `class` attribute, in document order.
fn paragraphs_with_class(
    tree: &Tree,
    root: NodeId,
    _call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    Ok(tree
        .iterdescendants(root, &["p"])
        .filter(|&node| tree.get(node, "class").is_some())
        .collect())
}

/// `.//p[@type]`: the `p` elements with a `type` attribute, in document order.
fn paragraphs_with_type(
    tree: &Tree,
    root: NodeId,
    _call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    Ok(tree
        .iterdescendants(root, &["p"])
        .filter(|&node| tree.get(node, "type").is_some())
        .collect())
}

#[test]
fn test_tags_with_complete_opengraph_metadata() {
    let document = r#"<html><head>
        <meta property="og:title" content="Example article"/>
        <meta property="og:author" content="Jane Smith"/>
        <meta property="og:url" content="https://example.org/post"/>
        <meta property="og:description" content="Example description"/>
        <meta property="og:site_name" content="Example News"/>
        <meta property="og:image" content="https://example.org/image.jpg"/>
        <meta name="keywords" content="science, research"/>
        <meta property="article:tag" content="astronomy"/>
        <meta name="twitter:title" content="Alternative title"/>
        </head><body><article>Example article content.</article></body></html>"#;

    let metadata = extract(document);
    assert_eq!(
        metadata.tags,
        Some(owned(&["science, research", "astronomy"]))
    );
    assert_eq!(metadata.title.as_deref(), Some("Example article"));
    assert_eq!(metadata.author.as_deref(), Some("Jane Smith"));
    assert_eq!(
        metadata.image.as_deref(),
        Some("https://example.org/image.jpg")
    );
}

#[test]
fn test_titles() {
    // Upstream's docstring: "Test the extraction of titles".
    let tests: [(&str, Option<&str>); 14] = [
        (
            r#"<html><body><h3 class="title">T</h3><h3 id="title"></h3></body></html>"#,
            None,
        ),
        (
            r#"<html><head><title>Test Title</title><meta property="og:title" content=" " /></head><body><h1>First</h1></body></html>"#,
            Some("First"),
        ),
        (
            r#"<html><head><title>Test Title</title><meta name="title" content=" " /></head><body><h1>First</h1></body></html>"#,
            Some("First"),
        ),
        (
            "<html><head><title>Test Title</title></head><body></body></html>",
            Some("Test Title"),
        ),
        (
            "<html><body><h1>First</h1><h1>Second</h1></body></html>",
            Some("First"),
        ),
        (
            r#"<html><body><h1>   </h1><div class="post-title">Test Title</div></body></html>"#,
            Some("Test Title"),
        ),
        (
            r#"<html><body><h2 class="block-title">Main menu</h2><h1 class="article-title">Test Title</h1></body></html>"#,
            Some("Test Title"),
        ),
        (
            "<html><body><h2>First</h2><h1>Second</h1></body></html>",
            Some("Second"),
        ),
        (
            "<html><body><h2>First</h2><h2>Second</h2></body></html>",
            Some("First"),
        ),
        ("<html><body><title></title></body></html>", None),
        // head title is preferred over h1s when it exists and is not a domain
        (
            "<html><head><title>Head Title</title></head><body><h1>First</h1><h1>Second</h1></body></html>",
            Some("Head Title"),
        ),
        // domain-like head titles are ignored, falling back to the first h1
        (
            "<html><head><title>example.com</title></head><body><h1>First</h1><h1>Second</h1></body></html>",
            Some("First"),
        ),
        // the first non-empty h1 is used when earlier h1s are blank
        (
            "<html><body><h1>   </h1><h1>Real Title</h1></body></html>",
            Some("Real Title"),
        ),
        // all-blank h1s fall through to nothing
        ("<html><body><h1>   </h1><h1>   </h1></body></html>", None),
    ];

    for (doc, expected_title) in tests {
        let metadata = extract(doc);
        assert_eq!(metadata.title.as_deref(), expected_title, "{doc}");
    }

    let metadata = extract(
        r#"<html><body><script type="application/ld+json">{"@context":"https:\/\/schema.org","@type":"Article","name":"Semantic satiation","url":"https:\/\/en.wikipedia.org\/wiki\/Semantic_satiation","sameAs":"http:\/\/www.wikidata.org\/entity\/Q226007","mainEntity":"http:\/\/www.wikidata.org\/entity\/Q226007","author":{"@type":"Organization","name":"Contributors to Wikimedia projects"},"publisher":{"@type":"Organization","name":"Wikimedia Foundation, Inc.","logo":{"@type":"ImageObject","url":"https:\/\/www.wikimedia.org\/static\/images\/wmf-hor-googpub.png"}},"datePublished":"2006-07-12T09:27:14Z","dateModified":"2020-08-31T23:55:26Z","headline":"psychological phenomenon in which repetition causes a word to temporarily lose meaning for the listener"}</script>
<script>(RLQ=window.RLQ||[]).push(function(){mw.config.set({"wgBackendResponseTime":112,"wgHostname":"mw2373"});});</script></html>"#,
    );
    assert_eq!(metadata.title.as_deref(), Some("Semantic satiation"));
    let metadata = extract("<html><head><title> - Home</title></head><body/></html>");
    assert_eq!(metadata.title.as_deref(), Some("- Home"));
    let metadata = extract("<html><head><title>My Title » My Website</title></head><body/></html>");
    // upstream's TODO: and metadata.sitename == "My Website"
    assert_eq!(metadata.title.as_deref(), Some("My Title"));
}

#[test]
fn test_author_blacklist() {
    // Filtering of author names against a blacklist
    let metadata = common::metadata(
        r#"<html><head><meta itemprop="author" content="Jenny Smith"/></head><body></body></html>"#,
        None,
        true,
        &["Jenny Smith"],
    )
    .unwrap();
    assert_eq!(metadata.author, None);
    // a JSON-LD author is filtered (first check, before the markup fallback)
    let metadata = common::metadata(
        r#"<html><body><script type="application/ld+json">{"@context":"https://schema.org","@type":"Article","author":{"name":"Jane Doe"}}</script></body></html>"#,
        None,
        true,
        &["Jane Doe"],
    )
    .unwrap();
    assert_eq!(metadata.author, None);
    // a markup-extracted author is filtered on the recheck pass
    let metadata = common::metadata(
        r#"<html><body><a class="author">Jane Doe</a></body></html>"#,
        None,
        true,
        &["Jane Doe"],
    )
    .unwrap();
    assert_eq!(metadata.author, None);
    // a single-word meta author is discarded
    assert_eq!(
        extract(r#"<html><head><meta name="author" content="Cher"/></head><body/></html>"#).author,
        None
    );
    let blacklist: HashSet<String> = owned(&["A", "b"]).into_iter().collect();
    assert_eq!(
        check_authors("a; B; c; d", &blacklist).as_deref(),
        Some("c; d")
    );
    assert_eq!(
        check_authors("a;B;c;d", &blacklist).as_deref(),
        Some("c; d")
    );
    // regression: author_blacklist alone must enable with_metadata. Adaptation: the settings
    // Extractor is built from a Config, as tests/unit_tests.rs does.
    let with_blacklist = Extractor::new(
        Config::default(),
        Format::Txt,
        false,
        false,
        None,
        false,
        false,
        HashSet::from(["X".to_owned()]),
        HashSet::new(),
    );
    assert!(with_blacklist.with_metadata);
    let without_blacklist = Extractor::new(
        Config::default(),
        Format::Txt,
        false,
        false,
        None,
        false,
        false,
        HashSet::new(),
        HashSet::new(),
    );
    assert!(!without_blacklist.with_metadata);
}

#[test]
fn test_author_from_meta() {
    // Author extraction from <head> meta tags
    let begin = "<html><head>";
    let end = "</head><body></body></html>";
    let cases = [
        (
            format!(r#"{begin}<meta itemprop="author" content="Jenny Smith"/>{end}"#),
            "Jenny Smith",
        ),
        (
            format!(
                r#"{begin}<meta itemprop="author" content="Jenny Smith"/><meta itemprop="author" content="John Smith"/>{end}"#
            ),
            "Jenny Smith; John Smith",
        ),
        (
            format!(
                r#"{begin}<meta itemprop="author" content="Jenny Smith und John Smith"/>{end}"#
            ),
            "Jenny Smith; John Smith",
        ),
        (
            format!(
                r#"{begin}<meta name="author" content="Jenny Smith"/><meta name="author" content="John Smith"/>{end}"#
            ),
            "Jenny Smith; John Smith",
        ),
        (
            format!(r#"{begin}<meta name="author" content="Jenny Smith and John Smith"/>{end}"#),
            "Jenny Smith; John Smith",
        ),
        (
            format!(r#"{begin}<meta name="author" content="Jenny Smith"/>{end}"#),
            "Jenny Smith",
        ),
        (
            format!(r#"{begin}<meta name="author" content="Hank O&#39;Hop"/>{end}"#),
            "Hank O'Hop",
        ),
        (
            format!(r#"{begin}<meta name="author" content="Jenny Smith ❤️"/>{end}"#),
            "Jenny Smith",
        ),
        (
            format!(
                r#"{begin}<meta name="citation_author" content="Jenny Smith and John Smith"/>{end}"#
            ),
            "Jenny Smith; John Smith",
        ),
        (
            format!(
                r#"{begin}<meta property="author" content="Jenny Smith"/><meta property="author" content="John Smith"/>{end}"#
            ),
            "Jenny Smith; John Smith",
        ),
        (
            format!(
                r#"{begin}<meta itemprop="author" content="Jenny Smith and John Smith"/>{end}"#
            ),
            "Jenny Smith; John Smith",
        ),
        (
            format!(r#"{begin}<meta name="article:author" content="Jenny Smith"/>{end}"#),
            "Jenny Smith",
        ),
    ];
    for (doc, expected) in cases {
        assert_eq!(extract(&doc).author.as_deref(), Some(expected), "{doc}");
    }
}

#[test]
fn test_author_from_markup() {
    // Author extraction from body markup
    let begin = "<html><body>";
    let end = "</body></html>";
    let cases: [(String, Option<&str>); 33] = [
        (
            format!(r#"{begin}<a href="" rel="author">Jenny Smith</a>{end}"#),
            Some("Jenny Smith"),
        ),
        (
            format!(r#"{begin}<a href="" rel="author">Jenny "The Author" Smith</a>{end}"#),
            Some("Jenny Smith"),
        ),
        (
            format!(r#"{begin}<span class="author">Jenny Smith</span>{end}"#),
            Some("Jenny Smith"),
        ),
        (
            format!(r#"{begin}<h4 class="author">Jenny Smith</h4>{end}"#),
            Some("Jenny Smith"),
        ),
        (
            format!(r#"{begin}<h4 class="author">Jenny Smith — Trafilatura</h4>{end}"#),
            Some("Jenny Smith"),
        ),
        (
            format!(r#"{begin}<span class="wrapper--detail__writer">Jenny Smith</span>{end}"#),
            Some("Jenny Smith"),
        ),
        (
            format!(r#"{begin}<span id="author-name">Jenny Smith</span>{end}"#),
            Some("Jenny Smith"),
        ),
        (
            format!(r#"{begin}<figure data-component="Figure"><div class="author">Jenny Smith</div></figure>{end}"#),
            None,
        ),
        (
            format!(r#"{begin}<div class="sidebar"><div class="author">Jenny Smith</div></figure>{end}"#),
            None,
        ),
        (
            format!(
                r#"{begin}<div class="quote"><p>My quote here</p><p class="quote-author"><span>—</span> Jenny Smith</p></div>{end}"#
            ),
            None,
        ),
        (
            format!(r#"{begin}<span class="author">Jenny Smith and John Smith</span>{end}"#),
            Some("Jenny Smith; John Smith"),
        ),
        (
            format!(r#"{begin}<a class="author">Jenny Smith</a>{end}"#),
            Some("Jenny Smith"),
        ),
        (
            format!(r#"{begin}<a class="author">Jenny Smith <div class="title">Editor</div></a>{end}"#),
            Some("Jenny Smith"),
        ),
        (
            format!(r#"{begin}<a class="author">Jenny Smith from Trafilatura</a>{end}"#),
            Some("Jenny Smith"),
        ),
        (
            format!(
                r#"{begin}<meta itemprop="author" content="Fake Author"/><a class="author">Jenny Smith from Trafilatura</a>{end}"#
            ),
            Some("Jenny Smith"),
        ),
        (
            format!(r#"{begin}<a class="username">Jenny Smith</a>{end}"#),
            Some("Jenny Smith"),
        ),
        (
            format!(r#"{begin}<div class="submitted-by"><a>Jenny Smith</a></div>{end}"#),
            Some("Jenny Smith"),
        ),
        (
            format!(
                r#"{begin}<div class="byline-content"><div class="byline"><a>Jenny Smith</a></div><time>July 12, 2021 08:05</time></div>{end}"#
            ),
            Some("Jenny Smith"),
        ),
        (
            format!(r#"{begin}<h3 itemprop="author">Jenny Smith</h3>{end}"#),
            Some("Jenny Smith"),
        ),
        (
            format!(
                r#"{begin}<div class="article-meta article-meta-byline article-meta-with-photo article-meta-author-and-reviewer" itemprop="author" itemscope="" itemtype="http://schema.org/Person"><span class="article-meta-photo-wrap"><img src="" alt="Jenny Smith" itemprop="image" class="article-meta-photo"></span><span class="article-meta-contents"><span class="article-meta-author">By <a href="" itemprop="url"><span itemprop="name">Jenny Smith</span></a></span><span class="article-meta-date">May 18 2022</span><span class="article-meta-reviewer">Reviewed by <a href="">Robert Smith</a></span></span></div>{end}"#
            ),
            Some("Jenny Smith"),
        ),
        (
            format!(r#"{begin}<div data-component="Byline">Jenny Smith</div>{end}"#),
            Some("Jenny Smith"),
        ),
        (
            format!(r#"{begin}<span id="author">Jenny Smith</span>{end}"#),
            Some("Jenny Smith"),
        ),
        (
            format!(r#"{begin}<span id="author">Jenny Smith – The Moon</span>{end}"#),
            Some("Jenny Smith"),
        ),
        (
            format!(r#"{begin}<span id="author">Jenny_Smith</span>{end}"#),
            Some("Jenny Smith"),
        ),
        (
            format!(
                r#"{begin}<span itemprop="author name">Shannon Deery, Mitch Clarke, Susie O’Brien, Laura Placella, Kara Irving, Jordy Atkinson, Suzan Delibasic</span>{end}"#
            ),
            Some(
                "Shannon Deery; Mitch Clarke; Susie O’Brien; Laura Placella; Kara Irving; Jordy Atkinson; Suzan Delibasic",
            ),
        ),
        (
            format!(r#"{begin}<address class="author">Jenny Smith</address>{end}"#),
            Some("Jenny Smith"),
        ),
        (
            format!(r#"{begin}<author>Jenny Smith</author>{end}"#),
            Some("Jenny Smith"),
        ),
        (
            r#"<html><head><meta data-rh="true" property="og:author" content="By &lt;a href=&quot;/profiles/amir-vera&quot;&gt;Amir Vera&lt;/a&gt;, Seán Federico O&#x27;Murchú, &lt;a href=&quot;/profiles/tara-subramaniam&quot;&gt;Tara Subramaniam&lt;/a&gt; and Adam Renton, CNN"/></head><body></body></html>"#
                .to_owned(),
            Some("Amir Vera; Seán Federico O'Murchú; Tara Subramaniam; Adam Renton; CNN"),
        ),
        (
            format!(
                r#"{begin}<div class="author"><span class="profile__name"> Jenny Smith </span> <a href="https://twitter.com/jenny_smith" class="profile__social" target="_blank"> @jenny_smith </a> <span class="profile__extra lg:hidden"> 11:57AM </span> </div>{end}"#
            ),
            Some("Jenny Smith"),
        ),
        (
            format!(
                r#"{begin}<p class="author-section byline-plain">By <a class="author" rel="nofollow">Jenny Smith For Daily Mail Australia</a></p>{end}"#
            ),
            Some("Jenny Smith"),
        ),
        (
            format!(
                r#"{begin}<div class="o-Attribution__a-Author"><span class="o-Attribution__a-Author--Label">By:</span><span class="o-Attribution__a-Author--Prefix"><span class="o-Attribution__a-Name"><a href="//web.archive.org/web/20210707074846/https://www.discovery.com/profiles/ian-shive">Ian Shive</a></span></span></div>{end}"#
            ),
            Some("Ian Shive"),
        ),
        (
            format!(
                r#"{begin}<div class="ArticlePage-authors"><div class="ArticlePage-authorName" itemprop="name"><span class="ArticlePage-authorBy">By&nbsp;</span><a aria-label="Ben Coxworth" href="https://newatlas.com/author/ben-coxworth/"><span>Ben Coxworth</span></a></div></div>{end}"#
            ),
            Some("Ben Coxworth"),
        ),
        (
            format!(
                r#"{begin}<div><strong><a class="d1dba0c3091a3c30ebd6" data-testid="AuthorURL" href="/by/p535y1">AUTHOR NAME</a></strong></div>{end}"#
            ),
            Some("AUTHOR NAME"),
        ),
    ];
    for (doc, expected) in cases {
        assert_eq!(extract(&doc).author.as_deref(), expected, "{doc}");
    }
}

#[test]
fn test_url() {
    // Test URL extraction
    let expected = Some("https://example.org");
    let cases = [
        r#"<html><head><meta property="og:url" content="https://example.org"/></head><body></body></html>"#,
        r#"<html><head><link rel="canonical" href="https://example.org"/></head><body></body></html>"#,
        r#"<html><head><meta name="twitter:url" content="https://example.org"/></head><body></body></html>"#,
        r#"<html><head><link rel="alternate" hreflang="x-default" href="https://example.org"/></head><body></body></html>"#,
        r#"<html><head><link rel="canonical" href="/article/medical-record"/></head><body></body></html>"#,
        r#"<html><head><base href="https://example.org" target="_blank"/></head><body></body></html>"#,
    ];
    let default_urls = [None, None, None, None, Some("https://example.org"), None];
    for (doc, default_url) in cases.into_iter().zip(default_urls) {
        let metadata = common::metadata(doc, default_url, true, &[]).unwrap();
        assert_eq!(metadata.url.as_deref(), expected, "{doc}");
    }

    // relative canonical joined via a twitter:url or og:url base
    assert_eq!(
        url_of(
            r#"<html><head><link rel="canonical" href="/article/medical-record"/><meta name="twitter:url" content="https://example.org"/></head><body></body></html>"#
        )
        .as_deref(),
        Some("https://example.org/article/medical-record")
    );
    assert_eq!(
        url_of(
            r#"<html><head><link rel="canonical" href="/p"/><meta property="og:url" content="https://example.org"/></head><body></body></html>"#
        )
        .as_deref(),
        Some("https://example.org/p")
    );
    // malformed base skipped
    assert_eq!(
        url_of(
            r#"<html><head><link rel="canonical" href="/p"/><meta property="og:url" content="//[::1"/><meta name="twitter:url" content="https://example.org"/></head><body></body></html>"#
        )
        .as_deref(),
        Some("https://example.org/p")
    );
    // protocol-relative canonical keeps its own host
    for doc in [
        r#"<html><head><link rel="canonical" href="//example.org/p"/></head><body></body></html>"#,
        r#"<html><head><link rel="canonical" href="//example.org/p"/><meta property="og:image" content="https://cdn.example.net/i.png"/></head><body></body></html>"#,
    ] {
        assert_eq!(
            url_of(doc).as_deref(),
            Some("https://example.org/p"),
            "{doc}"
        );
    }
}

#[test]
fn test_description() {
    // Test the extraction of descriptions
    let metadata = extract(
        r#"<html><head><meta itemprop="description" content="Description"/></head><body></body></html>"#,
    );
    assert_eq!(metadata.description.as_deref(), Some("Description"));
    let metadata = extract(
        r#"<html><head><meta property="og:description" content="&amp;#13; A Northern Territory action plan, which includes plans to support development and employment on Aboriginal land, has received an update. &amp;#13..." /></head><body></body></html>"#,
    );
    assert_eq!(
        metadata.description.as_deref(),
        Some(
            "A Northern Territory action plan, which includes plans to support development and employment on Aboriginal land, has received an update. ..."
        )
    );
}

#[test]
fn test_dates() {
    // Simple tests for date extraction (most of the tests are carried out externally for htmldate module)
    let tests = [
        (
            r#"<html><head><meta property="og:published_time" content="2017-09-01"/></head><body></body></html>"#,
            "2017-09-01",
            false,
        ),
        (
            r#"<html><head><meta property="og:url" content="https://example.org/2017/09/01/content.html"/></head><body></body></html>"#,
            "2017-09-01",
            false,
        ),
        (
            r#"<html><head><meta property="og:url" content="https://example.org/2017/09/01/content.html"/></head><body></body></html>"#,
            "2017-09-01",
            false,
        ),
        (
            "<html><body><p>Veröffentlicht am 1.9.17</p></body></html>",
            "2017-09-01",
            true,
        ),
        (
            "<html><body><p>Veröffentlicht am 1.9.17</p></body></html>",
            "2017-09-01",
            false,
        ),
    ];

    for (doc, expected, extensive) in tests {
        let metadata = common::metadata(doc, None, extensive, &[]).unwrap();
        assert_eq!(metadata.date.as_deref(), Some(expected), "{doc}");
    }
}

#[test]
fn test_sitename() {
    // Test extraction of site name
    let tests = [
        (
            r#"<html><head><meta name="article:publisher" content="@"/></head><body/></html>"#,
            None,
        ),
        (
            r#"<html><head><meta name="article:publisher" content="The Newspaper"/></head><body/></html>"#,
            Some("The Newspaper"),
        ),
        (
            r#"<html><head><meta property="article:publisher" content="The Newspaper"/></head><body/></html>"#,
            Some("The Newspaper"),
        ),
        (
            "<html><head><title>sitemaps.org - Home</title></head><body/></html>",
            Some("sitemaps.org"),
        ),
        // fallback: derive the site name from the URL host
        (
            r#"<html><head><meta property="og:url" content="https://www.example.org/article"/></head><body/></html>"#,
            Some("example.org"),
        ),
        // backup site name from application-name
        (
            r#"<html><head><meta name="application-name" content="MySite"/></head><body/></html>"#,
            Some("MySite"),
        ),
        // a no-dot lowercase publisher is title-cased
        (
            r#"<html><head><meta name="article:publisher" content="newspaper"/></head><body/></html>"#,
            Some("Newspaper"),
        ),
    ];

    for (doc, expected) in tests {
        let metadata = extract(doc);
        assert_eq!(metadata.sitename.as_deref(), expected, "{doc}");
    }
}

#[test]
fn test_meta() {
    // Test extraction out of meta-elements. Adaptation: an lxml XPath object is a one-selector slice
    // of a Selector fn that walks the same elements.
    let (tree, root) = parse_html(
        "<html><p class='test'>a</p><p class='other'>b</p><p type='this'>cde</p></html>",
    );
    let call = Call::default();
    let with_class: [Selector; 1] = [paragraphs_with_class];
    let with_type: [Selector; 1] = [paragraphs_with_type];
    assert_eq!(
        extract_metainfo(&tree, root, &with_class, 200, &call).unwrap(),
        None
    );
    assert_eq!(
        extract_metainfo(&tree, root, &with_type, 200, &call)
            .unwrap()
            .as_deref(),
        Some("cde")
    );

    let metadata = extract(
        r#"<html><head><meta property="og:title" content="Open Graph Title"/><meta property="og:author" content="Jenny Smith"/><meta property="og:description" content="This is an Open Graph description"/><meta property="og:site_name" content="My first site"/><meta property="og:url" content="https://example.org/test"/><meta property="og:type" content="Open Graph Type"/></head><body><a rel="license" href="https://creativecommons.org/">Creative Commons</a></body></html>"#,
    );
    assert_eq!(metadata.pagetype.as_deref(), Some("Open Graph Type"));
    assert_eq!(metadata.title.as_deref(), Some("Open Graph Title"));
    assert_eq!(metadata.author.as_deref(), Some("Jenny Smith"));
    assert_eq!(
        metadata.description.as_deref(),
        Some("This is an Open Graph description")
    );
    assert_eq!(metadata.sitename.as_deref(), Some("My first site"));
    assert_eq!(metadata.url.as_deref(), Some("https://example.org/test"));
    assert_eq!(metadata.license.as_deref(), Some("Creative Commons"));

    // all OpenGraph fields present -> early return before the meta loop
    let metadata = extract(
        r#"<html><head><meta property="og:title" content="T"/><meta property="og:author" content="A B"/><meta property="og:url" content="https://e.org"/><meta property="og:description" content="D"/><meta property="og:site_name" content="S"/><meta property="og:image" content="https://e.org/i.jpg"/></head><body/></html>"#,
    );
    assert_eq!(metadata.title.as_deref(), Some("T"));
    assert_eq!(metadata.image.as_deref(), Some("https://e.org/i.jpg"));

    let metadata = extract(
        r#"<html><head><meta name="dc.title" content="Open Graph Title"/><meta name="dc.creator" content="Jenny Smith"/><meta name="dc.description" content="This is an Open Graph description"/></head><body></body></html>"#,
    );
    assert_eq!(metadata.title.as_deref(), Some("Open Graph Title"));
    assert_eq!(metadata.author.as_deref(), Some("Jenny Smith"));
    assert_eq!(
        metadata.description.as_deref(),
        Some("This is an Open Graph description")
    );

    let metadata = extract(
        r#"<html><head><meta itemprop="headline" content="Title"/></head><body></body></html>"#,
    );
    assert_eq!(metadata.title.as_deref(), Some("Title"));

    // catch errors. Adaptation: upstream's empty Document() is metadata of "", which is None here.
    assert!(common::metadata("", None, true, &[]).is_none());
    let metadata = extract("<html><title></title></html>");
    assert_eq!(metadata.sitename, None);
    let long_title = format!(
        "<html><head><title>{}</title></head></html>",
        "AAA".repeat(10000)
    );
    let metadata = extract(&long_title);
    let title = metadata.title.unwrap();
    assert!(title.ends_with('…'));
    assert_eq!(title.chars().count(), 10000);
    assert_eq!(
        extract(r#"<html><head><meta otherkey="example" content="Unknown text"/></head></html>"#)
            .title,
        None
    );
    assert_eq!(
        extract("<html><head><title></title><title></title><title></title></head></html>").title,
        None
    );
    assert_eq!(
        extract(r#"<html><body><script type="application/ld+json"></script></body></html>"#).title,
        None
    );
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
    // D-2: html5ever closes the p before the div, so the a is empty; upstream's libxml2 tree gives "Local News".
    assert_eq!(cats.categories, Some(Vec::new()));

    let lic = extract(
        r#"<html><body><h1>T</h1><footer><a rel="license" href="/legal"><div>Content</div><div>under our terms</div></a></footer></body></html>"#,
    );
    assert_eq!(lic.license.as_deref(), Some("Content under our terms"));

    let inline = extract("<html><body><h1>Hyper<span>link</span>ed</h1></body></html>");
    assert_eq!(inline.title.as_deref(), Some("Hyperlinked"));
}
