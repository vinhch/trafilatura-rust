#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Ports of upstream `tests/unit_tests.py`, case by case with the upstream names.

use std::collections::HashSet;

use trafilatura_rust::__internals::{
    BODY_XPATH, Config, Extractor, Focus, Format, OVERALL_DISCARD_XPATH, PRECISION_DISCARD_XPATH,
    ParseLimits, Selector, Tree, alt, parse_document, parse_fragment, sanitize, textfilter, trim,
};
use trafilatura_rust::Call;

/// A `<p>` element in a tree of its own, with the given text.
fn element_with_text(text: &str) -> (Tree, trafilatura_rust::__internals::NodeId) {
    let mut tree = parse_fragment("<p></p>", &ParseLimits::default(), &Call::default()).unwrap();
    let p = tree.children(tree.root()).next().unwrap();
    tree.node_mut(p).text = Some(text.to_string());
    (tree, p)
}

/// Whether any of the selectors selects a `<tag attrs>` wrapping a paragraph.
// upstream: tests/unit_tests.py::_xpath_hits
// Upstream parses the snippet with `etree.fromstring`; the html5 parse selects the same element for
// these snippets.
fn xpath_hits(xpaths: &[Selector], attrs: &str, tag: &str) -> bool {
    let html = format!("<html><body><{tag} {attrs}><p>content</p></{tag}></body></html>");
    let call = Call::default();
    let tree = parse_document(&html, &ParseLimits::default(), &call).unwrap();
    xpaths
        .iter()
        .any(|select| !select(&tree, tree.root(), &call).unwrap().is_empty())
}

fn extractor(
    format: Format,
    precision: bool,
    recall: bool,
    formatting: Option<bool>,
    with_metadata: (bool, bool),
    author_blacklist: &[&str],
    url_blacklist: &[&str],
) -> Extractor {
    let set = |items: &[&str]| {
        items
            .iter()
            .map(|s| (*s).to_string())
            .collect::<HashSet<_>>()
    };
    Extractor::new(
        Config::default(),
        format,
        precision,
        recall,
        formatting,
        with_metadata.0,
        with_metadata.1,
        set(author_blacklist),
        set(url_blacklist),
    )
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

// Reading a cfg file is upstream API (`use_config`) that this crate does not offer, so the
// file-based assertions are not ported; the `extract` assertions land in phase 3f.
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
}

#[test]
fn extractor_resolves_like_upstream() {
    // oracle: Extractor(...) attributes, see docs of Extractor.__init__ in settings.py
    let plain = |format| extractor(format, false, false, None, (false, false), &[], &[]);
    assert!(plain(Format::Markdown).formatting);
    assert!(!plain(Format::Txt).formatting);
    assert!(
        !extractor(
            Format::Markdown,
            false,
            false,
            Some(false),
            (false, false),
            &[],
            &[]
        )
        .formatting
    );
    assert!(
        extractor(
            Format::Json,
            false,
            false,
            Some(true),
            (false, false),
            &[],
            &[]
        )
        .formatting
    );
    let focus = |precision, recall| {
        extractor(
            Format::Txt,
            precision,
            recall,
            None,
            (false, false),
            &[],
            &[],
        )
        .focus
    };
    assert_eq!(focus(true, true), Focus::Recall);
    assert_eq!(focus(false, true), Focus::Recall);
    assert_eq!(focus(true, false), Focus::Precision);
    assert_eq!(focus(false, false), Focus::Balanced);
    assert!(plain(Format::XmlTei).with_metadata);
    assert!(!plain(Format::Txt).with_metadata);
    let with_metadata = |flags, authors: &[&str], urls: &[&str]| {
        extractor(Format::Txt, false, false, None, flags, authors, urls).with_metadata
    };
    assert!(with_metadata((true, false), &[], &[]));
    assert!(with_metadata((false, true), &[], &[]));
    assert!(with_metadata((false, false), &["x"], &[]));
    assert!(with_metadata((false, false), &[], &["https://a.test"]));
    assert!(!with_metadata((false, false), &[], &[]));
    // the other defaults of upstream's Extractor
    let defaults = plain(Format::Txt);
    assert!(defaults.comments && defaults.tables);
    assert!(!defaults.links && !defaults.images && !defaults.dedup);
    assert_eq!(
        (defaults.min_extracted_size, defaults.min_duplcheck_size),
        (250, 100)
    );
    assert_eq!(
        (defaults.max_repetitions, defaults.max_tree_size),
        (2, None)
    );
    assert_eq!(
        (defaults.max_file_size, defaults.min_file_size),
        (20_000_000, 10)
    );
    assert!(defaults.date_params.original_date && defaults.date_params.extensive_search);
}

