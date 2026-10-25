#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Ports of upstream `tests/unit_tests.py`, case by case with the upstream names.
//!
//! `extract` runs through the public API (`Kw` holds the keyword arguments); a test that passes an
//! `html.fromstring` tree uses the tree entry point (`extract_tree`) over the html5 tree. The
//! criterion of every assertion is the upstream result with `fast=True` forced, on the html5 parse:
//! an assertion that holds is ported as it is, one that does not asserts that result with a
//! `deviation: D-1` comment (the slow path of `fast=False` is not ported), or `D-2` where the html5
//! and libxml2 trees differ. Every explicit `fast=False` call has a D-1 comment, even when the
//! value is the same. Lines that need the XML output (Phase 6) or metadata (Phase 4) are in
//! `#[ignore = "phase N"]` tests named `<test>_phase_N_lines`, over stubs that give the neutral
//! value.
//!
//! Not ported, and why:
//! - `test_deprecations` (Python-only parameters), `test_incompatible_options` (a `Focus` makes the
//!   combination unrepresentable, and the crate does not log), `test_settings_element_lists`
//!   (mutates `MANUALLY_CLEANED` in place; the crate's lists are constants, D-8).
//! - The slow path (D-1, F-6): `test_sanitize_tree_*`, `test_readability_*`, `test_external`,
//!   `test_is_probably_readerable`, `test_prefer_readability_rejects_raw_json`,
//!   `test_compare_extraction_justext_*`, `test_recall_escalation_justext`.
//! - Metadata and `Document` (Phase 4): `test_extract_with_metadata`, `test_python_output`,
//!   `test_url_blacklist`, `test_tojson`, `test_xmltocsv`, `test_extractor_parity`,
//!   `test_reset_caches`, `test_document_isolation`, `test_extraction_options`, and the cache lines
//!   of `test_trim`; language detection (OI-4): `test_wrong_language_discarded`,
//!   `test_lang_detection`.
//! - The XML and TEI output (Phase 6), whole tests: `test_yoast_faq_block`,
//!   `test_linebreak_before_inline_kept_in_xml`, `test_nested_element_tail_kept`,
//!   `test_extract_div_keeps_inline_link`, `test_lb_only_subtree_stays_one_paragraph`,
//!   `test_table_processing`, `test_xml_table_preserves_empty_cells`,
//!   `test_table_cell_escaping_stays_out_of_xml`, `test_short_document_fallback_preserves_image`,
//!   `test_longer_fallback_can_replace_requested_image`,
//!   `test_recall_escalation_captured_comment_list`, `test_list_item_attr_whitelist`,
//!   `test_list_processing`, `test_nested_list_first_item_not_duplicated`, `test_code_blocks`,
//!   `test_table_nested_in_cell_pipeline`, `test_check_tei_loose_text`.
//! - Single lines: `load_html(123)`, `extract(None, ...)` and the `output_format="xyz"` raises (the
//!   input and the format are typed), `core.Extractor(source=)`, the YAML parse of
//!   `test_markdown_metadata_yaml_safe`, the `prune_xpath` line of
//!   `test_extract_leaves_caller_tree_intact` (D-7).

use std::collections::HashSet;

use trafilatura_rust::__internals::{
    BODY_XPATH, Config, DEDUPE_SCAN_CAP, DumpMode, Extractor, Focus, Format, HtmlInput, LiveIter,
    MANUALLY_CLEANED, MANUALLY_STRIPPED, MAX_FILE_SIZE, MIN_DUPLICATE_LENGTH, NodeId,
    OVERALL_DISCARD_XPATH, PRECISION_DISCARD_XPATH, ParseLimits, Selector, Source, TAG_CATALOG,
    Tree, alt, baseline, baseline_tree, convert_tags, convert_to_html, delete_by_link_density,
    delete_element, dump, extract_content, handle_formatting, handle_image, handle_lists,
    handle_paragraphs, handle_quotes, handle_table, handle_textelem, handle_textnode,
    handle_titles, html2txt_tree, internal_extraction, link_density_test, link_density_test_tables,
    load_html, md_link, merge_with_parent, normalize_unicode, parse_document, parse_fragment,
    process_node, prune_html, prune_unwanted_nodes, prune_unwanted_sections, recover_wild_text,
    remove_empty_elements, replace_element_text, sanitize, settings_format, span,
    strip_double_tags, textfilter, tree_cleaning, tree_cleaning_with, trim, xml_to_text,
    yaml_scalar,
};
use trafilatura_rust::{Call, ExtractError, Format as ApiFormat, Input, Options};

#[allow(dead_code)]
mod common;
#[allow(dead_code)]
mod trees;
use trees::{ctx_for, lxml_fromstring, xml_tree};

