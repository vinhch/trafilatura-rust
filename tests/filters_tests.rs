#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Ports of upstream `tests/filters_tests.py`, case by case with the upstream names: the `extract`
//! lines of `test_filters`.
//!
//! The criterion of each assertion is the upstream result with `fast=True` forced on the html5
//! parse; `fast=False` calls carry a `deviation: D-1` comment. Upstream's tests build their pages
//! with `html.fromstring`, so they run through the tree entry point (`extract_tree`).
//!
//! Not ported here: the `language_filter` and `check_html_lang` lines of `test_filters`, which are
//! unit tests of `utils` (`check_html_lang_matches_python` in `src/utils/tests.rs`; the filter is a
//! stub, OI-4); `test_prune_xpath` (the crate takes no caller selectors, D-7, followup F-16). The
//! lines that need the language detection of OI-4 or the XML output and the URL blacklist are in the
//! `#[ignore]` tests below.

use std::collections::HashSet;

use trafilatura_rust::__internals::{
    Config, Extractor, Format, ParseLimits, Source, Tree, internal_extraction,
};
use trafilatura_rust::Call;

#[allow(dead_code)]
mod trees;
use trees::{ctx_for, lxml_fromstring};

/// `Extractor(config=config)` with the output format of `extract` and `formatting` as given.
fn settings(config: Config, formatting: Option<bool>, lang: Option<&str>) -> Extractor {
    let mut settings = Extractor::new(
        config,
        Format::Txt,
        false,
        false,
        formatting,
        false,
        false,
        HashSet::new(),
        HashSet::new(),
    );
    settings.lang = lang.map(str::to_string);
    settings
}

/// Upstream's `ZERO_CONFIG`.
fn zero_config() -> Config {
    Config {
        min_output_size: 0,
        min_extracted_size: 0,
        ..Config::default()
    }
}

/// `extract(html.fromstring(html), options=settings)`; upstream's `None` is `Err(Rejected(_))`.
fn extract_tree(html: &str, settings: &Extractor) -> Option<String> {
    let (tree, root): (Tree, _) = lxml_fromstring(html);
    let call = Call::default();
    internal_extraction(
        Source::Tree(&tree, root),
        None,
        &ctx_for(settings, &call),
        &ParseLimits::default(),
    )
    .ok()
    .map(|document| document.text.unwrap_or_default())
}

/// `<html><body>` with `count` times `paragraph`.
fn page(paragraph: &str, count: usize) -> String {
    format!("<html><body>{}</body></html>", paragraph.repeat(count))
}

#[test]
fn test_filters() {
    // recursion limit
    let limited = Config {
        max_tree_size: Some(500),
        ..Config::default()
    };
    let options = settings(limited.clone(), None, None);
    let my_p = "<p>abc</p>";
    assert!(extract_tree(&page(my_p, 50), &options).is_some());
    assert!(extract_tree(&page(my_p, 501), &options).is_none());

    let options = settings(limited, Some(true), None);
    let my_p = r##"<p><hi rend="#i">abc</hi></p>"##;
    assert!(extract_tree(&page(my_p, 501), &options).is_none());
    assert!(extract_tree(&page(my_p, 499), &options).is_some());

    // MAX_TREE_SIZE is read from the config (empty value disables the limit)
    assert_eq!(settings(Config::default(), None, None).max_tree_size, None);
    let config = Config {
        max_tree_size: Some(500),
        ..Config::default()
    };
    let options = settings(config, None, None);
    assert_eq!(options.max_tree_size, Some(500));
    let my_p = "<p>abc</p>";
    assert!(extract_tree(&page(my_p, 501), &options).is_none());
    assert!(extract_tree(&page(my_p, 50), &options).is_some());

    // HTML lang filter, text + lang. Upstream's else branch (no `py3langid`), the one the crate has:
    // the `html` tag is the only language check, and it does not reject `de-DE` for `de`
    let my_p = "<p>In sleep a king, but waking no such matter.</p>";
    let options = settings(Config::default(), None, Some("de"));
    // deviation: D-1, upstream runs this one with `fast=False`
    assert!(
        extract_tree(
            &format!(
                r#"<html lang="de-DE"><body>{}</body></html>"#,
                my_p.repeat(50)
            ),
            &options
        )
        .is_some()
    );
}

// The lines of `test_filters` that need the language detection of OI-4: upstream takes this branch
// with `py3langid` installed.
#[test]
#[ignore = "phase 4"]
fn test_filters_phase_4_lines() {
    let zero = |lang: &str| settings(zero_config(), None, Some(lang));
    // lang detection on the content
    let doc = "<html><body><article><p>How many ages hence/Shall this our lofty scene be acted over,/In states unborn and accents yet unknown!</p></article></body></html>";
    assert!(extract_tree(doc, &zero("de")).is_none());
    assert!(extract_tree(doc, &zero("en")).is_some());
    // text + lang
    let my_p = "<p>In sleep a king, but waking no such matter.</p>";
    let page = |lang: &str| {
        format!(
            r#"<html lang="{lang}"><body>{}</body></html>"#,
            my_p.repeat(50)
        )
    };
    let options = |lang: &str| settings(Config::default(), None, Some(lang));
    assert!(extract_tree(&page("en-US"), &options("en")).is_some());
    assert!(extract_tree(&page("en-US"), &options("de")).is_none());
    // caught; deviation: D-1, upstream runs this one with `fast=False`
    assert!(extract_tree(&page("de-DE"), &options("de")).is_none());
}

/// `extract(doc, output_format="xml", config=ZERO_CONFIG, url_blacklist=...)`.
// todo(phase 6): xml output, and phase 4: the URL blacklist
fn extract_xml(_doc: &str, _url_blacklist: &[&str]) -> Option<String> {
    None
}

// The URL blacklist lines of `test_filters`, which also need the XML output.
#[test]
#[ignore = "phase 6"]
fn test_filters_phase_6_lines() {
    // test URL blacklist (non-empty body so the None is caused by the blacklist, not missing content)
    let doc = r#"<html><head><link rel="canonical" href="https://example.org"/></head><body><article><p>Real body text here.</p></article></body></html>"#;
    assert!(extract_xml(doc, &[]).is_some());
    assert!(extract_xml(doc, &["https://example.org"]).is_none());
}
