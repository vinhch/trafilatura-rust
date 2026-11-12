#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Ports of upstream `tests/unit_tests.py`, case by case with the upstream names.
//!
//! `extract` runs through the public API (`Kw` holds the keyword arguments); a test that passes an
//! `html.fromstring` tree uses the tree entry point (`extract_tree`) over the libxml2 tree. The
//! criterion of every assertion is the upstream result with `fast=True` forced, on the libxml2 tree:
//! an assertion that holds is ported as it is, one that does not asserts that result with a
//! `deviation: D-1` comment (the slow path of `fast=False` is not ported).
//! Every explicit `fast=False` call has a D-1 comment, even when the
//! value is the same. Assertions that need the XML or JSON output are in separate tests, named for
//! the upstream test they split; the whole tests of the XML, XML-TEI, JSON and CSV output are in
//! `xml_output.rs`.
//!
//! Not ported, and why:
//! - `test_deprecations` (Python-only parameters), `test_incompatible_options` (a `Focus` makes the
//!   combination unrepresentable, and the crate does not log), `test_settings_element_lists`
//!   (mutates `MANUALLY_CLEANED` in place; the crate's lists are constants, D-8).
//! - The slow path (D-1, Phase 11): `test_sanitize_tree_*`, `test_readability_*`, `test_external`,
//!   `test_is_probably_readerable`, `test_prefer_readability_rejects_raw_json`,
//!   `test_compare_extraction_justext_*`, `test_recall_escalation_justext`, and the justext and
//!   readability lines of `test_extraction_options`.
//! - Python objects and caches: `test_extractor_parity` (compares Python `Extractor` attributes;
//!   `Options::build` is the one translation), `test_reset_caches` and the cache lines of
//!   `test_trim` (the crate keeps no caches, `docs/architecture.md` no process-global state),
//!   `test_document_isolation` (Python object identity; owned `Document` fields cannot alias),
//!   `test_python_output` (`as_dict` key count; the crate has no `as_dict`, and the parity metadata
//!   comparison checks the same keys).
//! - Language detection (OI-4): `test_wrong_language_discarded`, `test_lang_detection`.
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
    strip_double_tags, textfilter, tree_cleaning, tree_cleaning_with, trim, xml_to_csv,
    xml_to_text, yaml_scalar,
};
use trafilatura_rust::{
    Call, Document, ExtractError, Format as ApiFormat, Input, Options, Rejection,
};

#[allow(dead_code)]
#[path = "../common/mod.rs"]
mod common;
#[allow(dead_code)]
#[path = "../trees/mod.rs"]
mod trees;
use trees::{ctx_for, lxml_fromstring, xml_tree};