/// A `<p>` element in a tree of its own, with the given text.
fn element_with_text(text: &str) -> (Tree, NodeId) {
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

// Reading a cfg file is upstream API (`use_config`, `settingsfile=`) that this crate does not offer,
// so the file-based assertions are not ported; `NEW_CONFIG` is built from the values of
// `newsettings.cfg`. The `extract_with_metadata` line is `test_config_loading_phase_4_lines`.
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

/// Upstream's `NEW_CONFIG`: the values of `tests/resources/newsettings.cfg`; the keys the crate has
/// no field for (downloads, timeouts, user agents) are left out.
fn new_config() -> Config {
    Config {
        min_extracted_size: 10000,
        min_output_size: 10000,
        min_output_comm_size: 10000,
        min_duplcheck_size: 10,
        max_repetitions: 3,
        max_file_size: 20_000_000,
        min_file_size: 10,
        max_tree_size: None,
        extensive_date_search: false,
    }
}

// The metadata line of `test_config_loading`.
#[test]
#[ignore = "phase 4"]
fn test_config_loading_phase_4_lines() {
    let html =
        "<html><body><article><p>Short paragraph of content here.</p></article></body></html>";
    assert!(extract_with_metadata(html, &Kw::new().config(new_config()), None).is_none());
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

/// Upstream's `ZERO_CONFIG`: `Config` with the minimum output and extraction sizes at 0.
fn zero_config() -> Config {
    Config {
        min_output_size: 0,
        min_extracted_size: 0,
        ..Config::default()
    }
}

/// The keyword arguments of upstream's `extract`, with its defaults. There is no `fast` field:
/// the crate has the fast mode only (D-1), so a `fast=True` argument is dropped and every
/// `fast=False` one is marked in the test.
#[derive(Clone)]
struct Kw {
    format: ApiFormat,
    focus: Focus,
    comments: bool,
    formatting: Option<bool>,
    links: bool,
    images: bool,
    tables: bool,
    url: Option<String>,
    config: Config,
}

impl Kw {
    /// `extract(html)`.
    fn new() -> Self {
        Self {
            format: ApiFormat::Txt,
            focus: Focus::Balanced,
            comments: true,
            formatting: None,
            links: false,
            images: false,
            tables: true,
            url: None,
            config: Config::default(),
        }
    }

    /// `extract(html, config=ZERO_CONFIG)`.
    fn zero() -> Self {
        Self::new().config(zero_config())
    }

    fn format(mut self, format: ApiFormat) -> Self {
        self.format = format;
        self
    }

    fn markdown(self) -> Self {
        self.format(ApiFormat::Markdown)
    }

    fn html(self) -> Self {
        self.format(ApiFormat::Html)
    }

    fn focus(mut self, focus: Focus) -> Self {
        self.focus = focus;
        self
    }

    fn comments(mut self, comments: bool) -> Self {
        self.comments = comments;
        self
    }

    fn formatting(mut self, formatting: bool) -> Self {
        self.formatting = Some(formatting);
        self
    }

    fn links(mut self, links: bool) -> Self {
        self.links = links;
        self
    }

    fn images(mut self, images: bool) -> Self {
        self.images = images;
        self
    }

    fn tables(mut self, tables: bool) -> Self {
        self.tables = tables;
        self
    }

    fn url(mut self, url: &str) -> Self {
        self.url = Some(url.to_string());
        self
    }

    fn config(mut self, config: Config) -> Self {
        self.config = config;
        self
    }

    /// The options of the public API.
    fn options(&self) -> Options {
        let options = Options::new()
            .format(self.format)
            .focus(self.focus)
            .comments(self.comments)
            .links(self.links)
            .images(self.images)
            .tables(self.tables)
            .config(self.config.clone());
        match self.formatting {
            Some(formatting) => options.formatting(formatting),
            None => options,
        }
    }

    /// The same settings as the crate's internal `Extractor`, for the tree entry point.
    fn settings(&self) -> Extractor {
        let mut settings = Extractor::new(
            self.config.clone(),
            settings_format(self.format),
            self.focus == Focus::Precision,
            self.focus == Focus::Recall,
            self.formatting,
            false,
            false,
            HashSet::new(),
            HashSet::new(),
        );
        settings.comments = self.comments;
        settings.links = self.links;
        settings.images = self.images;
        settings.tables = self.tables;
        settings.url.clone_from(&self.url);
        settings
    }
}

/// Upstream's `None` is `Err(Rejected(_))`; any other error is a failure of the test.
fn or_none<T>(result: Result<T, ExtractError>) -> Option<T> {
    match result {
        Ok(value) => Some(value),
        Err(ExtractError::Rejected(_)) => None,
        Err(error) => panic!("unexpected {error:?}"),
    }
}

/// `extract(html, **kw)` on a string.
fn extract(html: &str, kw: &Kw) -> Option<String> {
    let input = match &kw.url {
        Some(url) => Input::text(html).url(url),
        None => Input::text(html),
    };
    or_none(kw.options().build().extract(&input, &Call::default()))
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

/// `extract(html.fromstring(html), **kw)`: a caller's tree, as the html5 tree stands for lxml's.
fn extract_tree(html: &str, kw: &Kw) -> Option<String> {
    let (tree, root) = lxml_fromstring(html);
    let settings = kw.settings();
    let call = Call::default();
    let document = internal_extraction(
        Source::Tree(&tree, root),
        None,
        &ctx_for(&settings, &call),
        &ParseLimits::default(),
    );
    or_none(document).map(|document| document.text.unwrap_or_default())
}

// The stubs below stand for calls that need a later phase. They have upstream's shape and give the
// neutral value; the tests that use them are `#[ignore = "phase N"]`.

/// `extract(html, output_format="xml", **kw)`.
// todo(phase 6): xml output
fn extract_xml(_html: &str, _kw: &Kw) -> Option<String> {
    None
}

/// `extract(html.fromstring(html), output_format="xml", **kw)`.
// todo(phase 6): xml output
fn extract_xml_tree(_html: &str, _kw: &Kw) -> Option<String> {
    None
}

/// `extract(html, url, output_format="xmltei", **kw)`.
// todo(phase 6): xml output
fn extract_xmltei(_html: &str, _kw: &Kw) -> Option<String> {
    None
}

/// `extract(html.fromstring(html), output_format="xmltei", **kw)`.
// todo(phase 6): xml output
fn extract_xmltei_tree(_html: &str, _kw: &Kw) -> Option<String> {
    None
}

/// `extract(html.fromstring(html), output_format="xml", with_metadata=True, **kw)`.
// todo(phase 6): xml output, and phase 4: metadata
fn extract_xml_with_metadata_tree(_html: &str, _kw: &Kw) -> Option<String> {
    None
}

/// `xml.validate_tei(etree.fromstring(xml))`.
// todo(phase 6): TEI validation
fn validate_tei(_xml: &str) -> bool {
    false
}

/// `extract(html, url, record_id=..., output_format="xmltei")`.
// todo(phase 6): xml output
fn extract_xmltei_with_id(_html: &str, _url: &str, _record_id: &str) -> Option<String> {
    None
}

/// `extract(html, with_metadata=True, record_id=..., **kw)` in the format of `kw`.
// todo(phase 4): metadata
fn extract_with_metadata(_html: &str, _kw: &Kw, _record_id: Option<&str>) -> Option<String> {
    None
}

/// `extract(html, output_format="json", with_metadata=True, record_id=..., **kw)`.
// todo(phase 4): metadata
fn extract_json_with_metadata(_html: &str, _kw: &Kw, _record_id: Option<&str>) -> Option<String> {
    None
}

/// The introduction of upstream's `_extract_doc`.
const INTRO: &str = "enough intro text here for extraction";

/// `_extract_doc(body, intro=intro, **kw)`: the body in an article document. Upstream defaults the
/// config to `ZERO_CONFIG`; `kw` is `Kw::zero()` where it does.
fn extract_doc(body: &str, intro: bool, kw: &Kw) -> Option<String> {
    let inner = if intro {
        format!("<p>{INTRO}</p>{body}")
    } else {
        body.to_string()
    };
    extract(
        &format!("<html><body><article>{inner}</article></body></html>"),
        kw,
    )
}

/// `_table_md(table, **kw)`.
fn table_md(table: &str, kw: Kw) -> Option<String> {
    extract_doc(table, true, &kw.markdown().tables(true))
}

/// `_table_txt(table)`.
fn table_txt(table: &str) -> Option<String> {
    extract_doc(table, false, &Kw::zero().tables(true))
}

/// `_md_inline(body, **kw)`.
fn md_inline(body: &str, kw: Kw) -> String {
    extract_doc(body, true, &kw.markdown().formatting(true)).unwrap_or_default()
}

/// The file `name` of upstream's `tests/resources`, read from the checkout; `None` (with a note)
/// when the checkout is absent, as `parity.rs` does; with `PARITY_REQUIRE_FIXTURES` set an absent
/// checkout panics instead.
fn resource(name: &str) -> Option<Vec<u8>> {
    let Some(root) = common::upstream_root() else {
        assert!(
            std::env::var_os("PARITY_REQUIRE_FIXTURES").is_none_or(|v| v.is_empty()),
            "PARITY_REQUIRE_FIXTURES is set but the upstream checkout is missing, so {name} cannot be read"
        );
        eprintln!("unit_tests: no upstream checkout, skipping the lines that read {name}");
        return None;
    };
    Some(std::fs::read(root.join("tests/resources").join(name)).unwrap())
}

/// `resource(name)` as text.
fn resource_text(name: &str) -> Option<String> {
    resource(name).map(|bytes| String::from_utf8(bytes).unwrap())
}

/// A fresh `core.Extractor()`.
fn default_options() -> Extractor {
    extractor(Format::Python, false, false, None, (false, false), &[], &[])
}

/// The lines joined as the dump prints them, one `\n` after each.
fn dump_lines(lines: &[&str]) -> String {
    lines.iter().map(|line| format!("{line}\n")).collect()
}

/// `etree.fromstring(source)[0]`: the tree and the first child of its root.
fn first_child_of(source: &str) -> (Tree, NodeId) {
    let tree = xml_tree(source);
    let child = tree.children(tree.root()).next().unwrap();
    (tree, child)
}

#[test]
fn test_htmlprocessing() {
    // the dummy tree exists only because `xml_to_text` takes `&mut Tree` even for `None`
    let mut empty = xml_tree("<doc/>");
    assert_eq!(
        xml_to_text(&mut empty, None, false, &Call::default()).unwrap(),
        ""
    );
    let mut options = default_options();
    options.tables = true;
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let mut tree = xml_tree("<html/>");
    let root = tree.root();
    assert_eq!(tree_cleaning(&mut tree, root, &ctx).unwrap(), root);
    let mut tree = xml_tree("<unwanted/>");
    let root = tree.root();
    prune_html(&mut tree, root, Focus::Balanced, &call).unwrap();
    let mut convert_options = default_options();
    convert_options.tables = true;
    convert_options.formatting = true;
    convert_options.images = true;
    convert_options.links = true;
    // html5 foster-parents the `a` out of the table (D-2); the assertions hold on either tree
    let (mut mydoc, mydoc_root) = lxml_fromstring(
        r#"<html><body><table><a href="">Link</a></table><img src="test.jpg"/><u>Underlined</u><tt>True Type</tt><sub>Text</sub><sup>Text</sup></body></html>"#,
    );
    convert_tags(
        &mut mydoc,
        mydoc_root,
        &ctx_for(&convert_options, &call),
        None,
    )
    .unwrap();
    assert!(mydoc.find(mydoc_root, "ref").is_some());
    assert!(mydoc.find(mydoc_root, "graphic").is_some());
    assert!(
        mydoc
            .iter(mydoc_root, &["hi"])
            .any(|hi| mydoc.get(hi, "rend") == Some("#t"))
    );
    assert!(mydoc.find(mydoc_root, "table").is_some());

    // multiple images inside a link must keep their original order after being
    // lifted out of the <ref> (addnext reverses order if iterated forward)
    let (mut multi_img, multi_root) = lxml_fromstring(
        r#"<html><body><a href="/x"><img src="a.jpg"/><img src="b.jpg"/><img src="c.jpg"/></a></body></html>"#,
    );
    convert_tags(
        &mut multi_img,
        multi_root,
        &ctx_for(&convert_options, &call),
        None,
    )
    .unwrap();
    let srcs: Vec<_> = multi_img
        .iter(multi_root, &["graphic"])
        .map(|graphic| multi_img.get(graphic, "src").unwrap().to_string())
        .collect();
    assert_eq!(srcs, ["a.jpg", "b.jpg", "c.jpg"]);

    // the tree_cleaning of upstream's next lines runs on the converted `mydoc`
    convert_options.tables = false;
    let cleaned = tree_cleaning(&mut mydoc, mydoc_root, &ctx_for(&convert_options, &call)).unwrap();
    assert!(mydoc.find(cleaned, "graphic").is_some());
    assert!(mydoc.find(cleaned, "table").is_none());

    // delete_element on an element without a parent leaves it unchanged
    let mut tree = xml_tree("<test/>");
    let root = tree.root();
    let before = dump(&tree, DumpMode::Exact);
    delete_element(&mut tree, root, true);
    assert_eq!(dump(&tree, DumpMode::Exact), before);

    // merge with parent function
    let mut element = xml_tree("<test/>");
    let root = element.root();
    merge_with_parent(&mut element, root, false, &call).unwrap();
    assert_eq!(element.tostring(root, false, false), "<test/>");

    let merge_spans = |html: &str| {
        let (mut mydoc, mydoc_root) = lxml_fromstring(html);
        let mut spans = LiveIter::new(&mydoc, mydoc_root, &["span"]);
        while let Some(span) = spans.next(&mydoc) {
            merge_with_parent(&mut mydoc, span, false, &call).unwrap();
        }
        mydoc.tostring(mydoc_root, false, false)
    };
    assert!(
        merge_spans("<html><body><p><span>A</span><span>B</span><span>C</span></p></body></html>")
            .contains("<p>A B C</p>")
    );
    assert!(
        merge_spans(
            "<html><body><p><span>A</span><span>B</span> tail<span>C</span></p></body></html>"
        )
        .contains("<p>A B tail C</p>")
    );

    // paywalls
    let html = r#"<html><body><main><p>1</p><p id="premium">2</p><p>3</p></main></body></html>"#;
    assert_eq!(extract(html, &Kw::zero()).unwrap(), "1\n3");
    // deviation: D-1, upstream asserts the same with `fast=False`
    // fencedframe
    let html = "<html><body><article><p>real</p><fencedframe><p>ad</p></fencedframe><p>more</p></article></body></html>";
    assert!(!extract(html, &Kw::zero()).unwrap().contains("ad"));
    // deviation: D-1, upstream asserts the same with `fast=False`
    let html = "<html><body><fencedframe><article><h1>ad</h1><p>buy now buy now buy now buy now</p></article></fencedframe></body></html>";
    // deviation: D-1, upstream runs this one with `fast=False`
    assert!(!extract(html, &Kw::zero()).unwrap().contains("ad"));
    assert_eq!(extract(html, &Kw::new()), None);

    // test tail of node deleted if set as text
    let (mut tree, node) = first_child_of("<div><p></p>tail</div>");
    process_node(&mut tree, node, &ctx);
    assert_eq!(tree.node(node).text.as_deref(), Some("tail"));
    assert_eq!(tree.node(node).tail, None);
    let (mut tree, node) = first_child_of("<list><item></item>text in tail</list>");
    process_node(&mut tree, node, &ctx);
    assert_eq!(tree.node(node).text.as_deref(), Some("text in tail"));
    assert_eq!(tree.node(node).tail, None);
    let (mut tree, line_break) = first_child_of("<p><lb/>tail</p>");
    process_node(&mut tree, line_break, &ctx);
    assert_eq!(tree.node(line_break).text, None);
    assert_eq!(tree.node(line_break).tail.as_deref(), Some("tail"));
    let (mut tree, node) = first_child_of("<div><p>some text</p>tail</div>");
    process_node(&mut tree, node, &ctx);
    assert_eq!(tree.node(node).text.as_deref(), Some("some text"));
    assert_eq!(tree.node(node).tail.as_deref(), Some("tail"));
    let (mut tree, node) =
        first_child_of("<p><ref target='url'><hi rend='#b'>bold</hi>inner</ref>outer</p>");
    let processed = handle_textnode(&mut tree, node, &ctx, true, false).unwrap();
    assert_eq!(tree.node(processed).tail.as_deref(), Some("outer"));
    let (mut tree, node) = first_child_of("<p><ref target='url'>text</ref>tail</p>");
    let processed = handle_textnode(&mut tree, node, &ctx, true, false).unwrap();
    assert_eq!(tree.node(processed).tail.as_deref(), Some("tail"));
    assert_eq!(tree.node(processed).text.as_deref(), Some("text"));
    let (mut tree, node) = first_child_of("<p><ref target='url'></ref>tail</p>");
    let processed = handle_textnode(&mut tree, node, &ctx, true, false).unwrap();
    assert_eq!(tree.node(processed).tail.as_deref(), Some(""));
    assert_eq!(tree.node(processed).text.as_deref(), Some("tail"));
    let (mut tree, node) =
        first_child_of("<p><ref target='url'>text<hi rend='#b'>bold</hi></ref>tail</p>");
    let processed = handle_textnode(&mut tree, node, &ctx, true, false).unwrap();
    assert_eq!(tree.node(processed).tail.as_deref(), Some("tail"));
    assert_eq!(tree.node(processed).text.as_deref(), Some("text"));

    // fix for bug 807
    let (mut tree, node) = lxml_fromstring("<div><p><span>span</span> span tail</p> p tail </div>");
    assert_eq!(tree.text_content(node), "span span tail p tail ");
    prune_unwanted_nodes(&mut tree, node, &[spans], false, &call).unwrap();
    assert_eq!(tree.text_content(node), " span tail p tail ");

    // link_density_test_tables: a text-rich table whose only <ref> links carry no text
    // (e.g. icon/flag links wrapping images) has zero link text -> not boilerplate, kept
    let linkless_table = xml_tree(&format!(
        r#"<table><cell>{}<ref target="/x"></ref></cell></table>"#,
        "word ".repeat(50)
    ));
    assert!(!link_density_test_tables(&linkless_table, linkless_table.root(), &call).unwrap());
    // short table (elemlen < 200) is never removed regardless of link ratio
    let short_table = xml_tree(&format!(
        r#"<table><cell>short {}</cell></table>"#,
        r#"<ref target="/x">link</ref> "#.repeat(5)
    ));
    assert!(!link_density_test_tables(&short_table, short_table.root(), &call).unwrap());
    // replace_element_text: an empty <ref> (no text) yields an empty string
    let mut empty_ref = xml_tree("<ref/>");
    let root = empty_ref.root();
    assert_eq!(
        replace_element_text(&mut empty_ref, root, false, false, &call).unwrap(),
        ""
    );

    // regression #797: a code block with a tailless <lb> must not emit a literal "None"
    let mut code = xml_tree("<code>print(1)<lb/></code>");
    let root = code.root();
    let code_md = replace_element_text(&mut code, root, true, false, &call).unwrap();
    assert!(!code_md.contains("None"));
    assert!(code_md.contains("print(1)"));

    // handle_paragraphs: a trailing <lb> with no tail is stripped from the output
    let mut tree = xml_tree("<p>text<lb>x</lb></p>");
    let para = tree.root();
    let processed = handle_paragraphs(&mut tree, para, &["p", "lb"], &ctx)
        .unwrap()
        .unwrap();
    assert!(tree.find(processed, "lb").is_none());

    // handle_paragraphs: non-INLINE_CARRIED children of <hi> are stripped with a leading space
    let mut formatting = default_options();
    formatting.formatting = true;
    let ctx = ctx_for(&formatting, &call);
    let mut tree = xml_tree(r##"<p><hi rend="#b">pre<quote>mid</quote>end</hi></p>"##);
    let para = tree.root();
    let processed = handle_paragraphs(&mut tree, para, TAG_CATALOG, &ctx)
        .unwrap()
        .unwrap();
    let hi = tree
        .children(processed)
        .find(|&c| tree.node(c).name == *"hi")
        .unwrap();
    let text = tree.node(hi).text.as_deref().unwrap();
    assert!(text.contains("pre"));
    assert!(text.contains(" mid"));

    let mut tree = xml_tree(r##"<p><hi rend="#b">start<lb/>tail</hi></p>"##);
    let para = tree.root();
    let processed = handle_paragraphs(&mut tree, para, TAG_CATALOG, &ctx)
        .unwrap()
        .unwrap();
    let hi = tree
        .children(processed)
        .find(|&c| tree.node(c).name == *"hi")
        .unwrap();
    assert!(tree.node(hi).text.as_deref().unwrap().contains(" tail"));
}

// The XML lines of `test_htmlprocessing`.
#[test]
#[ignore = "phase 6"]
fn test_htmlprocessing_phase_6_lines() {
    let html = "<html><body><article><h1>Test headline</h1><p>Test</p></article></body></html>";
    assert!(
        extract_xml_tree(html, &Kw::zero())
            .unwrap()
            .contains(r#"<head rend="h1">Test headline</head>"#)
    );
    assert!(
        extract_xmltei_tree(html, &Kw::zero())
            .unwrap()
            .contains(r#"<ab rend="h1" type="header">Test headline</ab>"#)
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
fn test_aria_layout_table_reclassified() {
    // Any table with role=presentation/none is reclassified as div.
    let call = Call::default();
    let mut options = default_options();
    options.tables = true;
    for role in ["presentation", "none"] {
        let clean = |html: &str| {
            let (mut tree, root) = lxml_fromstring(html);
            tree_cleaning(&mut tree, root, &ctx_for(&options, &call)).unwrap();
            (tree, root)
        };
        // with nested table: outer reclassified, inner kept
        let (tree, root) = clean(&format!(
            "<html><body><table role=\"{role}\"><tr><td><table><tr><td>data</td></tr></table></td></tr></table></body></html>"
        ));
        assert!(
            tree.iterdescendants(root, &["table"])
                .all(|table| tree.get(table, "role").is_none())
        );
        assert!(tree.find(root, "table").is_some());
        // without nested table: also reclassified
        let (tree, root) = clean(&format!(
            "<html><body><table role=\"{role}\"><tr><td>text</td></tr></table></body></html>"
        ));
        assert!(tree.find(root, "table").is_none());
    }
}

#[test]
fn test_wrapper_form_kept() {
    // A form holding most of the text is a layout wrapper and must survive; genuine widget forms
    // must still be removed.
    let article = format!(
        "<p>{}</p>",
        "This is the actual article body with enough text to be extracted. ".repeat(4)
    );
    let call = Call::default();
    let options = default_options();
    let load = |html: &str| {
        let tree = load_html(
            HtmlInput::Str(html),
            MAX_FILE_SIZE,
            &ParseLimits::default(),
            &call,
        )
        .unwrap();
        let root = tree.root();
        (tree, root)
    };

    // the wrapper form survives cleaning, so the content is still there to be selected
    let wrapped = format!(
        r#"<html><body><form method="post" id="aspnetForm"><div>{article}</div></form></body></html>"#
    );
    let (mut tree, root) = load(&wrapped);
    let cleaned = tree_cleaning(&mut tree, root, &ctx_for(&options, &call)).unwrap();
    assert!(
        tree.text_content(cleaned)
            .contains("the actual article body")
    );
    assert!(tree.find(cleaned, "form").is_none()); // demoted, not left as a form
    assert!(
        extract(&wrapped, &Kw::zero())
            .unwrap()
            .contains("the actual article body")
    );

    // a search/newsletter widget alongside real content is still removed outright
    let with_widget = format!(
        r#"<html><body><div>{article}</div><form><p>Search this site</p><input type="text"/></form></body></html>"#
    );
    let (mut tree, root) = load(&with_widget);
    let cleaned = tree_cleaning(&mut tree, root, &ctx_for(&options, &call)).unwrap();
    let text = tree.text_content(cleaned);
    assert!(text.contains("the actual article body"));
    assert!(!text.contains("Search this site"));

    // MANUALLY_CLEANED without "form" keeps widget forms instead of failing on the internal removal
    let without_form: Vec<&str> = MANUALLY_CLEANED
        .iter()
        .copied()
        .filter(|tag| *tag != "form")
        .collect();
    let (mut tree, root) = load(&with_widget);
    let ctx = ctx_for(&options, &call);
    let cleaned =
        tree_cleaning_with(&mut tree, root, &ctx, &without_form, MANUALLY_STRIPPED).unwrap();
    assert!(tree.text_content(cleaned).contains("Search this site"));
    // the constant itself is never mutated
    assert!(MANUALLY_CLEANED.contains(&"form"));
}

#[test]
fn test_math_recovery() {
    // D-2: upstream parses with libxml2, where `math` has no namespace and is recovered. On the
    // html5 tree `math` carries the MathML namespace, so neither `recover_math` nor the
    // `MANUALLY_CLEANED` loop sees it: nothing is recovered and nothing is dropped. The expected
    // dumps are the html5 results of upstream's own functions; `recover_math` itself is tested in
    // `html_processing::tests::recover_math_on_html_namespace_math`.
    // oracle: dump_tree(tree_cleaning(parse_document(html), options)) with options.tables = True
    let call = Call::default();
    let mut options = default_options();
    options.tables = true;
    let clean = |html: &str| {
        let (mut tree, root) = lxml_fromstring(html);
        tree_cleaning(&mut tree, root, &ctx_for(&options, &call)).unwrap();
        dump(&tree, DumpMode::Exact).0
    };

    // the annotation holds the original LaTeX and takes precedence over alttext
    assert_eq!(
        clean(
            r#"<html><body><p>a <math alttext="rendered"><semantics><annotation encoding="application/x-tex">x^2</annotation></semantics></math> b</p></body></html>"#
        ),
        dump_lines(&[
            "html",
            "  body",
            "    p",
            r#"      #text "a ""#,
            r#"      math@math alttext="rendered""#,
            "        semantics@math",
            r#"          annotation@math encoding="application/x-tex""#,
            r#"            #text "x^2""#,
            r#"      #text " b""#,
        ])
    );
    // alttext is used when there is no LaTeX annotation
    assert_eq!(
        clean(r#"<html><body><p>a <math alttext="y_1"><mi>y</mi></math> b</p></body></html>"#),
        dump_lines(&[
            "html",
            "  body",
            "    p",
            r#"      #text "a ""#,
            r#"      math@math alttext="y_1""#,
            "        mi@math",
            r#"          #text "y""#,
            r#"      #text " b""#,
        ])
    );
    // display="block" yields block delimiters
    assert_eq!(
        clean(
            r#"<html><body><p><math display="block" alttext="z"><mi>z</mi></math></p></body></html>"#
        ),
        dump_lines(&[
            "html",
            "  body",
            "    p",
            r#"      math@math alttext="z" display="block""#,
            "        mi@math",
            r#"          #text "z""#,
        ])
    );
    // nothing to recover: the element stays too, as nothing sees it
    assert_eq!(
        clean("<html><body><p>a <math><mi>q</mi></math> b</p></body></html>"),
        dump_lines(&[
            "html",
            "  body",
            "    p",
            r#"      #text "a ""#,
            "      math@math",
            "        mi@math",
            r#"          #text "q""#,
            r#"      #text " b""#,
        ])
    );
    // the tail of the formula
    assert_eq!(
        clean(r#"<html><body><p><math alttext="u"><mi>u</mi></math> tail</p></body></html>"#),
        dump_lines(&[
            "html",
            "  body",
            "    p",
            r#"      math@math alttext="u""#,
            "        mi@math",
            r#"          #text "u""#,
            r#"      #text " tail""#,
        ])
    );
    // a formula in a table cell
    assert_eq!(
        clean(
            r#"<html><body><table><tr><td>name</td><td><math alttext="1.33\times 10^{-4}"><mi>v</mi></math></td></tr></table></body></html>"#
        ),
        dump_lines(&[
            "html",
            "  body",
            "    table",
            "      tr",
            "        td",
            r#"          #text "name""#,
            "        td",
            r#"          math@math alttext="1.33\\times 10^{-4}""#,
            "            mi@math",
            r#"              #text "v""#,
        ])
    );

    // end to end: Markdown output renders the recovered formula as $...$
    // deviation: D-2, upstream asserts "$1.33\times 10^{-4}$" in the result; on the html5 tree the
    // `math` element carries the MathML namespace, so nothing is recovered and the annotation text
    // stays as it is. oracle: extract(doc, output_format="markdown", config=ZERO_CONFIG) inside
    // html5_parsers(), with fast=True
    let doc = r#"<html><body><article><p>Some introductory prose that is long enough to be retained here.</p><p>The error was <math alttext="1.33\times 10^{-4}"><semantics><annotation encoding="application/x-tex">1.33\times 10^{-4}</annotation></semantics></math> overall.</p></article></body></html>"#;
    assert_eq!(
        extract(doc, &Kw::zero().markdown()).as_deref(),
        Some(
            "Some introductory prose that is long enough to be retained here.\n\nThe error was 1.33\\times 10^{-4} overall."
        )
    );
}

/// `.//div[@class='widget']`
fn widget_divs(
    tree: &Tree,
    context: NodeId,
    _call: &Call<'_>,
) -> Result<Vec<NodeId>, ExtractError> {
    Ok(tree
        .iterdescendants(context, &["div"])
        .filter(|&div| tree.get(div, "class") == Some("widget"))
        .collect())
}

/// `.//span`
fn spans(tree: &Tree, context: NodeId, _call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    Ok(tree.iterdescendants(context, &["span"]).collect())
}

#[test]
fn test_prune_unwanted_nodes_guard() {
    // An over-pruning trip leaves the same subtree in place; upstream's `result is node` is that
    // the div is still a child of body.
    let call = Call::default();
    for (widget_words, pruned) in [(300, false), (3, true)] {
        let (mut tree, doc) = lxml_fromstring(&format!(
            "<html><body><div><p>real text</p><div class='widget'>{}</div></div></body></html>",
            "x ".repeat(widget_words)
        ));
        let node = tree.find(doc, "div").unwrap();
        let before = tree.text_content(doc);
        prune_unwanted_nodes(&mut tree, node, &[widget_divs], true, &call).unwrap();
        assert_eq!(
            tree.node(node).parent(),
            Some(tree.find(doc, "body").unwrap())
        );
        assert_eq!(tree.text_content(doc) != before, pruned);
    }
}

#[test]
fn test_link_density_tables_threshold() {
    let call = Call::default();
    let med_ref = format!(r#"<ref target="/x">{}</ref>"#, "x".repeat(250));
    let big_ref = format!(r#"<ref target="/x">{}</ref>"#, "x".repeat(600));
    // 60% link density at ~600 chars: kept by the 0.8 (medium) threshold, removed by the 0.5 (large)
    let straddle_ref = format!(r#"<ref target="/x">{}</ref>"#, "x".repeat(360));
    let rows = [
        // medium table (<1000 chars): 80% link threshold
        (
            format!("<table><cell>{}{med_ref}</cell></table>", "y".repeat(50)),
            true,
        ), // 83% links
        (
            format!("<table><cell>{}{med_ref}</cell></table>", "y".repeat(200)),
            false,
        ), // 56% links
        // straddle table (500-999 chars): kept by the 0.8 threshold, would go at 0.5 (guards the
        // 1000-char boundary)
        (
            format!(
                "<table><cell>{}{straddle_ref}</cell></table>",
                "y".repeat(240)
            ),
            false,
        ),
        // large table (>=1000 chars): 50% link threshold
        (
            format!("<table><cell>{}{big_ref}</cell></table>", "y".repeat(400)),
            true,
        ), // 60% links
        (
            format!("<table><cell>{}{big_ref}</cell></table>", "y".repeat(600)),
            false,
        ), // 40% links
    ];
    for (html, expected) in rows {
        let tree = xml_tree(&html);
        assert_eq!(
            link_density_test_tables(&tree, tree.root(), &call).unwrap(),
            expected,
            "{html}"
        );
    }
}

#[test]
fn test_link_density_tables_textless_links_kept() {
    // 250 chars of real text and one image-wrapping link with no text: linklen 0, must be kept
    let call = Call::default();
    let tree = xml_tree(&format!(
        r#"<table><cell>{}<ref target="/x"><graphic src="/i.png"/></ref></cell></table>"#,
        "data ".repeat(50)
    ));
    assert!(!link_density_test_tables(&tree, tree.root(), &call).unwrap());
}

#[test]
fn test_link_density_test() {
    // Link-density verdict on a div followed by a content sibling (#584, #900).
    let call = Call::default();
    let headlines: String = (0..20)
        .map(|i| {
            format!(
                r#"<ref target="/n{i}">Latest news headline number {i} about some topic today</ref> "#
            )
        })
        .collect();
    let listing: String = (0..14)
        .map(|i| format!(r#"<p><ref target="/d{i}.pdf">Instruktionsbok MC 258 part {i}</ref></p>"#))
        .collect();
    let card = "Align: a widget that aligns its child within itself and optionally sizes itself based on the child's given size";
    let rows = [
        // 100 to 150 chars of recommendation links (mixed.de): the length limit stays at 100
        (
            (0..3)
                .map(|i| {
                    format!(
                        r#"<ref target="/p{i}">Recommended product number {i}: a nice gadget</ref> "#
                    )
                })
                .collect::<String>(),
            true,
            false,
            "short-link-list-kept",
        ),
        // a big "latest news" sidebar, above limitlen
        (headlines.clone(), false, true, "584-large-link-farm-pruned"),
        // one long link per catalog card (avg link text >= 100 chars) is content
        (
            (0..8)
                .map(|i| format!(r#"<ref target="/w{i}">{card}</ref> "#))
                .collect(),
            false,
            false,
            "584-whole-card-links-kept",
        ),
        // a document listing, one short link per paragraph
        (listing.clone(), false, false, "900-paragraph-listing-kept"),
        // a farm wrapped in a single paragraph buys no exemption
        (
            format!("<p>{headlines}</p>"),
            false,
            true,
            "900-links-sharing-a-paragraph-pruned",
        ),
        // the exemption is all-or-nothing: one paragraph with two links makes it a farm again
        (
            format!(
                r#"{listing}<p><ref target="/a">Also see this page</ref> <ref target="/b">and this other one</ref></p>"#
            ),
            false,
            true,
            "900-listing-with-one-shared-paragraph-pruned",
        ),
    ];
    for (items, short, expected, id) in rows {
        let (tree, doc) = lxml_fromstring(&format!(
            "<body><div>{items}</div><p>real article sibling here</p></body>"
        ));
        let element = tree.first_child(doc).unwrap();
        let length = trim(&tree.text_content(element)).chars().count();
        if short {
            assert!(100 < length && length < 150, "{id}: {length}");
        } else {
            assert!(length > 300, "{id}: {length}");
        }
        assert_eq!(
            link_density_test(&tree, element, false, &call).unwrap().0,
            expected,
            "{id}"
        );
    }
}

#[test]
fn test_delete_by_link_density_backtracking() {
    // a short div with 3+ children and some link text is not dense, but backtracking still drops it
    let call = Call::default();
    let html = "<html><body><div><p>Some intro text</p><p>More words</p><ref>A link text</ref></div></body></html>";
    for (backtracking, kept) in [(false, true), (true, false)] {
        let (mut tree, root) = lxml_fromstring(html);
        delete_by_link_density(&mut tree, root, "div", backtracking, false, &call).unwrap();
        assert_eq!(
            tree.find(root, "div").is_some(),
            kept,
            "backtracking {backtracking}"
        );
    }
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
#[ignore = "phase 6"]
fn test_formatting_phase_6_lines() {
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

// The metadata lines of `test_formatting`.
#[test]
#[ignore = "phase 4"]
fn test_formatting_phase_4_lines() {
    let meta_string = "<html><head><title>Test</title></head><body><p>ABC.</p></body></html>";
    let meta_result = extract_with_metadata(meta_string, &Kw::zero().markdown(), None).unwrap();
    assert!(meta_result.starts_with("---\ntitle: Test\nfingerprint: "));
    assert!(meta_result.ends_with("---\nABC."));
    // the record ID and fingerprint are in the header, as in the other formats
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

/// `handle_image(html.fromstring(html))`: the tree and the new element, `None` for no image.
fn image_of(html: &str) -> Option<(Tree, NodeId)> {
    let (mut tree, img) = lxml_fromstring(html);
    let image = handle_image(&mut tree, img, None).unwrap()?;
    Some((tree, image))
}

#[test]
fn test_images() {
    // `is_image_file` is ported with `utils`; the XML line is `test_images_phase_6_lines`
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
#[ignore = "phase 6"]
fn test_images_phase_6_lines() {
    if let Some(page) = resource_text("http_sample.html") {
        assert!(
            extract_xml(&page, &Kw::zero().images(true))
                .unwrap()
                .contains(r#"<graphic src="test.jpg" title="Example image"/>"#)
        );
    }
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
fn test_srcless_graphic_dropped_text_kept() {
    // A src-less image is dropped from a quote, the text around it in a paragraph is kept.
    let options = default_options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let mut tree = xml_tree("<quote>a<graphic/></quote>");
    let root = tree.root();
    let quote = handle_quotes(&mut tree, root, &ctx).unwrap().unwrap();
    assert_eq!(tree.tostring(quote, false, true), "<quote>a</quote>");
    let mut tree = xml_tree("<p>a<graphic/>b</p>");
    let root = tree.root();
    let tags: Vec<&str> = TAG_CATALOG.iter().copied().chain(["graphic"]).collect();
    let para = handle_paragraphs(&mut tree, root, &tags, &ctx)
        .unwrap()
        .unwrap();
    assert_eq!(tree.itertext(para).collect::<String>(), "ab");
}

#[test]
fn test_handle_lists() {
    // handle_lists output structure on small and malformed lists.
    let options = default_options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let cases = [
        // malformed list (common error): loose text becomes an item
        (
            "Description:<item>a</item><item>b</item>",
            "<item>Description:</item><item>a</item><item>b</item>",
        ),
        (
            "header<item>text</item>",
            "<item>header</item><item>text</item>",
        ),
        ("   <item>text</item>", "<item>text</item>"),
        ("<item><p>text</p></item>", "<item><p>text</p></item>"),
        (
            "<item>text1<p>text2</p></item>",
            "<item>text1<p>text2</p></item>",
        ),
        (
            "<item>text<lb/>more text</item>",
            "<item>text<lb/>more text</item>",
        ),
        // item tails join the item, after its last child
        ("<item>text</item>tail", "<item>text tail</item>"),
        (
            "<item><p>text</p></item>tail",
            "<item><p>text</p>tail</item>",
        ),
        (
            "<item><p>text</p>tail1</item>tail",
            "<item><p>text</p>tail1 tail</item>",
        ),
        (
            "<item><p>text</p>\n</item>tail",
            "<item><p>text</p>tail</item>",
        ),
        (
            "<item><list><item>text</item></list></item>tail",
            "<item><list><item>text</item></list>tail</item>",
        ),
        // the first item of a nested list does not come back as an item of the parent
        (
            "<item>a<list><item>b<list><item>c</item></list></item></list></item>",
            "<item>a<list><item>b<list><item>c</item></list></item></list></item>",
        ),
    ];
    for (items, expected) in cases {
        let mut tree = xml_tree(&format!("<list>{items}</list>"));
        let root = tree.root();
        let list = handle_lists(&mut tree, root, &ctx).unwrap().unwrap();
        assert_eq!(
            tree.tostring(list, false, true),
            format!("<list>{expected}</list>"),
            "{items}"
        );
    }
}

/// `handle_table(html.fromstring(table), TAG_CATALOG, Extractor(recall=recall))` as `tostring`, `None`
/// where there is no table.
fn handle_table_of(tree: &mut Tree, table: NodeId, recall: bool) -> Option<String> {
    let options = extractor(
        Format::Python,
        false,
        recall,
        None,
        (false, false),
        &[],
        &[],
    );
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let processed = handle_table(tree, table, TAG_CATALOG, &ctx).unwrap()?;
    Some(tree.tostring(processed, false, true))
}

#[test]
fn test_handle_table() {
    // handle_table output structure on small and broken tables.
    let cases = [
        // comments in a table (the html5 tree keeps none)
        (
            "<table><!-- c1 --><tr><td>cell text<!-- c2 --></td></tr></table>",
            false,
            "<row><cell>cell text</cell></row>",
        ),
        (
            "<table><tr><!-- ignored --><td>visible</td></tr></table>",
            false,
            "<row><cell>visible</cell></row>",
        ),
        (
            "<table><tr><td>cell1</td><td>cell2</td></tr><tr><td>cell3</td><td>cell4</td></tr></table>",
            false,
            "<row><cell>cell1</cell><cell>cell2</cell></row><row><cell>cell3</cell><cell>cell4</cell></row>",
        ),
        (
            "<table><tr><td><p>text</p><p>more text</p></td></tr></table>",
            false,
            "<row><cell><p>text</p><p>more text</p></cell></row>",
        ),
        (
            "<table><tr><td>text<lb/><p>more text</p></td></tr></table>",
            false,
            "<row><cell>text<p>more text</p></cell></row>",
        ),
        // the ref stays in the cell, a <p> wrapper detached it
        (
            "<table><tr><td><ref target='test'>link</ref></td></tr></table>",
            false,
            "<row><cell><ref target=\"test\">link</ref></cell></row>",
        ),
        (
            "<table><tr><th>Month</th><th>Days</th></tr><tr><td>January</td><td>31</td></tr></table>",
            false,
            "<row><cell role=\"head\">Month</cell><cell role=\"head\">Days</cell></row><row><cell>January</cell><cell>31</cell></row>",
        ),
        (
            "<table><tr><th>Name</th><th colspan='2'>Phone</th></tr><tr><td>J</td><td>p1</td><td>p2</td></tr></table>",
            false,
            "<row><cell role=\"head\">Name</cell><cell role=\"head\">Phone</cell><cell role=\"head\"/></row><row><cell>J</cell><cell>p1</cell><cell>p2</cell></row>",
        ),
        (
            "<table><tr><td><hi>highlighted text</hi></td></tr></table>",
            false,
            "<row><cell><hi>highlighted text</hi></cell></row>",
        ),
        // only the outer table is processed, the extraction walk re-extracts the inner one
        (
            "<table><tr><td>\n<table><tr><td>1</td></tr></table>\n</td></tr></table>",
            false,
            "<row><cell>\n</cell></row>",
        ),
        (
            "<table><tr><td>\n<table><tr><td>1</td></tr></table>\n</td><td>text1</td></tr><tr><td>text2</td></tr></table>",
            false,
            "<row><cell>\n</cell><cell>text1</cell></row><row><cell>text2</cell><cell/></row>",
        ),
        (
            "<table><tr><td>A</td></tr><tr><td><table><tr><td>inner</td></tr></table></td></tr><tr><td>AFTER</td></tr></table>",
            false,
            "<row><cell>A</cell></row><row><cell>AFTER</cell></row>",
        ),
        // the tail after a nested table is kept, after the cell text or its last child
        (
            "<table><tr><td>before<table><tr><td>i</td></tr></table>after</td></tr></table>",
            false,
            "<row><cell>before after</cell></row>",
        ),
        (
            "<table><tr><td><del>struck</del><table><tr><td>i</td></tr></table>after</td></tr></table>",
            false,
            "<row><cell><del>struck</del> after</cell></row>",
        ),
        // lists in cells are only kept in recall mode
        (
            "<table><tr><td><p>a list</p><list><item>one</item></list></td></tr></table>",
            false,
            "<row><cell><p>a list</p></cell></row>",
        ),
        (
            "<table><tr><td><p>a list</p><list><item>one</item></list></td></tr></table>",
            true,
            "<row><cell><p>a list</p><list><item>one</item></list></cell></row>",
        ),
        // broken tables: orphan cells, no <tr> at all
        (
            "<table><td>cell1</td><tr><td>cell2</td></tr></table>",
            false,
            "<row><cell>cell1</cell></row><row><cell>cell2</cell></row>",
        ),
        (
            "<table><td>a</td><td>b</td></table>",
            false,
            "<row><cell>a</cell><cell>b</cell></row>",
        ),
        // colgroup/col are layout hints, a whitespace-only caption is skipped
        (
            "<table><colgroup><col style='width:50%'><col style='width:50%'></colgroup><tr><td>a</td><td>b</td></tr></table>",
            false,
            "<row><cell>a</cell><cell>b</cell></row>",
        ),
        (
            "<table><col span='2'><tr><td>x</td><td>y</td></tr></table>",
            false,
            "<row><cell>x</cell><cell>y</cell></row>",
        ),
        (
            "<table><col><td>orphan</td><tr><td>normal</td></tr></table>",
            false,
            "<row><cell>orphan</cell></row><row><cell>normal</cell></row>",
        ),
        (
            "<table><caption>  </caption><tr><td>x</td></tr></table>",
            false,
            "<row><cell>x</cell></row>",
        ),
    ];
    for (table, recall, expected) in cases {
        let (mut tree, element) = lxml_fromstring(table);
        assert_eq!(
            handle_table_of(&mut tree, element, recall).as_deref(),
            Some(format!("<table>{expected}</table>").as_str()),
            "{table}"
        );
    }
}

#[test]
fn test_handle_table_malformed_html5() {
    // D-2: the html5 parser repairs these three tables, so the tree is not the one libxml2 builds.
    // Each has an `xml_tree` twin of the libxml2 shape asserting upstream's value.
    // oracle: etree.tostring(handle_table(h5(s), TAG_CATALOG, Extractor())) with h5(s) the html5tree
    // parse of s read the way lxml.html.fromstring reads a fragment; None where that is no table
    let cases = [
        // a td directly in the table after a tr gets a row of its own
        (
            "<table><tr><td>a</td></tr><td>b</td><tr><td>c</td></tr></table>",
            Some("<row><cell>a</cell></row><row><cell>b</cell></row><row><cell>c</cell></row>"),
            "<row><cell>a</cell><cell>b</cell></row><row><cell>c</cell></row>",
        ),
        // a p in a tr is foster-parented out: lxml returns the body, which is no table
        (
            "<table><tr><p>text</p></tr><tr><td>cell</td></tr></table>",
            None,
            "<row><cell>cell</cell></row>",
        ),
        // a td in a div in a td closes the outer cell
        (
            "<table><tr><td><div><td>inner</td></div></td></tr></table>",
            Some("<row><cell/><cell>inner</cell></row>"),
            "<row><cell><cell>inner</cell></cell></row>",
        ),
    ];
    for (table, html5, upstream) in cases {
        let (mut tree, element) = lxml_fromstring(table);
        assert_eq!(
            handle_table_of(&mut tree, element, false),
            html5.map(|rows| format!("<table>{rows}</table>")),
            "{table}"
        );
        let mut tree = xml_tree(table);
        let root = tree.root();
        assert_eq!(
            handle_table_of(&mut tree, root, false),
            Some(format!("<table>{upstream}</table>")),
            "{table}"
        );
    }
}

#[test]
fn test_colspan_zero_trust() {
    // `span` must default to 1 for non-decimal values (isdigit() admits superscripts that int() rejects)
    let cases = [
        ("1", 1),
        ("3", 3),
        ("12", 12),
        ("\u{b2}", 1),
        ("1\u{b2}", 1),
        ("2x", 1),
        ("", 1),
        ("-2", 1),
    ];
    for (colspan, expected) in cases {
        let mut tree = xml_tree("<tr/>");
        let cell = tree.new_element("td");
        tree.set(cell, "colspan", colspan);
        assert_eq!(span(&tree, cell, "colspan"), Ok(expected), "{colspan:?}");
    }
    // D-2: the html5 parser drops a td outside a table, so the parsed node carries no attribute
    // oracle: _span(h5(f'<td colspan="{colspan}">x</td>'), "colspan") is 1 for "3" and "12"
    for colspan in ["3", "12"] {
        let (tree, cell) = lxml_fromstring(&format!("<td colspan=\"{colspan}\">x</td>"));
        assert_eq!(span(&tree, cell, "colspan"), Ok(1), "{colspan:?}");
    }
}

#[test]
fn test_nested_block_tail_kept() {
    // The tail of a nested list or paragraph is kept, whether the block itself is kept or dropped.
    let cases = [
        (
            "<quote>a<list><item/></list> b</quote>",
            "<quote>a b</quote>",
        ),
        ("<quote>a<p><hi/></p> b</quote>", "<quote>a b</quote>"),
        ("<quote>a<list><item/></list> </quote>", "<quote>a</quote>"),
        (
            "<quote>a<list><item/></list>b</quote>",
            "<quote>a b</quote>",
        ),
        (
            "<quote>a <list><item/></list>b</quote>",
            "<quote>a b</quote>",
        ),
        ("<quote>a<graphic/>b</quote>", "<quote>ab</quote>"),
        (
            "<table><tr><td>x<list><item/></list> b</td></tr></table>",
            "<table><row><cell>x b</cell></row></table>",
        ),
        (
            "<table><tr><td>x<list><item/></list> </td></tr></table>",
            "<table><row><cell>x</cell></row></table>",
        ),
        (
            "<table><tr><td>x<list><item>i</item></list> b</td></tr></table>",
            "<table><row><cell>x<list><item>i</item></list> b</cell></row></table>",
        ),
    ];
    let options = extractor(Format::Python, false, true, None, (false, false), &[], &[]);
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let tags: Vec<&str> = TAG_CATALOG.iter().copied().chain(["table"]).collect();
    for (block, expected) in cases {
        let mut tree = xml_tree(block);
        let element = tree.root();
        let result = if tree.tag(element) == Some("quote") {
            handle_quotes(&mut tree, element, &ctx)
        } else {
            handle_table(&mut tree, element, &tags, &ctx)
        };
        assert_eq!(
            tree.tostring(result.unwrap().unwrap(), false, true),
            expected,
            "{block}"
        );
    }
}

#[test]
fn test_exotic_tags() {
    // `check_tei` and the TEI output come with phase 6 (`test_exotic_tags_phase_6_lines`)
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
#[ignore = "phase 6"]
fn test_exotic_tags_phase_6_lines() {
    // `load_mock_page("http://exotic_tags", tei_output=True)`: the page is read from the checkout
    let Some(page) = resource_text("exotic_tags.html") else {
        return;
    };
    let result = extract_xmltei_with_id(&page, "http://exotic_tags", "0000").unwrap();
    assert!(result.contains("Teletype text"));
    assert!(result.contains("My new car is silver."));
    // `xml.check_tei` on `exotic_tags_tei.html` is `check_tei`, ported in Phase 6
}

// Not ported: `load_html(123)` and `extract(None, ...)` (the input is typed), `core.Extractor(source=)`
// and the `output_format="xyz"` raises (no such option; `Format` is an enum), `_RespLike` (no
// response objects). The XML line is `test_input_phase_6_lines`. The other lines of upstream's test
// that touch encoding and repair are `test_input_*` and `test_repair_*` in `src/utils/tests.rs`.
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
#[ignore = "phase 6"]
fn test_input_phase_6_lines() {
    // output format
    assert!(
        extract_xml("<html><body><p>ABC</p></body></html>", &Kw::new())
            .unwrap()
            .contains("ABC")
    );
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
#[ignore = "phase 6"]
fn test_links_phase_6_lines() {
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
    // test license link; the metadata of the `license` attribute is Phase 4 as well
    let html = r#"<html><body><p>Test text under <a rel="license" href="">CC BY-SA license</a>.</p></body></html>"#;
    assert!(
        extract_xml_with_metadata_tree(html, &Kw::zero().links(true))
            .unwrap()
            .contains(r#"license="CC BY-SA license""#)
    );
}

#[test]
fn test_recover_wild_text_default_tags() {
    // recover_wild_text with default tags in recall mode must not crash
    let options = extractor(Format::Python, false, true, None, (false, false), &[], &[]);
    let call = Call::default();
    let (mut tree, root) = lxml_fromstring(
        "<body><div>some wild text outside the main frame, long enough to be recovered</div></body>",
    );
    let body = tree.new_element("body");
    recover_wild_text(&mut tree, root, body, &ctx_for(&options, &call), None, None).unwrap();
    // oracle: etree.tostring(recover_wild_text(tree, etree.Element("body"), Extractor(recall=True)))
    // The frame the parse returns is the div itself, which `.//div` does not select.
    assert_eq!(tree.tostring(body, false, false), "<body/>");
}

/// The trimmed text of each element `recover_wild_text` leaves in a new body for three divs:
/// `filler`, `CONTAINER`, and a substring of `CONTAINER`.
fn recovered_texts(filler: &str) -> Vec<String> {
    const CONTAINER: &str = "The quick brown fox jumps over the lazy dog while the sun was setting slowly over the meadow today.";
    const SUBSTRING_DUP: &str =
        "quick brown fox jumps over the lazy dog while the sun was setting slowly";
    let options = extractor(Format::Python, false, true, None, (false, false), &[], &[]);
    let call = Call::default();
    let html = format!(
        "<html><body><div>{filler}</div><div>{CONTAINER}</div><div>{SUBSTRING_DUP}</div></body></html>"
    );
    let (mut tree, root) = lxml_fromstring(&html);
    let body = tree.new_element("body");
    recover_wild_text(&mut tree, root, body, &ctx_for(&options, &call), None, None).unwrap();
    let children: Vec<NodeId> = tree.children(body).collect();
    children
        .into_iter()
        .map(|el| trim(&tree.itertext(el).collect::<String>()))
        .collect()
}

#[test]
fn test_recover_wild_text_dedup_scan_cap() {
    // DEDUPE_SCAN_CAP gates the substring scan against `existing`: a duplicate longer than
    // MIN_DUPLICATE_LENGTH is dropped while `existing` is under the cap and kept once past it.
    // Upstream monkeypatches the cap; the port keeps the constant and sizes the filler against it.
    let substring_dup = "quick brown fox jumps over the lazy dog while the sun was setting slowly";
    assert!(substring_dup.chars().count() > MIN_DUPLICATE_LENGTH);
    let sentence =
        "Zebra quokka platypus wallaby echidna kookaburra numbat bilby quoll dingo marsupial. ";
    // oracle: [len(t) for t in run(filler)] is [339, 99] and substring_dup is not in it
    let texts = recovered_texts(&sentence.repeat(4));
    assert_eq!(
        texts.iter().map(|t| t.chars().count()).collect::<Vec<_>>(),
        [339, 99]
    );
    assert!(!texts.iter().any(|t| t == substring_dup));
    // oracle: the same run with the filler repeated 2600 times is [220999, 99, 72], dup kept
    let long_filler = sentence.repeat(2600);
    assert!(long_filler.chars().count() >= DEDUPE_SCAN_CAP + 4000);
    let texts = recovered_texts(&long_filler);
    assert_eq!(
        texts.iter().map(|t| t.chars().count()).collect::<Vec<_>>(),
        [220_999, 99, 72]
    );
    assert!(texts.iter().any(|t| t == substring_dup));
}

const BOILERPLATE_LINK: &str =
    r#"<ref target="/x">link text that is reasonably long over here</ref>"#;

/// The page of `test_prune_boilerplate_table_after_nested`.
fn boilerplate_page() -> String {
    let rows = format!("<row><cell>{BOILERPLATE_LINK}</cell></row>").repeat(6);
    let nested = "<row><cell><table><row><cell>x</cell></row></table></cell></row>";
    let real = "Real article content paragraph that should always survive the pruning pass intact here now.";
    format!(
        "<html><body><table>{rows}{nested}</table><table>{rows}</table><p>{real}</p></body></html>"
    )
}

#[test]
fn test_prune_boilerplate_table_after_nested() {
    // prune_unwanted_sections must not skip a boilerplate table that follows a deleted table
    // containing a nested one.
    let options = default_options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let page = boilerplate_page();
    // D-2: the html5 parser foster-parents `row`, `cell` and `ref` out of the tables, so three
    // empty tables remain where libxml2 leaves none.
    // oracle: len(prune_unwanted_sections(h5(page), {"table", "p"}, Extractor(tables=True))
    //   .findall(".//table")) is 3, and the p is kept
    let (mut tree, root) = lxml_fromstring(&page);
    prune_unwanted_sections(&mut tree, root, &["table", "p"], &ctx, false).unwrap();
    assert_eq!(tree.iter(root, &["table"]).count(), 3);
    assert!(tree.find(root, "p").is_some());
    // oracle: the same call on html.fromstring(page) leaves findall(".//table") == [] and the p
    let mut tree = xml_tree(&page);
    let root = tree.root();
    prune_unwanted_sections(&mut tree, root, &["table", "p"], &ctx, false).unwrap();
    assert_eq!(tree.iter(root, &["table"]).count(), 0);
    assert!(tree.find(root, "p").is_some());
}

#[test]
fn test_prune_strips_span_and_ref() {
    // spans are always flattened, refs only when links are not kept
    let options = default_options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let page = r#"<html><body><p>One <span>two</span> <ref target="/x">three</ref> four.</p></body></html>"#;
    let (mut tree, root) = lxml_fromstring(page);
    prune_unwanted_sections(&mut tree, root, &["p"], &ctx, false).unwrap();
    assert!(tree.find(root, "span").is_none());
    assert!(tree.find(root, "ref").is_none());
    let p = tree.find(root, "p").unwrap();
    assert_eq!(trim(&tree.text_content(p)), "One two three four.");
    let (mut tree, root) = lxml_fromstring(page);
    prune_unwanted_sections(&mut tree, root, &["p", "ref"], &ctx, false).unwrap();
    assert!(tree.find(root, "span").is_none());
    assert!(tree.find(root, "ref").is_some());
}

#[test]
fn test_prune_keep_teasers() {
    // keep_teasers spares teaser-class blocks, some of which wrap real article text
    let options = default_options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let page = concat!(
        r#"<html><body><div class="teaser"><p>Real article body text that is long enough "#,
        "to count as genuine content here now.</p></div></body></html>"
    );
    let (mut tree, root) = lxml_fromstring(page);
    prune_unwanted_sections(&mut tree, root, &["p"], &ctx, false).unwrap();
    assert!(tree.find(root, "p").is_none());
    let (mut tree, root) = lxml_fromstring(page);
    prune_unwanted_sections(&mut tree, root, &["p"], &ctx, true).unwrap();
    assert!(tree.find(root, "p").is_some());
}

#[test]
fn test_no_duplicate_content_list_item() {
    // a list item's paragraph is counted once in the result
    let options = default_options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let para = "This is a moderately long description paragraph exceeding the fifty character dedup threshold here.";
    let (mut tree, root) = lxml_fromstring(&format!(
        "<html><body><article><dl><dt>Term</dt><dd><p>{para}</p></dd></dl></article></body></html>"
    ));
    let root = tree_cleaning(&mut tree, root, &ctx).unwrap();
    convert_tags(&mut tree, root, &ctx, None).unwrap();
    let (_, text) = extract_content(&mut tree, root, &ctx).unwrap();
    assert_eq!(text.matches(para).count(), 1);
}

#[test]
fn test_markdown_link_angle_bracket_targets() {
    // A '<' or '>' in a link/image target must stay inside the angle-bracket destination.
    // upstream: tests/unit_tests.py::test_markdown_link_angle_bracket_targets
    let cases = [
        ("http://x.com/a>b", "[t](<http://x.com/a\\>b>)"),
        ("http://x.com/a<b", "[t](<http://x.com/a\\<b>)"),
        ("http://x.com/a<b>c", "[t](<http://x.com/a\\<b\\>c>)"),
        ("http://x.com/a b>c", "[t](<http://x.com/a b\\>c>)"),
        ("http://x.com/end>", "[t](<http://x.com/end\\>>)"),
        ("<start.example/p", "[t](<\\<start.example/p>)"),
        ("http://x.com/a\\>b", "[t](<http://x.com/a\\\\\\>b>)"),
        ("http://x.com/a>>b", "[t](<http://x.com/a\\>\\>b>)"),
        (">x.com/p", "[t](<\\>x.com/p>)"),
        ("http://x.com/<>", "[t](<http://x.com/\\<\\>>)"),
        ("http://x.com/a b/c", "[t](<http://x.com/a b/c>)"),
        ("http://x.com/a(b)", "[t](<http://x.com/a(b)>)"),
        ("http://x.com/a)b(c", "[t](<http://x.com/a)b(c>)"),
        ("http://x.com/a(b", "[t](<http://x.com/a(b>)"),
        ("http://x.com/plain", "[t](http://x.com/plain)"),
    ];
    for (target, expected) in cases {
        assert_eq!(md_link("t", Some(target), false), expected);
        assert_eq!(md_link("t", Some(target), true), format!("!{expected}"));
    }
    assert_eq!(md_link("t", None, false), "[t]");
    assert_eq!(md_link("t", None, true), "![t]");
    assert_eq!(
        md_link("a[b]c", Some("http://x.com/p>q"), false),
        "[a\\[b\\]c](<http://x.com/p\\>q>)"
    );

    // end-to-end through xmltotxt: '>' and '<' arrive as entities in the attribute
    assert_eq!(
        md(r#"<body><p><ref target="http://x.com/a&gt;b">t</ref></p></body>"#),
        "[t](<http://x.com/a\\>b>)"
    );
    assert_eq!(
        md(r#"<body><graphic src="http://x.com/a&lt;b" alt="A"/></body>"#),
        "![A](<http://x.com/a\\<b>)"
    );
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

// Upstream's other blocks call `extract`, `validate_tei` and `write_fullheader`: 3f and Phase 6.
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

// The `extract` and TEI lines of `test_tei`; `test_tei` itself holds the parts that need neither.
#[test]
#[ignore = "phase 6"]
fn test_tei_phase_6_lines() {
    // open local resources to avoid redownloading at each run
    if let Some(page) = resource_text("httpbin_sample.html") {
        let kw = Kw::new().url("mocked");
        // `tei_validation=True` is an option of the TEI output, so `result2` is not compared yet
        let result = extract_xmltei(&page, &kw).unwrap();
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
    // `xml.write_fullheader` is a Phase 6 function as well
    let kw = Kw::zero().url("mocked");
    let extracted = extract_xmltei_tree(
        "<html><head/><body><div><h2><p>text</p></h2></div></body></html>",
        &kw,
    )
    .unwrap();
    assert!(validate_tei(&extracted));
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

#[test]
fn test_heading_level_zero_trust() {
    // replace_element_text must never crash on a malformed head rend and defaults to level 2
    let call = Call::default();
    let cases = [
        (Some("h1"), "# T"),
        (Some("h2"), "## T"),
        (Some("h6"), "###### T"),
        // absent -> default level 2
        (None, "## T"),
        (Some(""), "## T"),
        // no digit
        (Some("h"), "## T"),
        // non-digit
        (Some("hx"), "## T"),
        // superscript: isdigit() True but int() raises -> must not crash
        (Some("h\u{b2}"), "## T"),
        // non-ASCII decimal digit -> not an accepted level
        (Some("h\u{664}"), "## T"),
        // out of range
        (Some("h0"), "## T"),
        (Some("h9"), "## T"),
    ];
    for (rend, expected) in cases {
        let mut el = xml_tree("<head/>");
        let root = el.root();
        el.node_mut(root).text = Some("T".to_owned());
        if let Some(rend) = rend {
            el.set(root, "rend", rend);
        }
        assert_eq!(
            replace_element_text(&mut el, root, true, false, &call).unwrap(),
            expected,
            "rend {rend:?}"
        );
    }
}

/// Python `str.strip`: Rust's whitespace plus `\x1c`-`\x1f`, which `str::trim` misses.
fn py_strip(text: &str) -> &str {
    text.trim_matches(|c: char| c.is_whitespace() || ('\x1c'..='\x1f').contains(&c))
}

/// `xmltotxt` of the XML snippet, without stripping.
fn xmltotxt(body: &str, include_formatting: bool) -> String {
    let mut tree = xml_tree(body);
    let root = tree.root();
    xml_to_text(&mut tree, Some(root), include_formatting, &Call::default()).unwrap()
}

/// Serialize an XML snippet to Markdown (or plain text), stripped.
// upstream: tests/unit_tests.py::_md
fn md_with(body: &str, include_formatting: bool) -> String {
    py_strip(&xmltotxt(body, include_formatting)).to_owned()
}

/// `md_with` in Markdown mode.
fn md(body: &str) -> String {
    md_with(body, true)
}

#[test]
fn test_serializer_tail_spacing() {
    // Item and cell tails keep source separators, in order, without stacked spaces.
    let cases = [
        ("<list><item><lb/>one</item></list>", "- one"),
        ("<list><item>\n  <lb/>one</item></list>", "- one"),
        ("<list><item>one<p><lb/>two</p></item></list>", "- one\ntwo"),
        ("<list><item>a<lb/>b<lb/>c</item></list>", "- a\nb\nc"),
        (
            "<list><item>\n  <hi rend='#b'>x</hi> y</item></list>",
            "- **x** y",
        ),
        (
            "<list><item><ref target='s'>s</ref>: who use <ref target='b'>b</ref></item></list>",
            "- [s](s): who use [b](b)",
        ),
        (
            "<list><item>use <hi rend='#b'>bots</hi>, to resell.</item></list>",
            "- use **bots**, to resell.",
        ),
        (
            "<table><row><cell><p>x<lb/>y</p>z</cell></row></table>",
            "| x y z |",
        ),
        (
            "<table><row><cell><list><item>a</item><item>b</item></list>z</cell></row></table>",
            "| a b z |",
        ),
        (
            "<table><row><cell>use <hi rend='#b'>bots</hi>, x</cell></row></table>",
            "| use **bots**, x |",
        ),
        (
            "<table><row><cell><hi rend='#b'>A</hi> <hi rend='#b'>B</hi>, x</cell></row></table>",
            "| **A B**, x |",
        ),
        (
            "<table><row><cell>a<ref target='u'/> b</cell></row></table>",
            "| a b |",
        ),
        (
            "<table><row><cell>x<p>y</p>z mid<p>w</p></cell></row></table>",
            "| x y z mid w |",
        ),
        ("<list><item>a<p>b</p>c<p>d</p></item></list>", "- a b c d"),
        ("<list><item><lb/></item><item>b</item></list>", "- b"),
        (
            r##"<p><hi rend="#b">a</hi><hi rend="#b">b</hi><hi rend="#b">c</hi> tail</p>"##,
            "**abc** tail",
        ),
    ];
    for (body, expected) in cases {
        let doc = format!("<doc>{body}</doc>");
        assert_eq!(md_with(&doc, true), expected, "{body}");
        assert_eq!(md_with(&doc, false), expected.replace("**", ""), "{body}");
    }
}

#[test]
fn test_serializer_nested_lb_in_item_keeps_break() {
    // A nested line break after item content keeps its newline.
    let text = xmltotxt(
        "<doc><list><item><p>para</p><p><lb/>second</p></item></list></doc>",
        true,
    );
    assert!(text.contains("\nsecond"));
}

#[test]
fn test_serializer_nested_list_tail_stays_in_parent_item() {
    // Text after a nested list belongs to the parent item, not to the last sub-item.
    let source = "<doc><list><item>L1<list><item>L2<list><item>L3</item></list> t2</item></list> t1</item><item>n</item></list></doc>";
    assert_eq!(
        xmltotxt(source, true),
        "- L1\n  - L2\n    - L3\n\n    t2\n\n  t1\n- n\n"
    );
    assert_eq!(
        xmltotxt(source, false),
        "- L1\n  - L2\n    - L3\nt2\nt1\n- n"
    );
}

#[test]
fn test_cell_context_follows_real_ancestry() {
    // Cell handling must follow real ancestry, not 'a cell exists somewhere' (#767).
    let source = "<body><table><row><cell><p>inside</p></cell></row></table><p>outside</p></body>";
    assert_eq!(xmltotxt(source, true), "| inside | \n\noutside\n");
    assert_eq!(xmltotxt(source, false), "| inside | \noutside");
}

#[test]
fn test_markdown_escaping() {
    // Markdown-mode output escapes metacharacters that would corrupt GFM structure.
    // pipe in table cell text must be escaped so it doesn't split the column
    assert!(
        md("<body><table><row><cell>a|b</cell><cell>c</cell></row></table></body>")
            .contains("a\\|b")
    );

    // pipe inside formatted text (hi) inside a cell must also be escaped
    assert!(
        md(r##"<body><table><row><cell><hi rend="#b">x|y</hi></cell></row></table></body>"##)
            .contains("x\\|y")
    );

    // URL with a space must be wrapped in angle brackets
    assert!(
        md(r#"<body><p><ref target="http://a b/c">link</ref></p></body>"#)
            .contains("[link](<http://a b/c>)")
    );

    // square brackets in link text must be escaped
    assert!(
        md(r#"<body><p><ref target="http://x">a[b]c</ref></p></body>"#)
            .contains("[a\\[b\\]c](http://x)")
    );

    // square brackets in image alt must be escaped
    assert!(
        md(r#"<body><graphic src="img.png" alt="a[b]c"/></body>"#)
            .contains("![a\\[b\\]c](img.png)")
    );

    // a ref with no/empty target renders as bare [text]; a graphic with no src keeps empty parens
    assert_eq!(md("<body><p><ref>txt</ref></p></body>"), "[txt]");
    assert_eq!(
        md(r#"<body><p><ref target="">txt</ref></p></body>"#),
        "[txt]"
    );
    assert_eq!(md(r#"<body><graphic alt="a"/></body>"#), "![a]()");

    // backtick in inline code needs a longer fence
    assert!(md(r##"<body><p><hi rend="#t">a`b</hi></p></body>"##).contains("``a`b``"));

    // inline code whose content abuts a backtick gets a space pad so the fence does not merge
    assert_eq!(
        md(r##"<body><p><hi rend="#t">`x</hi></p></body>"##),
        "`` `x ``"
    );
    assert_eq!(
        md(r##"<body><p><hi rend="#t">x`</hi></p></body>"##),
        "`` x` ``"
    );
    assert_eq!(
        md(r##"<body><p><hi rend="#t">`</hi></p></body>"##),
        "`` ` ``"
    );

    // triple backtick in block code needs a 4-backtick fence
    let result = md("<body><code>a```b</code></body>");
    assert!(result.contains("````"));
    assert!(result.contains("a```b"));

    // ~~ inside del content must not close the strikethrough early
    assert!(md("<body><p><del>a~~b</del></p></body>").contains("~~a\\~\\~b~~"));

    // del wrapping an inline child must preserve the strikethrough marker
    assert!(
        md(r##"<body><p><del><hi rend="#b">bold</hi></del></p></body>"##).contains("~~**bold**~~")
    );

    // pipe in graphic tail inside a table cell must be escaped
    assert!(
        md(r#"<body><table><row><cell><graphic src="img.png" alt="img"/>tail|text</cell></row></table></body>"#)
            .contains("tail\\|text")
    );
    // pipe in non-graphic element tail inside a cell must also be escaped (separate code path)
    assert!(
        md(r##"<body><table><row><cell><hi rend="#b">bold</hi>tail|pipe</cell></row></table></body>"##)
            .contains("tail\\|pipe")
    );

    // block math in a cell must not inject newlines that split the table row
    let result = md("<body><table><row><cell>x \\[E=mc^2\\] y</cell></row></table></body>");
    assert!(!result.contains('\n'));
    assert!(result.contains("$$ E=mc^2 $$"));

    // bold wrapping a link must preserve both the bold marker and the link
    assert!(
        md(r##"<body><p><hi rend="#b"><ref target="http://x.com">link</ref></hi></p></body>"##)
            .contains("**[link](http://x.com)**")
    );

    // a graphic wrapped in inline formatting must keep the image, not be dropped
    assert_eq!(
        md(r##"<body><p><hi rend="#b">x <graphic src="i.jpg" alt="A"/> y</hi></p></body>"##),
        "**x ![A](i.jpg) y**"
    );
    assert_eq!(
        md(r#"<body><p><del>x <graphic src="i.jpg" alt="A"/> y</del></p></body>"#),
        "~~x ![A](i.jpg) y~~"
    );

    // del as a direct child of a table cell (no enclosing p)
    let result = extract(
        "<html><body><table><tr><td><del>gone</del></td></tr></table></body>",
        &Kw::zero().markdown().formatting(true),
    )
    .unwrap();
    assert!(!result.is_empty());
    assert!(result.contains("~~gone~~"));
}

#[test]
fn test_markdown_asterisk_escaping() {
    // Literal '*' in source text must be escaped so CommonMark renders it literally, not as emphasis.

    // a bare asterisk in plain paragraph text must not turn into emphasis
    assert_eq!(
        md("<body><p>This *should* not be italic and 3*4=12</p></body>"),
        "This \\*should\\* not be italic and 3\\*4=12"
    );

    // a real <hi> bold marker must survive untouched next to escaped literal asterisks
    assert_eq!(
        md(r##"<body><p><hi rend="#b">bold</hi> and *literal* asterisks</p></body>"##),
        "**bold** and \\*literal\\* asterisks"
    );

    // asterisks inside a heading are escaped, the '#' prefix is untouched
    assert_eq!(
        md(r#"<body><head rend="h2">Title *with* asterisk</head></body>"#),
        "## Title \\*with\\* asterisk"
    );

    // asterisks inside a list item are escaped, the '- ' marker is untouched
    assert_eq!(
        md("<body><list><item>list item *text*</item></list></body>"),
        "- list item \\*text\\*"
    );

    // asterisks inside table cell text are escaped alongside the pre-existing pipe escaping
    assert!(md("<body><table><row><cell>a*b|c</cell></row></table></body>").contains("a\\*b\\|c"));

    // asterisks inside link text are escaped, the [text](url) syntax is untouched
    assert!(
        md(r#"<body><p><ref target="http://x.com">link *text*</ref></p></body>"#)
            .contains("[link \\*text\\*](http://x.com)")
    );

    // asterisks inside image alt text are escaped
    assert!(md(r#"<body><graphic src="i.png" alt="a*b"/></body>"#).contains("![a\\*b](i.png)"));

    // asterisks inside struck-through (del) text are escaped, the ~~ markers are untouched
    assert_eq!(md("<body><p><del>a*b</del></p></body>"), "~~a\\*b~~");

    // a bold link keeps both its ** markers and its brackets: nested markup must not be
    // re-escaped as if it were literal source text
    assert_eq!(
        md(
            r##"<body><p><ref target="http://x.com"><hi rend="#b">bold link</hi></ref></p></body>"##
        ),
        "[**bold link**](http://x.com)"
    );

    // asterisks inside verbatim code (block and inline) must NOT be escaped
    assert_eq!(md("<body><code>a * b</code></body>"), "`a * b`");
    assert_eq!(
        md(r##"<body><p><hi rend="#t">a*b</hi></p></body>"##),
        "`a*b`"
    );

    // a run of consecutive asterisks (e.g. a plain-text divider) must not be read as emphasis
    assert_eq!(
        md("<body><p>*** section break ***</p></body>"),
        "\\*\\*\\* section break \\*\\*\\*"
    );

    // an even-length run must also be escaped character-by-character, not skipped as a pair
    assert_eq!(
        md("<body><p>**not bold**</p></body>"),
        "\\*\\*not bold\\*\\*"
    );

    // a literal '*' touching a real <hi> open/close boundary (immediately inside or outside
    // the tag, singly or doubled) must be escaped without disturbing the real emphasis
    // marker. #i is the higher-risk case since its marker is a single '*', the same
    // character being escaped, while #b's '**' is a different-looking two-char sequence.
    for (rend, marker) in [("#i", "*"), ("#b", "**")] {
        // just outside the opening tag
        assert_eq!(
            md(&format!(r#"<body><p>*<hi rend="{rend}">x</hi></p></body>"#)),
            format!("\\*{marker}x{marker}")
        );
        // just inside the opening tag (first character of the hi's own text)
        assert_eq!(
            md(&format!(r#"<body><p><hi rend="{rend}">*x</hi></p></body>"#)),
            format!("{marker}\\*x{marker}")
        );
        // just outside the closing tag
        assert_eq!(
            md(&format!(r#"<body><p><hi rend="{rend}">x</hi>*</p></body>"#)),
            format!("{marker}x{marker}\\*")
        );
        // just inside the closing tag (last character of the hi's own text)
        assert_eq!(
            md(&format!(r#"<body><p><hi rend="{rend}">x*</hi></p></body>"#)),
            format!("{marker}x\\*{marker}")
        );
        // wrapping the text from inside the tag on both sides
        assert_eq!(
            md(&format!(
                r#"<body><p><hi rend="{rend}">*x*</hi></p></body>"#
            )),
            format!("{marker}\\*x\\*{marker}")
        );
        // wrapping the whole <hi> element from outside on both sides
        assert_eq!(
            md(&format!(
                r#"<body><p>*<hi rend="{rend}">x</hi>*</p></body>"#
            )),
            format!("\\*{marker}x{marker}\\*")
        );
        // a literal '**' run just inside the tag on both sides
        assert_eq!(
            md(&format!(
                r#"<body><p><hi rend="{rend}">**x**</hi></p></body>"#
            )),
            format!("{marker}\\*\\*x\\*\\*{marker}")
        );
        // a literal '**' run just outside the tag on both sides
        assert_eq!(
            md(&format!(
                r#"<body><p>**<hi rend="{rend}">x</hi>**</p></body>"#
            )),
            format!("\\*\\*{marker}x{marker}\\*\\*")
        );
    }

    // a pre-existing literal backslash immediately before an asterisk must round-trip:
    // rendered output should still show exactly one backslash then one asterisk
    assert_eq!(
        md("<body><p>Edge case: a\\*b</p></body>"),
        "Edge case: a\\\\\\*b"
    );

    // include_formatting=False is plain-text mode: asterisks must be left completely alone
    assert_eq!(
        md_with("<body><p>This *stays* as-is</p></body>", false),
        "This *stays* as-is"
    );
}

#[test]
fn test_markdown_special_char_escaping() {
    // Literal CommonMark metacharacters other than '*' must also be escaped so they render as
    // themselves: '_', '`', '[', ']', '<', '~' are ambiguous wherever they appear in prose, while
    // '#', '-', '+', '.'/')' after digits and '>' only matter at the very start of a fresh block
    // line (heading/list/blockquote syntax). Everything else (quotes, parens, punctuation) is inert
    // in body text and must be left alone.

    // underscore, backtick, brackets and '<' are escaped anywhere in a paragraph
    assert_eq!(
        md("<body><p>a_b_c and `code` and [bracket] and &lt;tag&gt; text</p></body>"),
        "a\\_b\\_c and \\`code\\` and \\[bracket\\] and \\<tag> text"
    );
    // a lone '~' is also escaped: GitHub renders a single tilde as strikethrough, not just '~~'
    assert_eq!(
        md("<body><p>~single~ tilde</p></body>"),
        "\\~single\\~ tilde"
    );

    // a leading '#' in a plain paragraph must not become an ATX heading
    assert_eq!(
        md("<body><p># not a heading</p></body>"),
        "\\# not a heading"
    );
    // a leading '>' in a plain paragraph must not become a blockquote
    assert_eq!(
        md("<body><p>&gt; not a quote</p></body>"),
        "\\> not a quote"
    );
    // a leading '-'/'+' in a plain paragraph must not become a bullet list
    assert_eq!(md("<body><p>- not a list</p></body>"), "\\- not a list");
    assert_eq!(md("<body><p>+ not a list</p></body>"), "\\+ not a list");
    // a leading digit + '.'/')' in a plain paragraph must not become an ordered list
    assert_eq!(md("<body><p>1. not a list</p></body>"), "1\\. not a list");
    assert_eq!(md("<body><p>1) not a list</p></body>"), "1\\) not a list");

    // the same markers are safe mid-line and must be left alone (only line-start is risky)
    assert_eq!(
        md("<body><p>mid-line - and 3.14 and 100% and word) stay put</p></body>"),
        "mid-line - and 3.14 and 100% and word) stay put"
    );

    // a list item's own content starting with a marker-like character must not nest as a
    // sub-list; the item's real '- '/'N. ' marker (added separately) is untouched
    assert_eq!(
        md("<body><list><item>- nested looking</item></list></body>"),
        "- \\- nested looking"
    );
    assert_eq!(
        md(r#"<body><list rend="ol"><item>2. nested ordered looking</item></list></body>"#),
        "1. 2\\. nested ordered looking"
    );

    // a blockquote element's own leading '>' must also be escaped
    assert_eq!(
        md("<body><quote>&gt; quoted text with - marker</quote></body>"),
        "\\> quoted text with - marker"
    );

    // a heading's own '#' prefix already claims the line, so an inner leading '#' is inert
    assert_eq!(
        md(r##"<body><head rend="h2"># nested hash in heading</head></body>"##),
        "## # nested hash in heading"
    );

    // table cells are inline-only in GFM: a leading marker-like character there is never risky
    assert!(
        md("<body><table><row><cell>- looks like a list</cell></row></table></body>")
            .contains("| - looks like a list |")
    );

    // inert punctuation (quotes, parens, and other characters with no CommonMark meaning in
    // body text) must be left completely alone
    let inert = "<body><p>quote &quot; apostrophe ' paren ( ) percent % dollar $ at @ caret ^ eq = brace { } colon : semi ; comma , bang !</p></body>";
    assert_eq!(
        md(inert),
        "quote \" apostrophe ' paren ( ) percent % dollar $ at @ caret ^ eq = brace { } colon : semi ; comma , bang !"
    );
    // '!' alone (not immediately escaping a '[') does not need its own escaping
    assert_eq!(
        md("<body><p>! not an image on its own</p></body>"),
        "! not an image on its own"
    );

    // verbatim code (block and inline) must not have any of these characters escaped
    assert_eq!(
        md("<body><code>a_b`c[d]e&lt;f&gt;</code></body>"),
        "``a_b`c[d]e<f>``"
    );
    assert_eq!(
        md(r##"<body><p><hi rend="#t">a_b`c</hi></p></body>"##),
        "``a_b`c``"
    );

    // brackets in link/image text are still escaped so a false image ('![...]') can't form,
    // without needing to escape '!' itself: an escaped '[' already blocks the image syntax
    assert_eq!(
        md(r#"<body><p><ref target="http://x.com">![img]-like text</ref></p></body>"#),
        "[!\\[img\\]-like text](http://x.com)"
    );

    // a pre-existing literal backslash immediately before an underscore must round-trip:
    // rendered output should still show exactly one backslash then one underscore
    assert_eq!(
        md("<body><p>Edge case: a\\_b</p></body>"),
        "Edge case: a\\\\\\_b"
    );

    // include_formatting=False is plain-text mode: none of these characters are touched
    assert_eq!(
        md_with(
            "<body><p>This _stays_ as-is with `code` [b] &lt;tag&gt;</p></body>",
            false
        ),
        "This _stays_ as-is with `code` [b] <tag>"
    );
}

#[test]
fn test_markdown_link_code_brackets() {
    // Backslash escapes are literal inside a code span, so brackets in inline code within link text stay as-is.
    let cases = [
        ("<code>a[b]c</code>", "[`a[b]c`](http://x.com)"),
        // code spans bind tighter than link brackets, unbalanced ones included
        ("<code>a]b</code>", "[`a]b`](http://x.com)"),
        (r##"<hi rend="#t">x[0]</hi>"##, "[`x[0]`](http://x.com)"),
        ("<code>a`b[1]</code>", "[``a`b[1]``](http://x.com)"),
        // brackets in the surrounding link text are still escaped
        ("f <code>a]b</code> [q]", "[f `a]b` \\[q\\]](http://x.com)"),
    ];
    for (inner, expected) in cases {
        let body = format!(r#"<body><p><ref target="http://x.com">{inner}</ref></p></body>"#);
        assert_eq!(md(&body), expected);
    }
}

#[test]
fn test_xmltotxt_no_mutation() {
    // xmltotxt must not mutate its input tree (math/emphasis passes run on a deepcopy).
    let mut tree =
        xml_tree(r##"<body><p>formula \(x\) <hi rend="#b"><hi rend="#i">y</hi></hi></p></body>"##);
    let root = tree.root();
    let before = tree.tostring(root, false, false);
    let out = xml_to_text(&mut tree, Some(root), true, &Call::default()).unwrap();
    assert!(out.contains("$x$"));
    assert!(out.contains("***y***"));
    // source tree untouched
    assert_eq!(tree.tostring(root, false, false), before);
}

#[test]
fn test_math_conversion() {
    // LaTeX math delimiters are converted to $ notation; unmatched delimiters and code are left alone.
    // inline math \(...\) -> $...$
    assert_eq!(md("<body><p>\\(x^2\\)</p></body>"), "$x^2$");
    // unmatched \( must not become a lone $
    assert_eq!(md("<body><p>regex \\( open</p></body>"), "regex \\( open");
    // math delimiters inside a fenced code block must not be converted
    assert!(
        xmltotxt(
            "<body><code>prefix\n\\[E=mc^2\\]\nsuffix</code></body>",
            true
        )
        .contains("\\[E=mc^2\\]")
    );
    // math delimiters inside inline code must not be converted either
    assert!(
        xmltotxt(
            r##"<body><p>use <hi rend="#t">a\(x\)b</hi> here</p></body>"##,
            true
        )
        .contains("`a\\(x\\)b`")
    );
    // but math in a code element's prose tail is still converted
    assert!(
        xmltotxt("<body><p><code>k</code> then \\(t\\) end</p></body>", true)
            .contains("`k` then $t$ end")
    );
}

#[test]
fn test_math_is_not_commonmark_escaped() {
    // Inside a formula '_', '*', '[', ']', '<', '`' and '~' are LaTeX notation, so the
    // CommonMark escaping pass must step over math spans while still escaping the prose
    // around them.

    // subscripts and '<' survive inline math verbatim
    assert_eq!(
        md("<body><p>we write \\(a_1 &lt; a_2\\) here</p></body>"),
        "we write $a_1 < a_2$ here"
    );
    // block math too, including braces, stars and brackets
    assert_eq!(
        md("<body><p>\\[ x_i = \\sum_j A_{ij} y_j^* \\]</p></body>"),
        "$$\nx_i = \\sum_j A_{ij} y_j^*\n$$"
    );
    assert_eq!(
        md("<body><p>\\(\\left[u_0\\right]\\)</p></body>"),
        "$\\left[u_0\\right]$"
    );
    // prose on both sides of a formula is still escaped
    assert_eq!(
        md("<body><p>file_one has \\(n_1\\) rows in dir_two</p></body>"),
        "file\\_one has $n_1$ rows in dir\\_two"
    );
    // an unmatched delimiter is not a math span, so its text is escaped as ordinary prose
    assert_eq!(
        md("<body><p>regex \\( a_b open</p></body>"),
        "regex \\( a\\_b open"
    );
}

#[test]
fn test_inline_edge_cases() {
    // Cover lb in inline context, structural-tag fallback, and redundant emphasis collapse.
    // <lb/> inside an inline element (hi) renders as a newline
    assert!(
        xmltotxt(r##"<body><p><hi rend="#b">A<lb/>B</hi></p></body>"##, true).contains("**A\nB**")
    );
    // structural tag (e.g. <span>) nested inside inline: its text is carried through
    assert!(
        xmltotxt(
            r##"<body><p><hi rend="#b"><span>carried</span></hi></p></body>"##,
            true
        )
        .contains("**carried**")
    );
    // redundant nested emphasis is collapsed: **x** not ****x****
    assert_eq!(
        md(r##"<body><p><hi rend="#b"><hi rend="#b">x</hi></hi></p></body>"##),
        "**x**"
    );
}

#[test]
fn test_heading_inline_formatting() {
    // A heading keeps its # prefix even when it starts with an inline child (e.g. <h2><strong>...).

    // child-first headings must NOT lose the # prefix
    assert_eq!(
        md(r##"<body><head rend="h2"><hi rend="#b">B</hi> text</head></body>"##),
        "## **B** text"
    );
    assert_eq!(
        md(r##"<body><head rend="h2"><hi rend="#b">B</hi></head></body>"##),
        "## **B**"
    );
    assert_eq!(
        md(r##"<body><head rend="h3"><hi rend="#i">I</hi> rest</head></body>"##),
        "### *I* rest"
    );
    // no rend -> level 2
    assert_eq!(
        md(r##"<body><head><hi rend="#b">B</hi> t</head></body>"##),
        "## **B** t"
    );
    // text-first and plain headings are unchanged
    assert_eq!(
        md(r##"<body><head rend="h2">x <hi rend="#b">B</hi></head></body>"##),
        "## x **B**"
    );
    assert_eq!(
        md(r#"<body><head rend="h2">plain</head></body>"#),
        "## plain"
    );
    // a heading inside a table cell stays prefix-less
    assert_eq!(
        md(
            r##"<body><table><row><cell><head rend="h3"><hi rend="#b">H</hi></head></cell></row></table></body>"##
        ),
        "| **H** |"
    );
}

// The first assert uses `method="html"`, which writes the same as the XML method on this input.
// The metadata lines are `test_html_conversion_phase_4_lines`.
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
#[ignore = "phase 4"]
fn test_html_conversion_phase_4_lines() {
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
fn test_no_duplicate_paragraph_from_lb_tail() {
    // A text-bearing div moved into result_body is not revisited (alsumaria.tv), whitespace-only
    // <lb> tails are dropped.
    let html = concat!(
        "<html><body><article><div>",
        "First paragraph of the article, long enough to be kept by the extractor.<br/>",
        r#"        <div class="advert">        </div>        <br/>"#,
        "Second paragraph of the article, also long enough to be kept here.",
        "</div></article></body></html>"
    );
    let result = extract(html, &Kw::zero()).unwrap();
    assert_eq!(result.matches("Second paragraph of the article").count(), 1);
    assert!(result.contains("First paragraph of the article"));
    // no blank, space-filled lines from the pruned block's indentation
    assert!(
        !result
            .split('\n')
            .any(|line| !line.is_empty() && line.chars().all(char::is_whitespace))
    );
}

#[test]
fn test_extract_deep_caller_supplied_tree() {
    // Caller-supplied trees have no depth cap, so the content walk must not recurse. The thread
    // has the 512 KiB stack of a musl host's own threads (AGENTS.md, Runtime rules).
    let result = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            let (mut tree, root) = lxml_fromstring("<html><body><p>root text</p></body></html>");
            let mut node = tree.find(root, "body").unwrap();
            for _ in 0..3000 {
                let div = tree.new_element("div");
                tree.append(node, div);
                node = div;
            }
            tree.node_mut(node).text = Some(
                "Deep text long enough to be kept by the extractor, appearing at the very bottom."
                    .to_string(),
            );
            let settings = Kw::zero().settings();
            let call = Call::default();
            or_none(internal_extraction(
                Source::Tree(&tree, root),
                None,
                &ctx_for(&settings, &call),
                &ParseLimits::default(),
            ))
            .and_then(|document| document.text)
        })
        .unwrap()
        .join()
        .unwrap()
        .unwrap();
    assert!(result.contains("Deep text long enough"));
}

// The `prune_xpath` line is D-7: the crate has no caller selectors.
#[test]
fn test_extract_leaves_caller_tree_intact() {
    // A caller-supplied tree must come back unchanged.
    let (tree, root) = lxml_fromstring(
        "<html><body><p>First paragraph.</p><p>Second paragraph.</p></body></html>",
    );
    let before = tree.tostring(root, false, true);
    let settings = Kw::zero().settings();
    let call = Call::default();
    let document = internal_extraction(
        Source::Tree(&tree, root),
        None,
        &ctx_for(&settings, &call),
        &ParseLimits::default(),
    )
    .unwrap();
    // oracle: extract(tree, config=ZERO_CONFIG) with fast=True, no prune: both paragraphs
    assert_eq!(
        document.text.as_deref(),
        Some("First paragraph.\nSecond paragraph.")
    );
    assert_eq!(tree.tostring(root, false, true), before);
}

#[test]
fn test_text_after_block_is_kept() {
    // Regression: text directly after a list, table, quote or code block (its tail) was dropped:
    // _extract() rebuilds those blocks as new elements and left the source tail behind. It is now
    // kept as a paragraph of its own, exactly once.
    for block in [
        "<ul><li>Improve the tram network</li><li>Expand the bicycle lanes</li></ul>",
        "<table><tr><td>Budget</td><td>40</td></tr><tr><td>Staff</td><td>12</td></tr></table>",
        "<blockquote><p>We will deliver the plan on time and on budget.</p></blockquote>",
        "<pre>total = sum(values)</pre>",
    ] {
        let html = format!(
            "<html><body><article><h1>Headline</h1><p>{}</p>{block}These totals include every \
             department that submitted figures in time.<p>{}</p></article></body></html>",
            "The committee reviewed the proposal over several sessions. ".repeat(4),
            "The findings were published together with the underlying data. ".repeat(4)
        );
        let result = extract(&html, &Kw::zero()).unwrap();
        assert_eq!(
            result
                .matches("These totals include every department")
                .count(),
            1,
            "{block}"
        );
    }
}

#[test]
fn test_text_after_inline_code_not_duplicated() {
    // GH#951: inline code in a div is copied whole, tail included, so the tail must not also be
    // emitted as a paragraph of its own.
    let html = format!(
        "<html><body><article><p>{}</p><div>Tool events such as <code>PreToolUse</code> and \
         <code>PostToolUse</code> fire the same hooks.</div><p>{}</p></article></body></html>",
        "The committee reviewed the proposal over several sessions. ".repeat(4),
        "The findings were published together with the underlying data. ".repeat(4)
    );
    let result = extract(&html, &Kw::zero()).unwrap();
    assert!(result.contains("PreToolUse and PostToolUse fire the same hooks."));
    assert_eq!(result.matches("fire the same hooks").count(), 1);
}

#[test]
fn test_basic_cleaning_cookie_banner_scope() {
    // Regression: BASIC_CLEAN_XPATH's cookie/consent tokens are ANCHORED banner/CMP compounds, not
    // bare substrings: the substrings matched WP Cookie Notice BODY classes (cookies-not-set) and
    // topical content classes, deleting up to 97% of a page from baseline()/html2txt() and zeroing
    // the escalation-gate denominator. Banner containers (incl. CMP vendors) must still be pruned.
    let content = format!("<p>{}</p>", "Real article text about a subject. ".repeat(5));
    let banners = "<div id='onetrust-consent-sdk'><p>By clicking Accept you agree we can store cookies.</p></div>\
                   <div class='cookie-notice-container'><p>We use cookies to improve our service.</p></div>";
    let (mut tree, root) = lxml_fromstring(&format!(
        "<html><body class='single-post cookies-not-set'><div class='cookie-recipe-content'>{content}</div>{banners}</body></html>"
    ));
    let call = Call::default();
    let (_, text, _) = baseline_tree(&mut tree, root, &ParseLimits::default(), &call).unwrap();
    // body class + topical content class survive (the old substrings deleted both)
    assert!(text.contains("Real article text"));
    // banner containers pruned (substring lives only in banner prose)
    assert!(!text.contains("cookies"));
    let page_measure = html2txt_tree(&mut tree, root, true, &call).unwrap();
    assert!(page_measure.contains("Real article text"));
    assert!(!page_measure.contains("cookies"));
}

const COLSPAN_CONTENT_BASE: &str = "<tr><td>a</td><td>b</td><td>c</td></tr>";
const COLSPAN_CONTENT_ROW: &str = "<tr><td>a</td><td colspan='2'><p>b</p><p>c</p></td></tr>";

#[test]
fn test_table_colspan_content() {
    // A colspan=2 cell with block children is flattened; the placeholder fills to max_cols.
    for (rows, expected) in [
        (1, "| a | b | c | \n| a | b c |  |"),
        (2, "| a | b | c | \n| a | b c |  | \n| a | b c |  |"),
    ] {
        let html = format!(
            "<html><body><article><table>{COLSPAN_CONTENT_BASE}{}</table></article></body></html>",
            COLSPAN_CONTENT_ROW.repeat(rows)
        );
        assert_eq!(
            extract(&html, &Kw::zero().tables(true)).as_deref(),
            Some(expected),
            "{rows} rows"
        );
    }
}

const IMG_URL: &str = "http://aa.bb/c.jpg";
const IMG_TABLE_HEADER: &str = "<tr><td>a</td><td>b</td><td>c</td></tr>";
const IMG_TABLE_TAIL: &str = "<td><p>b</p><p>c</p></td><td>d</td>";

#[test]
fn test_table_image_in_cell() {
    // Images in table cells render as GFM inline images; link wrapping is preserved.
    for (cell1, expected_cell1) in [
        (
            format!(r#"<td>a<img src="{IMG_URL}" alt="img"/><span>a</span></td>"#),
            format!("a ![img]({IMG_URL}) a"),
        ),
        (
            format!(
                r#"<td><a href="{IMG_URL}"><img src="{IMG_URL}" alt="img"/><span>a</span></a></td>"#
            ),
            format!("![img]({IMG_URL}) a"),
        ),
        (
            format!(r#"<td><img src="{IMG_URL}" alt="img"/><span>a</span></td>"#),
            format!("![img]({IMG_URL}) a"),
        ),
        (
            format!(
                r#"<td><img src="{IMG_URL}" alt="img1"/><span>a</span><img src="{IMG_URL}" alt="img2"/></td>"#
            ),
            format!("![img1]({IMG_URL}) a ![img2]({IMG_URL})"),
        ),
    ] {
        let html = format!(
            "<html><body><article><table>{IMG_TABLE_HEADER}<tr>{cell1}{IMG_TABLE_TAIL}</tr></table></article></body></html>"
        );
        let result = extract(&html, &Kw::zero().markdown().images(true).tables(true));
        assert_eq!(
            result,
            Some(format!("| a | b | c | \n| {expected_cell1} | b c | d |")),
            "{cell1}"
        );
    }
}

/// A link and bold paragraph, plus a table cell with a link and bold and an image cell: shared by
/// the combination tests.
const COMBO_DOC: &str = concat!(
    "<html><body><article>",
    r#"<p>Intro with a <a href="http://x.io/p">link</a> and <b>bold</b> word.</p>"#,
    "<table><tr><td>h1</td><td>h2</td></tr>",
    r#"<tr><td><a href="http://x.io/c"><b>bold link</b></a></td>"#,
    r#"<td><img src="http://x.io/i.jpg" alt="pic"/></td></tr></table>"#,
    "</article></body></html>"
);

/// `extract(_COMBO_DOC, output_format="markdown", config=ZERO_CONFIG, **_COMBO_ALL_ON)`.
fn combo_all_on() -> Kw {
    Kw::zero()
        .markdown()
        .links(true)
        .formatting(true)
        .images(true)
        .tables(true)
}

#[test]
fn test_combined_links_formatting_images_tables() {
    // All four flags on: cells keep formatting + images, and a link wrapping formatting keeps its
    // target.
    assert_eq!(
        extract(COMBO_DOC, &combo_all_on()).as_deref(),
        Some(concat!(
            "Intro with a [link](http://x.io/p) and **bold** word.\n\n",
            "| h1 | h2 | \n| [**bold link**](http://x.io/c) | ![pic](http://x.io/i.jpg) |"
        ))
    );
}

#[test]
fn test_combined_flags_toggle_off() {
    // Pin current behavior: disabling one flag changes only its feature (combined doc).
    let all_on = combo_all_on();
    for (kw, expected) in [
        (
            all_on.clone().links(false),
            "Intro with a link and **bold** word.\n\n| h1 | h2 | \n| **bold link** | ![pic](http://x.io/i.jpg) |",
        ),
        (
            all_on.clone().formatting(false),
            "Intro with a [link](http://x.io/p) and bold word.\n| h1 | h2 | \n| [bold link](http://x.io/c) | ![pic](http://x.io/i.jpg) |",
        ),
        (
            all_on.clone().images(false),
            "Intro with a [link](http://x.io/p) and **bold** word.\n\n| h1 | h2 | \n| [**bold link**](http://x.io/c) |  |",
        ),
        (
            all_on.tables(false),
            "Intro with a [link](http://x.io/p) and **bold** word.",
        ),
    ] {
        assert_eq!(extract(COMBO_DOC, &kw).as_deref(), Some(expected));
    }
}

/// A first-row tail cell and a 3-cell second row that sets the table to 3 columns; the first row
/// fills to 3 via a colspan placeholder or a trailing pad depending on the span.
const COLSPAN_ROWS: &str = "<td>b</td></tr><tr><td>c</td><td>d</td><td>e</td></tr></table>";

#[test]
fn test_table_colspan_padding() {
    // A colspan=2 cell materializes an inline placeholder; the row fills to the table's column
    // count.
    assert!(
        table_txt(&format!("<table><tr><td colspan='2'>a</td>{COLSPAN_ROWS}"))
            .unwrap()
            .contains("| a |  | b |")
    );
}

#[test]
fn test_table_bad_span_attr_treated_as_colspan1() {
    // Non-colspan span attributes are ignored (colspan=1); trailing pad fills to max_cols.
    for bad_span in [
        r#"span="2""#,
        r#"span="2.1""#,
        r#"span="-1""#,
        r#"span="abc""#,
    ] {
        assert!(
            table_txt(&format!("<table><tr><td {bad_span}>a</td>{COLSPAN_ROWS}"))
                .unwrap()
                .contains("| a | b |  |"),
            "{bad_span}"
        );
    }
}

#[test]
fn test_table_huge_or_bad_colspan_no_crash() {
    // Huge or non-numeric colspan must not crash or discard the document (#657).
    for first_cell in [
        r#"<td colspan="9007199254740991">a</td>"#,
        r#"<th colspan="9007199254740991">a</th>"#,
        r#"<td colspan="2x">a</td>"#,
    ] {
        assert!(
            table_txt(&format!("<table><tr>{first_cell}{COLSPAN_ROWS}")).is_some(),
            "{first_cell}"
        );
    }
}

#[test]
fn test_table_rowspan() {
    // Rowspan placeholders keep the columns aligned.
    for (table, suffix) in [
        // a rowspan leaves a placeholder in the continuation row
        (
            "<tr><td rowspan='2'>x</td><td>a</td></tr><tr><td>b</td></tr>",
            "| x | a | \n|  | b |",
        ),
        // rowspan + colspan registers every spanned column
        (
            "<tr><td rowspan='2' colspan='2'>big</td><td>c</td></tr><tr><td>x</td></tr>",
            "|  |  | x |",
        ),
        // padding a short row consumes the rowspan, no stale placeholder below
        (
            "<tr><td>a</td><td rowspan='2'>b</td><td>c</td></tr><tr><td>x</td></tr><tr><td>d</td><td>e</td><td>f</td></tr>",
            "| d | e | f |",
        ),
        // an empty <tr> still consumes one row of a pending rowspan
        (
            "<tr><td rowspan='3'>a</td><td>b</td></tr><tr></tr><tr><td>c</td></tr>",
            "| a | b | \n|  | c |",
        ),
        (
            "<tr><td rowspan='2'>a</td><td>b</td></tr><tr></tr><tr><td>c</td><td>d</td></tr>",
            "| a | b | \n| c | d |",
        ),
    ] {
        assert!(
            table_md(&format!("<table>{table}</table>"), Kw::zero())
                .unwrap()
                .ends_with(suffix),
            "{table}"
        );
    }
}

#[test]
fn test_table_empty_cells_and_rows() {
    // Empty cells are kept blank to keep columns aligned; rows that are entirely empty are dropped.
    for (html, suffix) in [
        ("<table><tr><td></td><td>b</td></tr></table>", "|  | b |"),
        ("<table><tr><td>a</td><td></td></tr></table>", "| a |  |"),
        (
            "<table><tr><td>a</td><td>b</td></tr><tr><td></td><td></td></tr></table>",
            "| a | b |",
        ),
        (
            "<table><tr><td>a</td><td>c</td></tr><tr><td></td><td></td></tr><tr><td>d</td><td>e</td></tr></table>",
            "| a | c | \n| d | e |",
        ),
        (
            "<table><tr><td>a</td><td>c</td></tr><tr></tr><tr><td>d</td><td>e</td></tr></table>",
            "| a | c | \n| d | e |",
        ),
    ] {
        assert!(
            table_md(html, Kw::zero()).unwrap().ends_with(suffix),
            "{html}"
        );
    }
}

#[test]
fn test_table_cell_list_no_row_break() {
    // A <ul> in a cell (no recall mode) must not inject a row-breaking newline.
    let result = table_md(
        "<table><tr><td><ul><li>i1</li><li>i2</li></ul></td><td>b</td></tr></table>",
        Kw::zero(),
    )
    .unwrap();
    let row = result.rsplit("\n\n").next().unwrap();
    assert!(!row.contains('\n'));
    assert!(row.ends_with("| b |"));
}

#[test]
fn test_table_cell_block_elements_flattened() {
    // Block elements in a cell are flattened to inline: no ## marker, space-separated, no mashing.
    for (html, suffix, kw) in [
        (
            "<table><tr><td><h2>Title</h2></td><td>b</td></tr></table>",
            "| Title | b |",
            Kw::zero(),
        ),
        (
            "<table><tr><td><p>para</p></td><td>b</td></tr></table>",
            "| para | b |",
            Kw::zero(),
        ),
        (
            "<table><tr><td><h2>Title</h2>txt</td><td>b</td></tr></table>",
            "| Title txt | b |",
            Kw::zero(),
        ),
        (
            "<table><tr><td><ul><li>i1</li><li>i2</li></ul></td><td>b</td></tr></table>",
            "| i1 i2 | b |",
            Kw::zero().focus(Focus::Recall),
        ),
    ] {
        assert!(table_md(html, kw).unwrap().ends_with(suffix), "{html}");
    }
}

#[test]
fn test_table_cell_keeps_nested_formatting() {
    // Regression #829/#396: formatting/images wrapped in a block inside a cell must survive.
    for (expected, cell, kw) in [
        (
            "**bold**",
            "<p><b>bold</b></p>",
            Kw::zero().formatting(true),
        ),
        (
            "![a](/i.jpg)",
            "<p><img src='/i.jpg' alt='a'></p>",
            Kw::zero().images(true),
        ),
        (
            "pre **mid** post",
            "<p>pre <b>mid</b> post</p>",
            Kw::zero().formatting(true),
        ),
        (
            "x ~~gone~~ y",
            "<p>x <del>gone</del> y</p>",
            Kw::zero().formatting(true),
        ),
        (
            "x `c` y",
            "<p>x <code>c</code> y</p>",
            Kw::zero().formatting(true),
        ),
    ] {
        assert!(
            table_md(
                &format!("<table><tr><td>{cell}</td><td>x</td></tr></table>"),
                kw
            )
            .unwrap()
            .contains(expected),
            "{cell}"
        );
    }
}

#[test]
fn test_include_images_does_not_truncate() {
    // Regression #194/#842: a lead image plus one paragraph must not let _extract break early and
    // drop later content. Real config + a >250-char lead so wild-text recovery does not fire and
    // mask the early break.
    let lead = "This single lead paragraph is deliberately long enough to exceed the minimum extracted size. ".repeat(4);
    let continuation: String = (1..5)
        .map(|i| {
            format!(
                "<p>Continuation paragraph {i} that must also survive extraction in full here.</p>"
            )
        })
        .collect();
    let doc = format!(
        "<html><body><article><img src='/lead.jpg' alt='lead'><p>{lead}</p></article><div id='content'>{continuation}</div></body></html>"
    );
    let result = extract(&doc, &Kw::new().images(true)).unwrap_or_default();
    assert!(result.contains("/lead.jpg"));
    for i in 1..5 {
        assert!(
            result.contains(&format!("Continuation paragraph {i}")),
            "{i}"
        );
    }
}

#[test]
fn test_no_duplicate_content() {
    // Regression #768/#817: content must not be emitted twice (overlapping candidates / wild-text
    // recovery). Real config: ZERO_CONFIG's min_extracted_size=0 hides #817.
    let dup768 = "<!doctype html><body><main><article><div><br>Line that has to have at least 125 characters for the bug to appear so here is some filler text text text text text text text</div></article></main></body></html>";
    assert_eq!(
        extract(dup768, &Kw::new())
            .unwrap_or_default()
            .matches("Line that has to have")
            .count(),
        1
    );
    let dup817 = "<html><body><div id='content'><p>Authoritative taxonomy of but let us leave it as it is 1 2 3</p></div><p>some text long enough not to skip and printed twice on this line some text long enough not to skip and printed twice on this line</p></body></html>";
    assert_eq!(
        extract(dup817, &Kw::new())
            .unwrap_or_default()
            .matches("Authoritative taxonomy")
            .count(),
        1
    );
    // #879: a small <article>/<main> body (below min_extracted_size) triggers wild-text recovery,
    // which must not re-append the paragraphs the main pass already collected (interleaved dupes)
    let dup879 = concat!(
        "<html><body><nav>menu chrome</nav><article><h1>The Example Chronicle</h1>",
        "<p>First synthetic paragraph of adequate length for extraction to engage properly.</p>",
        "<p>Second synthetic paragraph, also long enough to matter for the extractor.</p>",
        "</article><footer>footer chrome</footer></body></html>"
    );
    for doc in [dup879.to_string(), dup879.replace("article>", "main>")] {
        let out = extract(&doc, &Kw::new()).unwrap_or_default();
        assert_eq!(out.matches("First synthetic paragraph").count(), 1);
        assert_eq!(out.matches("Second synthetic paragraph").count(), 1);
    }
}

#[test]
fn test_short_document_keeps_structure() {
    // Regression #896: the baseline rescue must not flatten a valid short extraction it cannot
    // improve on.
    let doc = concat!(
        "<html><body><article><h1>Notice Title</h1>",
        "<p>This municipal notice concerns the closure of the main road for repairs.</p>",
        "<ul><li>Detour signposted east</li><li>Works end on Friday</li></ul>",
        "</article></body></html>"
    );
    // real config: the doc is below min_extracted_size, so the rescue engages (ZERO_CONFIG hides it)
    let result = extract(doc, &Kw::new().markdown().formatting(true)).unwrap_or_default();
    assert!(result.contains("# Notice Title"));
    assert!(result.contains("- Detour signposted east"));
}

#[test]
fn test_short_document_baseline_rescue_accepted() {
    // The other side of the #896 gate: when the rescue does add text, it must still win.
    // Distinct sentences: a repeated one trips the body-level duplicate_test instead. The empty
    // <body> keeps the main extractor at zero, so the rescue is what supplies the text.
    let body = (0..6)
        .map(|i| format!("Sentence {i} of a body that arrives only as structured data."))
        .collect::<Vec<_>>()
        .join(" ");
    let doc = format!(
        r#"<html><body><script type="application/ld+json">{{"@type": "Article", "articleBody": "{body}"}}</script></body></html>"#
    );
    assert!(
        extract(&doc, &Kw::new())
            .unwrap_or_default()
            .contains("Sentence 3 of a body")
    );
}

#[test]
fn test_no_duplicate_content_nonadjacent() {
    // Regression T6 (non-adjacent case): the duplicate is not the element immediately preceding it
    // in result_body once genuine wild text is recovered in between: an adjacent-only dedup check
    // would miss it.
    let dup = format!(
        "{} short duplicate description text for the list item here right now please.",
        "X".repeat(30)
    );
    let wild = format!(
        "{} this is genuinely separate wild text living outside the article container elsewhere in the page body content over here, quite far removed from it.",
        "Y".repeat(30)
    );
    let html = format!(
        "<html><body><p>{wild}</p><article><dl><dt>Term</dt><dd><p>{dup}</p></dd></dl></article></body></html>"
    );
    let result = extract(&html, &Kw::new()).unwrap_or_default();
    assert_eq!(result.matches(&dup).count(), 1);
    assert_eq!(result.matches(&wild).count(), 1);
    assert!(result.contains("Term"));
}

#[test]
fn test_no_duplicate_content_short_elements() {
    // Regression #634: on short pages, recovery must not re-add elements the main pass already
    // extracted, even when their text is below the substring-dedup length gate.
    let body = concat!(
        r#"<p>this is sample intro</p><h3 class="wp-block-heading">intro 2</h3><p>table below</p>"#,
        r#"<figure class="wp-block-table"><table><tbody><tr><td>a</td><td>b</td><td></td></tr>"#,
        "<tr><td>f</td><td>s</td><td>s</td></tr><tr><td>g</td><td></td><td>b</td></tr></tbody></table></figure>",
        "<p>header table below</p>",
        r#"<figure class="wp-block-table"><table><thead><tr><th>b</th><th>s</th><th>h</th></tr></thead>"#,
        "<tbody><tr><td>a</td><td>b</td><td></td></tr><tr><td>f</td><td>s</td><td>s</td></tr>",
        "<tr><td>g</td><td></td><td>b</td></tr></tbody></table></figure>",
        "<p>list below</p><ul><li>this is 1</li><li>this is 2</li><li>this is 3</li></ul>",
        "<p>numbered list below</p><ol><li>this is 1</li><li>this is 2</li><li>this is 3</li></ol>"
    );
    let doc = format!(r#"<html><body><div class="entry-content">{body}</div></body></html>"#);
    let result = extract(&doc, &Kw::new()).unwrap_or_default();
    assert_eq!(result.matches("this is sample intro").count(), 1);
}

#[test]
fn test_recover_wild_text_inline_formatting_dedup() {
    // Regression: the recovery dedup must render accumulated text per element with plain
    // concatenation, exactly like the candidates: ' '.join across all text nodes invented spaces
    // at inline-tag boundaries (Hyper<b>link</b>ed -> 'Hyper link ed'), so a paragraph folded into
    // a list item escaped the substring check and was emitted twice.
    let para = concat!(
        "This paragraph has Hyper<b>link</b>ed formatting inside and needs to be comfortably ",
        "longer than the fifty character dedup gate to be caught by the substring check."
    );
    let doc = format!(
        "<html><body><article><dl><dt>Term one</dt><dd><p>{para}</p></dd></dl></article></body></html>"
    );
    let result = extract(&doc, &Kw::new().formatting(true)).unwrap_or_default();
    assert_eq!(result.matches("formatting inside").count(), 1);
}

#[test]
fn test_recall_escalation() {
    // A short balanced extraction covering little of the page is retried in recall mode: wild
    // paragraphs block the baseline rescue while the bulk of the content sits in bare divs.
    let ps: String = (0..3)
        .map(|i| {
            format!("<p>Wild paragraph number {i} directly under body, with enough words to pass the paragraph checks in place.</p>")
        })
        .collect();
    let divs: String = (0..30)
        .map(|i| {
            format!("<div>Main content block {i} living in a bare div element with plenty of meaningful words to matter here today.</div>")
        })
        .collect();
    let doc = format!("<html><body>{ps}{divs}</body></html>");
    let result = extract(&doc, &Kw::new()).unwrap_or_default();
    assert_eq!(result.matches("Wild paragraph").count(), 3);
    assert_eq!(result.matches("Main content block").count(), 30);
}

/// Shared by the recall-escalation tests: long enough to clear the minimum extraction size.
fn escalation_intro() -> String {
    format!(
        "<p>{}</p>",
        "Introductory paragraph with regular prose content that is moderately long and clearly \
         meaningful, providing enough text to exceed the minimum extraction size threshold used \
         by the extractor. "
            .repeat(2)
    )
}

const DFP_JSONLD: &str = concat!(
    r#"<script type="application/ld+json">{"@context":"https://schema.org","#,
    r#""@type":"DiscussionForumPosting","headline":"Test thread"}</script>"#
);

/// `_thread_doc`: article intro plus 8 replies in a comments-classed container. Unwrapped, there is
/// no article-region marker at all, so body selection stays broad.
fn thread_doc(jsonld: &str, reply_tag: &str, intro: &str, wrap: bool) -> String {
    let replies: String = (0..8)
        .map(|i| {
            format!(
                "<{reply_tag} class='comment-body'>Reply number {i} contains substantial discussion \
                 content with plenty of genuine words and enough length to be recognized as real text \
                 by a paragraph density classifier, not boilerplate at all today, said the \
                 commenter.</{reply_tag}>"
            )
        })
        .collect();
    let article = if wrap {
        format!("<article>{intro}</article>")
    } else {
        format!("<div>{intro}</div>")
    };
    format!(
        "<html><head>{jsonld}</head><body>{article}<div id='comments' class='comments-area'>{replies}</div></body></html>"
    )
}

#[test]
fn test_recall_escalation_justext_comment_scoping() {
    // Regression: justext has no article-region scoping of its own and will absorb a reader-comments
    // thread as if it were content (AEB regression: a short article inviting comments, justext
    // grabbed the replies too): comment containers must be stripped before it runs, UNLESS this is
    // a thread-forum (schema.org DiscussionForumPosting), where those same containers hold the
    // actual posts, not boilerplate replies (WCXB regression fixed by _forum_thread_page). The
    // fast=True case is the rule-based retry alone (justext is skipped in fast mode): it must apply
    // the same comment-scoping, or a reader-comments thread leaks into the body precisely when the
    // caller asked to exclude comments.
    let intro = escalation_intro();
    let kw = Kw::new().comments(false);
    // blog_excludes_comments, deviation: D-1, upstream runs it with `fast=False` (the value is the
    // same in fast mode)
    let result = extract(&thread_doc("", "div", &intro, true), &kw).unwrap_or_default();
    assert!(result.contains("Introductory paragraph"));
    assert_eq!(result.matches("Reply number").count(), 0);
    // forum_keeps_posts, deviation: D-1, upstream asserts 8 replies with `fast=False`; the rule-based
    // retry of fast mode keeps none; oracle: extract(doc, output_format="txt",
    // include_comments=False) with fast=True: result.count("Reply number") == 0
    let result = extract(&thread_doc(DFP_JSONLD, "div", &intro, true), &kw).unwrap_or_default();
    assert!(result.contains("Introductory paragraph"));
    assert_eq!(result.matches("Reply number").count(), 0);
    // blog_excludes_comments_fast
    let result = extract(&thread_doc("", "div", &intro, true), &kw).unwrap_or_default();
    assert!(result.contains("Introductory paragraph"));
    assert_eq!(result.matches("Reply number").count(), 0);
}

#[test]
fn test_recall_escalation_no_comment_doubling() {
    // Regression: with include_comments=True on a thread-forum page, posts must not appear in both
    // text and comments (the escalation stage sees the raw tree, whose comment containers
    // extract_comments already consumed). Policy: comment capture on a DFP page is re-routed into
    // the body (the 'comments' are the posts); partial captures must not lose the rest of the
    // thread either (WCXB regression: pruning all containers because a fragment was captured
    // tanked 3 forum pages). <p> replies are capturable, <div> replies are not (extract_comments
    // rejects divs).
    let intro = escalation_intro();
    // txt output concatenates text and comments: exactly one occurrence per post overall
    let result =
        extract(&thread_doc(DFP_JSONLD, "p", &intro, true), &Kw::new()).unwrap_or_default();
    assert!(result.contains("Introductory paragraph"));
    assert_eq!(result.matches("Reply number").count(), 8);
    // capture_missed, deviation: D-1, upstream asserts 8 with `fast=False`; fast mode keeps no reply
    // of the uncaptured div thread; oracle: extract(doc, output_format="txt",
    // include_comments=True) with fast=True: result.count("Reply number") == 0
    let result =
        extract(&thread_doc(DFP_JSONLD, "div", &intro, true), &Kw::new()).unwrap_or_default();
    assert!(result.contains("Introductory paragraph"));
    assert_eq!(result.matches("Reply number").count(), 0);
}

#[test]
fn test_dfp_long_opening_post_keeps_replies() {
    // Regression (review round 3): the DFP re-route emptied the captured comments betting on the
    // stage-4 escalation to restore the posts into the body, but the escalation is gated on
    // len_text < ESCALATION_MAX_LENGTH: a thread whose opening post alone exceeds the gate lost
    // every reply from both channels. Captured posts must survive regardless of gates.
    let long_op: String = (0..16)
        .map(|i| {
            format!(
                "<p>Opening post paragraph {i} lays out the question in full detail, with context, \
                 history and several worked examples that make the thread starter alone longer than \
                 the escalation length gate, so no rescue pass can be relied upon for the replies.</p>"
            )
        })
        .collect();
    let result =
        extract(&thread_doc(DFP_JSONLD, "p", &long_op, true), &Kw::new()).unwrap_or_default();
    assert!(result.contains("Opening post paragraph"));
    assert_eq!(result.matches("Reply number").count(), 8);
}

#[test]
fn test_dfp_precision_keeps_posts() {
    // Regression (review round 3): precision mode pruned REMOVE_COMMENTS_XPATH from the very tree
    // the forum re-route had just restored the posts into, and precision closes the stage-3/4
    // rescues, so a DFP page returned no posts at all.
    let doc = thread_doc(DFP_JSONLD, "p", &escalation_intro(), true);
    let result = extract(&doc, &Kw::new().focus(Focus::Precision)).unwrap_or_default();
    assert_eq!(result.matches("Reply number").count(), 8);
}

#[test]
fn test_recall_escalation_blog_comment_leak() {
    // Regression (review round 3): on a wrapper-less blog layout (broad body selection) with
    // include_comments=True and an uncapturable div-based reply thread, the recall retry kept the
    // comment containers (its prune gate checked options.comments, the justext branch checked
    // len_comments) and emitted the reader comments as main body text.
    let doc = thread_doc("", "div", &escalation_intro(), false);
    // deviation: D-1, upstream runs the "full" case with `fast=False` (the value is the same in
    // fast mode), then the "fast" case, which is this one
    let result = extract(&doc, &Kw::new()).unwrap_or_default();
    assert!(result.contains("Introductory paragraph"));
    assert_eq!(result.matches("Reply number").count(), 0);
}

#[test]
fn test_escalation_retry_no_comment_capture() {
    // Regression (review round 3): the stage-4 retry re-ran extract_comments although the caller
    // discards the retry's comments triple: a container matching COMMENTS_XPATH but not
    // REMOVE_COMMENTS_XPATH had its text captured and deleted inside the retry, so it vanished from
    // both channels. The retry must not capture comments at all.
    let intro: String = (0..4)
        .map(|i| {
            format!("<li>Point number {i} of the short visible article summary text here.</li>")
        })
        .collect();
    let cont: String = (0..12)
        .map(|i| {
            format!(
                "<p>Continuation paragraph {i} with substantial article prose that only the \
                 recall retry can recover from inside the form wrapper element.</p>"
            )
        })
        .collect();
    let replies: String = (0..8)
        .map(|i| {
            format!(
                "<p>Reply {i} ZQXJKVREPLY says something reasonably long about the topic at hand \
                 in this discussion thread below the article body.</p>"
            )
        })
        .collect();
    let doc = format!(
        "<html><body><article><h1>Title of the page</h1><ul>{intro}</ul><form>{cont}<div class='user-comment-area'>{replies}</div></form></article></body></html>"
    );
    let result = extract(&doc, &Kw::new()).unwrap_or_default();
    assert!(result.contains("Continuation paragraph 5"));
    assert!(result.contains("ZQXJKVREPLY"));
}

#[test]
fn test_main_pass_excludes_comments_when_disabled() {
    // Regression: with include_comments=False, comment containers must be pruned before the MAIN
    // pass, not only in precision mode: balanced/recall let the div-ladder pull a short article's
    // comment thread into the body (a bare-div thread the comment extractor cannot capture).
    let intro = "<p>Short intro under the escalation and rescue thresholds here.</p>";
    let replies: String = (0..8)
        .map(|i| {
            format!("<div>Reader comment number {i} that must never appear when comments are excluded, long enough to matter.</div>")
        })
        .collect();
    let doc = format!(
        "<html><body><article>{intro}</article><div id='comments' class='comments-area'>{replies}</div></body></html>"
    );
    // WordPress comment lists: still ol/ul on the raw tree
    let items = replies.replace("<div>", "<li>").replace("</div>", "</li>");
    let doc_lists = [("ol", "commentlist"), ("ul", "comment-list")].map(|(tag, class)| {
        format!(
            "<html><body><article>{intro}</article><{tag} class='{class}'>{items}</{tag}></body></html>"
        )
    });
    // deviation: D-1, upstream runs every page with `fast=False` and with `fast=True`; the values
    // are the same
    for page in std::iter::once(&doc).chain(&doc_lists) {
        let result = extract(page, &Kw::new().comments(false)).unwrap_or_default();
        assert!(result.contains("Short intro"));
        assert_eq!(result.matches("Reader comment number").count(), 0);
    }
}

#[test]
fn test_main_pass_excludes_details_wrapped_comments() {
    // Regression (#850): comment containers wrapped in <details> must also be pruned when
    // include_comments=False: REMOVE_COMMENTS_XPATH matched only div/list/section, so a
    // <details id='comments'> thread leaked into the body outside precision mode. A <details>
    // without a comment id/class must be kept (no over-pruning).
    let body = format!(
        "<article>{}</article>",
        "<p>Real article paragraph with enough content to be extracted normally here.</p>"
            .repeat(3)
    );
    let comments: String = format!(
        "<details id='comments'><summary>Comments</summary>{}</details>",
        (0..6)
            .map(|i| format!(
                "<p>Reader comment number {i} that must never leak into the body text.</p>"
            ))
            .collect::<String>()
    );
    let doc = format!("<html><body>{body}{comments}</body></html>");
    // deviation: D-1, upstream runs this with `fast=False` and with `fast=True`; the values are the
    // same
    let result = extract(&doc, &Kw::new().comments(false)).unwrap_or_default();
    assert!(result.contains("Real article paragraph"));
    assert_eq!(result.matches("Reader comment number").count(), 0);
    // a non-comment <details> is legitimate content and must survive
    let faq = "<details class='faq'><summary>More</summary><p>Kept expandable content paragraph that is genuine.</p></details>";
    let doc_faq = format!("<html><body>{body}{faq}</body></html>");
    assert!(
        extract(&doc_faq, &Kw::new())
            .unwrap_or_default()
            .contains("Kept expandable content")
    );
    // "commentary" is content, not reader comments
    for attr in ["id", "class"] {
        let commentary = format!(
            "<section {attr}='commentary'><p>Expert commentary paragraph that is part of the article.</p></section>"
        );
        let doc_commentary = format!("<html><body>{body}{commentary}</body></html>");
        assert!(
            extract(&doc_commentary, &Kw::new().comments(false))
                .unwrap_or_default()
                .contains("Expert commentary"),
            "{attr}"
        );
    }
}

#[test]
fn test_infinite_scroll_container_with_h1_kept() {
    // Regression: the #911 prune deleted the real article when the scroll container holds the h1
    // (BGR).
    let real = "<p>Real article paragraph with enough content to be extracted normally here.</p>"
        .repeat(4);
    let appended =
        "<p>Appended follow-up story text that belongs to another article entirely.</p>".repeat(4);
    // deviation: D-1, upstream runs the checks below with `fast=False` and with `fast=True`; the
    // values are the same
    let doc = format!(
        "<html><body><div class='site-content infinite-scroll-container'><article><h1>Real title</h1>{real}</article></div></body></html>"
    );
    assert!(
        extract(&doc, &Kw::new())
            .unwrap_or_default()
            .contains("Real article paragraph")
    );
    // still pruned: an infinite-scroll container without an h1, an mvp container even with one
    for (class, head) in [("infinite-scroll", "h2"), ("mvp-post-add-box", "h1")] {
        let doc = format!(
            "<html><body><article><h1>Real title</h1>{real}<div class='{class}'><{head}>Next story</{head}>{appended}</div></article></body></html>"
        );
        let result = extract(&doc, &Kw::new()).unwrap_or_default();
        assert!(result.contains("Real article paragraph"), "{class}");
        assert!(!result.contains("Appended follow-up"), "{class}");
    }
}

#[test]
fn test_markdown_inline_rendering() {
    // Inline markers, links and list markers in markdown (regressions #843, A5, marker
    // composition).
    for (body, expected) in [
        ("<ul><li><p>x <b>bold</b> y</p></li></ul>", "- x **bold** y"),
        ("<p>a <b> x </b> b</p>", "a  **x**  b"),
        ("<p>a <i> x </i> b</p>", "a  *x*  b"),
        ("<p>a <del> x </del> b</p>", "a  ~~x~~  b"),
        ("<p>a <code> x </code> b</p>", "a  `x`  b"),
        ("<p>a <b>   </b> b</p>", "a     b"),
        (
            "<p>see <a href='/x'> bold link </a> ok</p>",
            "see  [bold link](/x)  ok",
        ),
        (
            "<ol><li>one</li><li>two</li><li>three</li></ol>",
            "1. one\n2. two\n3. three",
        ),
        ("<ol><li>only</li></ol>", "1. only"),
        ("<ul><li>a</li><li>b</li></ul>", "- a\n- b"),
        (
            "<ul><li>a<ul><li>b</li><li>c</li></ul></li><li>d</li></ul>",
            "- a\n  - b\n  - c\n- d",
        ),
        ("<ul><li>a<ol><li>b</li></ol></li></ul>", "- a\n  1. b"),
        (
            "<ul><li><ul><li>b</li></ul></li><li>d</li></ul>",
            "  - b\n- d",
        ),
        ("<ul><li><p>a</p></li><li>b</li></ul>", "- a\n- b"),
        ("<ul><li><br/>a</li><li>b<br/>c</li></ul>", "- a\n- b\nc"),
        (
            r#"<ul><li><a href="/x"></a></li><li><a href="/y">b</a></li><li>c</li></ul>"#,
            "- [b](/y)\n- c",
        ),
        (
            "<ul><li>see <a href='http://x.com'><b>bold link</b></a> here</li></ul>",
            "- see [**bold link**](http://x.com) here",
        ),
        (
            "<ul><li>see <a href='http://x.com'>link <b>bold</b></a> here</li></ul>",
            "- see [link **bold**](http://x.com) here",
        ),
        (
            "<p>see <a href='http://x.com'><b>bold</b></a> more</p>",
            "see [**bold**](http://x.com) more",
        ),
        (
            "<p>text <b><i>nested</i></b> end</p>",
            "text ***nested*** end",
        ),
        ("<p><b>prefix <i>italic</i></b></p>", "**prefix *italic***"),
        (
            "<ul><li>text <b><i>nested</i></b> end</li></ul>",
            "- text ***nested*** end",
        ),
        (
            "<blockquote><b>bold</b> text here</blockquote>",
            "**bold** text here",
        ),
        ("<del>gone</del>", "~~gone~~"),
        (
            "<ul><li>text <del>struck</del> more</li></ul>",
            "- text ~~struck~~ more",
        ),
        (
            "<blockquote>text <del>struck</del> more</blockquote>",
            "text ~~struck~~ more",
        ),
        (
            "<ul><li>use <code>func()</code> here</li></ul>",
            "- use `func()` here",
        ),
        (
            "<p>before <b>bold <del>struck</del></b></p>",
            "before **bold ~~struck~~**",
        ),
        (
            "<ol><li>Foo <em>bar</em> baz.</li></ol>",
            "1. Foo *bar* baz.",
        ),
        (
            "<blockquote><p>A <b>bold</b> word</p></blockquote>",
            "A **bold** word",
        ),
        (
            "<blockquote><p>see <a href='http://x.com'>link</a></p></blockquote>",
            "see [link](http://x.com)",
        ),
        (
            "<blockquote><p>text</p><img src='x.jpg' alt='img'/></blockquote>",
            "text\n\n![img](x.jpg)",
        ),
        (
            "<ul><li><img src='/i.jpg' alt='a'></li><li>plain</li></ul>",
            "- ![a](/i.jpg)\n- plain",
        ),
    ] {
        assert_eq!(
            md_inline(body, Kw::zero().links(true).images(true)),
            format!("{INTRO}\n\n{expected}"),
            "{body}"
        );
    }
}

#[test]
fn test_loose_text_tail_not_squished() {
    // Regression #661: a block's loose-text tail must not mash onto the next sibling block.
    let html = r#"<!DOCTYPE html><html lang="en-us"><body><main><section>
        <p>First</p>
        This gets Squished
        <div>
            <h4>There should be a space</h4>
            <p>Another sentence</p>
            This also gets Squished
        </div>
        <div>
            <h4>Where is the space</h4>
            <p>This sentence has to be long enough. If it's long enough the duplication stops, but if it's not long enough then you'll get an extra first. This sentence has to be long enough.</p>
        </div>
    </section></main></body></html>"#;
    let result = extract(html, &Kw::new()).unwrap_or_default();
    assert!(!result.contains("SquishedThere"));
    assert!(!result.contains("SquishedWhere"));
}

#[test]
fn test_table_caption() {
    // Table caption is emitted as a header row above the table body.
    let result = table_md(
        "<table><caption>My Caption</caption><tr><td>a</td><td>b</td></tr></table>",
        Kw::zero(),
    )
    .unwrap();
    assert!(result.contains("My Caption"));
    // caption row appears before the body row
    assert!(result.find("My Caption").unwrap() < result.find("| a |").unwrap());
    // separator must span all columns (2), not just the caption cell (1)
    assert!(result.contains("|---|---|"));
}

#[test]
fn test_table_caption_image_kept() {
    // An image in a caption is emitted once, its caption text is not repeated.
    let words = "Article words go here and they are plentiful. ".repeat(3);
    let doc = format!(
        "<html><body><article><p>Intro {words}</p><table><caption><img src='https://e.com/c.png' alt='Chart'/> Caption words</caption><tr><td>Cell one {words}</td><td>Cell two</td></tr></table><p>Outro {words}</p></article></body></html>"
    );
    let result = extract(&doc, &Kw::new().images(true).markdown()).unwrap();
    assert_eq!(result.matches("![Chart](https://e.com/c.png)").count(), 1);
    assert_eq!(result.matches("Caption words").count(), 1);
}

#[test]
fn test_image_tail_not_duplicated() {
    // Regression: an image's tail text must not be emitted twice outside a table cell.
    let result = extract_doc(
        "<ul><li>a <img src='i.jpg' alt='A'/> b</li></ul>",
        true,
        &Kw::zero().images(true),
    )
    .unwrap();
    assert!(result.ends_with("a ![A](i.jpg) b"));
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
#[ignore = "phase 4"]
fn test_markdown_metadata_yaml_safe_phase_4_lines() {
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

#[test]
fn test_include_formatting_markdown() {
    // Markdown defaults to formatted, but an explicit include_formatting=False is honored (was
    // ignored for markdown).
    let doc =
        "<html><body><article><p>plain and <b>bold</b> text here.</p></article></body></html>";
    assert_eq!(
        extract(doc, &Kw::zero().markdown()).as_deref(),
        Some("plain and **bold** text here.")
    );
    assert_eq!(
        extract(doc, &Kw::zero().markdown().formatting(false)).as_deref(),
        Some("plain and bold text here.")
    );
    assert_eq!(
        extract(doc, &Kw::zero().formatting(true)).as_deref(),
        Some("plain and **bold** text here.")
    );
}

#[test]
fn test_extract_snippet() {
    // Line breaks before inline content, underline and <wbr> on intro-less snippets.
    for (id, body, kw, expected) in [
        (
            "item-leading-br",
            "<ul><li>\n  <br>tail after break</li><li>x</li></ul>",
            Kw::zero().markdown(),
            "- tail after break\n- x",
        ),
        (
            "cell-br-between-bold",
            "<table><tr><td><strong>A</strong><br>\n<strong>B</strong>, x</td></tr></table>",
            Kw::zero().markdown().tables(true),
            "| **A** **B**, x |",
        ),
        (
            "p-br-between-bold",
            "<p><strong>A</strong><br><strong>B</strong></p>",
            Kw::zero().markdown(),
            "**A**\n\n**B**",
        ),
        // underline has no markdown syntax (__ is bold), adjacent runs still merge
        (
            "underline-as-html",
            "<p><u>a</u><u>b</u> c</p>",
            Kw::zero().markdown(),
            "<u>ab</u> c",
        ),
        // the parser nests the rest of the paragraph inside <wbr>
        (
            "wbr-keeps-text",
            "<p>long<wbr>word and more</p>",
            Kw::zero(),
            "longword and more",
        ),
    ] {
        assert_eq!(
            extract_doc(body, false, &kw).as_deref(),
            Some(expected),
            "{id}"
        );
    }
}

#[test]
fn test_markdown_sup_sub_keep_boundary() {
    // sup/sub must keep a boundary, else 100<sup>2</sup> reads as 1002 (issue #889)
    let kw = Kw::zero().markdown();
    let sup = "<html><body><article><p>The layer has 100<sup>2</sup>=10000 nodes.</p></article></body></html>";
    let sub = "<html><body><article><p>Written 2011<sub>15ya</sub> in winter.</p></article></body></html>";
    assert_eq!(
        extract(sup, &kw).as_deref(),
        Some("The layer has 100<sup>2</sup>=10000 nodes.")
    );
    assert_eq!(
        extract(sub, &kw).as_deref(),
        Some("Written 2011<sub>15ya</sub> in winter.")
    );
    // flanking whitespace stays outside the tags, exactly as for the symmetric markers
    let spaced = "<html><body><article><p>x <sup> 2 </sup> y</p></article></body></html>";
    let bold = "<html><body><article><p>x <b> 2 </b> y</p></article></body></html>";
    assert_eq!(extract(spaced, &kw).as_deref(), Some("x  <sup>2</sup>  y"));
    assert_eq!(extract(bold, &kw).as_deref(), Some("x  **2**  y"));
}

#[test]
fn test_block_in_inline_wrapper_in_cell() {
    // A block wrapped in a link or bold inside a cell is kept.
    for (id, cell, links, expected) in [
        (
            "bold",
            "<b><div>Block in bold</div></b>",
            false,
            "| Block in bold | y |",
        ),
        (
            "link",
            r#"<a href="/x"><p>Block in link</p></a>"#,
            true,
            "| Block in link | y |",
        ),
        (
            "link-with-inline",
            r#"<a href="/x"><b>bold</b><p>after</p></a>"#,
            true,
            "| [**bold**](/x) after | y |",
        ),
    ] {
        let output = table_md(
            &format!("<table><tr><td>{cell}</td><td>y</td></tr></table>"),
            Kw::zero().links(links),
        )
        .unwrap();
        assert_eq!(output.lines().next_back(), Some(expected), "{id}");
    }
}

#[test]
fn test_markdown_empty_sup_sub_are_dropped() {
    // An empty sup/sub must be removed without taking the text after it (issue #889).
    // process_node() hands a textless element its tail as text, so without the removal in
    // convert_tags() the new marker would wrap the *following* words: a<sup>b</sup>
    let kw = Kw::zero().markdown();
    for tag in ["sup", "sub"] {
        let doc = format!("<html><body><article><p>a<{tag}></{tag}>b</p></article></body></html>");
        assert_eq!(extract(&doc, &kw).as_deref(), Some("ab"), "{tag}");
    }
    // a footnote marker whose only child is dropped must still not swallow the sentence
    let footnote = r#"<html><body><article><p>Fact<sup><img src="x.png"/></sup> follows here.</p></article></body></html>"#;
    assert_eq!(
        extract(footnote, &kw).as_deref(),
        Some("Fact follows here.")
    );
    // the tail survives in precision mode too, where CUT_EMPTY_ELEMS pruning would drop it
    let tailed = "<html><body><article><p>alpha<sup></sup>beta gamma.</p></article></body></html>";
    assert_eq!(extract(tailed, &kw).as_deref(), Some("alphabeta gamma."));
    assert_eq!(
        extract(tailed, &kw.focus(Focus::Precision)).as_deref(),
        Some("alphabeta gamma.")
    );
}

#[test]
fn test_extract_restored_subtree_not_extracted_twice() {
    // A later BODY_XPATH pass sees the restored subtree with its consumed paragraph marked done.
    let hidden = format!(
        "<div style='display:none'>{}</div>",
        "Hidden block sentence. ".repeat(30)
    );
    let page = format!(
        "<html><body><main id='content'><article><p>Lead paragraph of the story.</p>{hidden}</article><p>Outside paragraph one.</p></main></body></html>"
    );
    let result = extract(&page, &Kw::zero()).unwrap();
    assert_eq!(result.matches("Lead paragraph of the story.").count(), 1);
    assert!(result.contains("Outside paragraph one."));
}

#[test]
fn test_precision_recall() {
    // test precision- and recall-oriented settings; the test cases could be better
    let html = "<html><body><p>This here is the text.</p></body></html>";
    assert!(extract_tree(html, &Kw::zero().focus(Focus::Precision)).is_some());
    assert!(extract_tree(html, &Kw::zero().focus(Focus::Recall)).is_some());

    let html = r#"<html><body><div class="article-body"><div class="teaser-content"><p>This here is a teaser text.</p></div><div><p>This here is the text.</p></div></body></html>"#;
    assert!(
        extract_tree(html, &Kw::zero().focus(Focus::Recall))
            .unwrap()
            .contains("teaser text")
    );
    assert!(
        !extract_tree(html, &Kw::zero())
            .unwrap()
            .contains("teaser text")
    );
    assert!(
        !extract_tree(html, &Kw::zero().focus(Focus::Precision))
            .unwrap()
            .contains("teaser text")
    );

    let html = r#"<html><body><article><div><p><a href="test.html">1.</a><br/><a href="test2.html">2.</a></p></div></article></body></html>"#;
    let result = extract_tree(html, &Kw::zero().focus(Focus::Recall)).unwrap();
    assert!(!result.contains('1'));
    let result = extract_tree(html, &Kw::zero().focus(Focus::Precision)).unwrap();
    assert!(!result.contains('1'));

    let html = r#"<html><body><div class="article-body"><p>content</p><p class="link">Test</p></div></body></html>"#;
    let result = extract_tree(html, &Kw::zero().focus(Focus::Balanced)).unwrap();
    assert!(result.contains("content"));
    assert!(result.contains("Test"));
    let result = extract_tree(html, &Kw::zero().focus(Focus::Precision)).unwrap();
    assert!(result.contains("content"));
    assert!(!result.contains("Test"));

    let html =
        "<html><body><article><aside><p>Here is the text.</p></aside></article></body></html>";
    let result = extract_tree(html, &Kw::zero().focus(Focus::Balanced)).unwrap();
    assert_ne!(result, "Here is the text.");
    let result = extract_tree(html, &Kw::zero().focus(Focus::Recall)).unwrap();
    assert_eq!(result, "Here is the text.");

    // deviation: D-1, upstream runs this one with `fast=False` and asserts a non-empty result (the
    // fallback recovers the `small` text); oracle: extract(copy(my_document), favor_recall=True,
    // config=ZERO_CONFIG) with fast=True: ""
    let html = "<html><body><div><h2>Title</h2><small>Text.</small></div></body></html>";
    let result = extract_tree(html, &Kw::zero().focus(Focus::Recall)).unwrap();
    assert!(result.is_empty());

    let html = "<html><body><div><span>Text.</span></div></body></html>";
    assert_eq!(
        extract_tree(html, &Kw::zero().focus(Focus::Precision)).as_deref(),
        Some("")
    );
    assert_eq!(
        extract_tree(html, &Kw::zero().focus(Focus::Recall)).as_deref(),
        Some("Text.")
    );
}

#[test]
fn test_mixed_content_extraction() {
    // Test extraction from HTML with mixed content.
    let html =
        r#"<html><body><p>Text here</p><img src="img.jpg"/><video src="video.mp4"/></body></html>"#;
    // deviation: D-1, upstream runs this with `fast=False`; the value is the same
    assert_eq!(extract(html, &Kw::zero()).as_deref(), Some("Text here"));
}

#[test]
fn test_nonstd_html_entities() {
    // Test handling non-standard HTML entities.
    let html = "<html><body><p>Text &customentity; more text</p></body></html>";
    // deviation: D-1, upstream runs this with `fast=False`; the value is the same
    assert_eq!(
        extract(html, &Kw::zero()).as_deref(),
        Some("Text &customentity; more text")
    );
}

#[test]
fn test_large_doc_performance() {
    // Performance test on large HTML documents.
    let large_html = format!(
        "<html><body>{}</body></html>",
        "<p>Sample text</p>".repeat(10000)
    );
    let start = std::time::Instant::now();
    // deviation: D-1, upstream runs this with `fast=False`
    extract(&large_html, &Kw::zero());
    assert!(
        start.elapsed() < std::time::Duration::from_secs(5),
        "Large document performance issue"
    );
}
