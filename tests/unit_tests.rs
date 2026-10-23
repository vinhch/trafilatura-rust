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
    MANUALLY_STRIPPED, NodeId, OVERALL_DISCARD_XPATH, PRECISION_DISCARD_XPATH, ParseLimits,
    Selector, Tree, alt, convert_tags, delete_by_link_density, delete_element, dump,
    handle_textnode, link_density_test, link_density_test_tables, load_html, parse_document,
    parse_fragment, process_node, prune_html, prune_unwanted_nodes, sanitize, textfilter,
    tree_cleaning, tree_cleaning_with, trim,
};
use trafilatura_rust::{Call, ExtractError};

mod trees;
use trees::{ctx_for, lxml_fromstring, tostring, xml_tree};

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

/// `etree.fromstring(source)[0]`: the tree and the first child of its root.
fn first_child_of(source: &str) -> (Tree, NodeId) {
    let tree = xml_tree(source);
    let child = tree.children(tree.root()).next().unwrap();
    (tree, child)
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
    // phase 3d, 3e and 3f: the rest of the test (`xmltotxt`, `extract`)

    // delete_element on an element without a parent leaves it unchanged
    let mut tree = xml_tree("<test/>");
    let root = tree.root();
    let before = dump(&tree, DumpMode::Exact);
    delete_element(&mut tree, root, true);
    assert_eq!(dump(&tree, DumpMode::Exact), before);
    // phase 3e: merge_with_parent

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
fn tostring_matches_lxml() {
    // Each expected string is `etree.tostring(e, encoding="unicode")` from the oracle.
    // oracle: etree.Element("x")
    let tree = xml_tree("<x/>");
    assert_eq!(tostring(&tree, tree.root(), true), "<x/>");

    // oracle: e = etree.Element("x"); e.text = ""
    let mut tree = xml_tree("<x/>");
    let root = tree.root();
    tree.node_mut(root).text = Some(String::new());
    assert_eq!(tostring(&tree, root, true), "<x></x>");

    // oracle: e = etree.Element("x"); e.set("b", "1"); e.set("a", "2")
    let mut tree = xml_tree("<x/>");
    tree.set(root, "b", "1");
    tree.set(root, "a", "2");
    assert_eq!(tostring(&tree, root, true), r#"<x b="1" a="2"/>"#);

    // oracle: x.text = "a & b < c > d"; c = etree.SubElement(x, "c"); c.tail = "e & f < g > h"
    //   (etree.tostring(c, ...), and with with_tail=False)
    let tree = xml_tree("<x>a &amp; b &lt; c &gt; d<c/>e &amp; f &lt; g &gt; h</x>");
    let root = tree.root();
    let c = tree.first_child(root).unwrap();
    assert_eq!(
        tostring(&tree, root, true),
        "<x>a &amp; b &lt; c &gt; d<c/>e &amp; f &lt; g &gt; h</x>"
    );
    assert_eq!(tostring(&tree, c, true), "<c/>e &amp; f &lt; g &gt; h");
    assert_eq!(tostring(&tree, c, false), "<c/>");

    // oracle: e.set("v", 'q"\n\t\r<&>')
    let mut tree = xml_tree("<x/>");
    let root = tree.root();
    tree.set(root, "v", "q\"\n\t\r<&>");
    assert_eq!(
        tostring(&tree, root, true),
        r#"<x v="q&quot;&#10;&#9;&#13;&lt;&amp;&gt;"/>"#
    );

    // oracle: e = etree.Element("x"); e.text = "a\rb"
    let mut tree = xml_tree("<x/>");
    let root = tree.root();
    tree.node_mut(root).text = Some("a\rb".to_string());
    assert_eq!(tostring(&tree, root, true), "<x>a&#13;b</x>");

    // oracle: x.text = "t"; c = SubElement(x, "c"); c.text = ""; c.tail = ""; d = SubElement(x, "d"); d.tail = "z"
    let mut tree = xml_tree("<x>t<c/><d/>z</x>");
    let root = tree.root();
    let (c, d) = (
        tree.child_at(root, 0).unwrap(),
        tree.child_at(root, 1).unwrap(),
    );
    tree.node_mut(c).text = Some(String::new());
    tree.node_mut(c).tail = Some(String::new());
    tree.node_mut(d).tail = Some("z".to_string());
    assert_eq!(tostring(&tree, root, true), "<x>t<c></c><d/>z</x>");
}