mod basics;
mod config_html;
mod dedup_recall;
mod htmlprocessing;
mod markdown_math;
mod metadata_links_tei;
mod options_discard;
mod serializer;
mod tables;
mod xml_output;

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
    only_with_metadata: bool,
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
            only_with_metadata: false,
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

    fn only_with_metadata(mut self, only_with_metadata: bool) -> Self {
        self.only_with_metadata = only_with_metadata;
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
            .config(self.config.clone())
            .only_with_metadata(self.only_with_metadata);
        match self.formatting {
            Some(formatting) => options.formatting(formatting),
            None => options,
        }
    }

    /// The same settings as the crate's internal `Extractor`, for the tree entry point.
    fn settings(&self) -> Extractor {
        self.settings_as(settings_format(self.format), false)
    }

    /// The internal settings of `format`, with the metadata flag as the constructor takes it.
    fn settings_as(&self, format: Format, with_metadata: bool) -> Extractor {
        let mut settings = Extractor::new(
            self.config.clone(),
            format,
            self.focus == Focus::Precision,
            self.focus == Focus::Recall,
            self.formatting,
            with_metadata,
            self.only_with_metadata,
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

/// `extract(html.fromstring(html), **kw)`: a caller's tree, as lxml's fromstring builds it.
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

// The XML and XML-TEI entry points.

/// `extract(html, output_format="xml", **kw)`.
fn extract_xml(html: &str, kw: &Kw) -> Option<String> {
    xml_output(Source::Input(HtmlInput::Str(html)), kw)
}

/// `extract(html.fromstring(html), output_format="xml", **kw)`.
fn extract_xml_tree(html: &str, kw: &Kw) -> Option<String> {
    let (tree, root) = lxml_fromstring(html);
    xml_output(Source::Tree(&tree, root), kw)
}

/// The XML output of `source` with the settings of `kw`.
fn xml_output(source: Source<'_>, kw: &Kw) -> Option<String> {
    let mut settings = kw.settings();
    settings.format = Format::Xml;
    let call = Call::default();
    let document = internal_extraction(
        source,
        None,
        &ctx_for(&settings, &call),
        &ParseLimits::default(),
    );
    or_none(document).map(|document| document.text.unwrap_or_default())
}

/// `extract(html.fromstring(html), output_format="xmltei", **kw)`.
fn extract_xmltei_tree(html: &str, kw: &Kw) -> Option<String> {
    let (tree, root) = lxml_fromstring(html);
    xmltei_output(Source::Tree(&tree, root), kw, None)
}

/// `extract(html, url, record_id=..., output_format="xmltei")`, with the default config.
fn extract_xmltei_with_id(html: &str, url: &str, record_id: &str) -> Option<String> {
    xmltei_output(
        Source::Input(HtmlInput::Str(html)),
        &Kw::new().url(url),
        Some(record_id),
    )
}

/// The XML-TEI output of `source` with the settings of `kw`.
fn xmltei_output(source: Source<'_>, kw: &Kw, record_id: Option<&str>) -> Option<String> {
    internal_document(source, kw, Format::XmlTei, false, record_id)
        .map(|document| document.text.unwrap_or_default())
}

/// The document of `source` in the internal `format` with the settings of `kw`. The XML-TEI format
/// forces the metadata in the constructor, so `with_metadata` only matters for the other formats.
fn internal_document(
    source: Source<'_>,
    kw: &Kw,
    format: Format,
    with_metadata: bool,
    record_id: Option<&str>,
) -> Option<Document> {
    let settings = kw.settings_as(format, with_metadata);
    let call = Call::default();
    or_none(internal_extraction(
        source,
        record_id,
        &ctx_for(&settings, &call),
        &ParseLimits::default(),
    ))
}

/// `extract(html, with_metadata=True, record_id=..., **kw)` in the format of `kw`.
fn extract_with_metadata(html: &str, kw: &Kw, record_id: Option<&str>) -> Option<String> {
    or_none(
        kw.options()
            .with_metadata(true)
            .build()
            .extract(&input_of(html, kw, record_id), &Call::default()),
    )
}

/// The input of `html` with the URL of `kw` and the record id, each when set.
fn input_of<'a>(html: &'a str, kw: &'a Kw, record_id: Option<&'a str>) -> Input<'a> {
    let input = Input::text(html);
    let input = match kw.url.as_deref() {
        Some(url) => input.url(url),
        None => input,
    };
    match record_id {
        Some(id) => input.record_id(id),
        None => input,
    }
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

/// `_md_inline(body, **kw)`.
fn md_inline(body: &str, kw: Kw) -> String {
    extract_doc(body, true, &kw.markdown().formatting(true)).unwrap_or_default()
}

/// The file `name` of upstream's `tests/resources`, read from the checkout; `None` (with a note)
/// when the checkout is absent, as `parity/` does; with `PARITY_REQUIRE_FIXTURES` set an absent
/// checkout panics instead.
fn resource(name: &str) -> Option<Vec<u8>> {
    let Some(root) = common::upstream_root() else {
        assert!(
            !common::require_fixtures(),
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

/// `.//span`
fn spans(tree: &Tree, context: NodeId, _call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    Ok(tree.iterdescendants(context, &["span"]).collect())
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