#[test]
fn test_overall_discard_legacy_tokens() {
    for cls in ["yin", "zlylin", "mol-factbox"] {
        assert!(
            xpath_hits(OVERALL_DISCARD_XPATH, &format!(r#"class="{cls}""#), "div"),
            "{cls}"
        );
    }
    // removed upstream on 2026-07-10
    assert!(!xpath_hits(OVERALL_DISCARD_XPATH, r#"class="xg1""#, "div"));
}

#[test]
fn test_overall_discard_matches_both_attributes() {
    let discarded = |attrs: &str| xpath_hits(OVERALL_DISCARD_XPATH, attrs, "div");
    // token in @id, class written first
    assert!(discarded(r#"class="x" id="author-box""#));
    // token in @class, id written first
    assert!(discarded(r#"id="x" class="sidebar""#));
    // cookie exception: class-first protects
    assert!(!discarded(r#"class="hidden-x" id="cookieBanner""#));
}

#[test]
fn test_xpath_alt_rejects_empty_group() {
    assert_eq!(alt(&["a", "b"]).as_deref(), Some("a|b"));
    // upstream raises ValueError
    assert_eq!(alt(&[]), None);
}

#[test]
fn test_precision_discard_link_token_only() {
    let discarded = |attrs: &str, tag: &str| xpath_hits(PRECISION_DISCARD_XPATH, attrs, tag);
    // standalone token still dropped (intended)
    assert!(discarded(r#"class="link""#, "div"));
    // token among others
    assert!(discarded(r#"class="nav link""#, "div"));
    // compound: no longer dropped
    assert!(!discarded(r#"class="article-permalink""#, "div"));
    assert!(!discarded(r#"class="headline-link""#, "div"));
    assert!(!discarded(r#"class="featured-link--wrap""#, "div"));
    // 'bottom' still matched at a token boundary
    assert!(discarded(r#"class="article-bottom""#, "div"));
    // .//header still discarded
    assert!(discarded(r#"class="site-header""#, "header"));
}

#[test]
fn test_precision_discard_bottom_token_boundary() {
    let discarded = |attrs: &str| xpath_hits(PRECISION_DISCARD_XPATH, attrs, "div");
    // page-bottom chrome: still discarded, whichever end of the token 'bottom' sits at
    assert!(discarded(r#"class="bottom""#));
    assert!(discarded(r#"class="article-bottom""#));
    assert!(discarded(r#"class="bottom-bar""#));
    assert!(discarded(r#"id="bottom""#));
    assert!(discarded(r#"class="wrapper bottom""#));
    // spacing/border utilities on real content: 'bottom' mid-token, must NOT be discarded
    assert!(!discarded(r#"class="Padding-bottom-lg-30""#));
    assert!(!discarded(r#"class="col-8 Padding-bottom-lg-30 floatL""#));
    assert!(!discarded(r#"class="border-bottom-0""#));
    assert!(!discarded(r#"class="mb-bottom-3""#));
}

#[test]
fn test_body_xpath_fulltext_class() {
    for cls in [
        "fulltext",
        "FullText",
        "fullText",
        "FULLTEXT",
        "article-fulltext",
    ] {
        assert!(
            xpath_hits(BODY_XPATH, &format!(r#"class="{cls}""#), "div"),
            "{cls}"
        );
    }
    // GH#780's reported false positive: still matches upstream (documented, not fixed there)
    assert!(xpath_hits(BODY_XPATH, r#"class="FulltextWrapper""#, "div"));
}
