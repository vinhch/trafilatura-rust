#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Ports of upstream `tests/unit_tests.py`, case by case with the upstream names.

use std::collections::HashSet;

use trafilatura_rust::__internals::{
    BODY_XPATH, Config, DumpMode, Extractor, Focus, Format, HtmlInput, MANUALLY_CLEANED,
    MANUALLY_STRIPPED, OVERALL_DISCARD_XPATH, PRECISION_DISCARD_XPATH, ParseLimits, Selector, Tree,
    alt, delete_element, dump, load_html, parse_document, parse_fragment, prune_html, sanitize,
    textfilter, tree_cleaning, tree_cleaning_with, trim,
};
use trafilatura_rust::Call;

mod trees;
use trees::{ctx_for, lxml_fromstring, xml_tree};

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

/// A fresh `core.Extractor()`.
fn default_options() -> Extractor {
    extractor(Format::Python, false, false, None, (false, false), &[], &[])
}

/// The lines joined as the dump prints them, one `\n` after each.
fn dump_lines(lines: &[&str]) -> String {
    lines.iter().map(|line| format!("{line}\n")).collect()
}

#[test]
fn test_htmlprocessing() {
    // upstream's first line, `xml.xmltotxt(None, ...)`, is phase 3e
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
    // phase 3d, 3e and 3f: the rest of the test (`convert_tags`, `xmltotxt`, `extract`)

    // delete_element on an element without a parent leaves it unchanged
    let mut tree = xml_tree("<test/>");
    let root = tree.root();
    let before = dump(&tree, DumpMode::Exact);
    delete_element(&mut tree, root, true);
    assert_eq!(dump(&tree, DumpMode::Exact), before);
    // phase 3e: merge_with_parent
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
        let tree = load_html(HtmlInput::Str(html), &ParseLimits::default(), &call).unwrap();
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
    // phase 3f: extract line

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
}
