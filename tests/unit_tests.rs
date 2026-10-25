#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Ports of upstream `tests/unit_tests.py`, case by case with the upstream names.

use std::collections::HashSet;

use trafilatura_rust::__internals::{
    BODY_XPATH, Config, DEDUPE_SCAN_CAP, DumpMode, Extractor, Focus, Format, HtmlInput, LiveIter,
    MANUALLY_CLEANED, MANUALLY_STRIPPED, MAX_FILE_SIZE, MIN_DUPLICATE_LENGTH, NodeId,
    OVERALL_DISCARD_XPATH, PRECISION_DISCARD_XPATH, ParseLimits, Selector, TAG_CATALOG, Tree, alt,
    convert_tags, convert_to_html, delete_by_link_density, delete_element, dump, extract_content,
    handle_formatting, handle_image, handle_lists, handle_paragraphs, handle_quotes, handle_table,
    handle_textelem, handle_textnode, handle_titles, link_density_test, link_density_test_tables,
    load_html, md_link, merge_with_parent, parse_document, parse_fragment, process_node,
    prune_html, prune_unwanted_nodes, prune_unwanted_sections, recover_wild_text,
    remove_empty_elements, replace_element_text, sanitize, span, strip_double_tags, textfilter,
    tree_cleaning, tree_cleaning_with, trim, xml_to_text,
};
use trafilatura_rust::{Call, ExtractError};

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
    // phase 3f: the rest of the test (`extract`)

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
}

/// `handle_image(html.fromstring(html))`: the tree and the new element, `None` for no image.
fn image_of(html: &str) -> Option<(Tree, NodeId)> {
    let (mut tree, img) = lxml_fromstring(html);
    let image = handle_image(&mut tree, img, None).unwrap()?;
    Some((tree, image))
}

#[test]
fn test_images() {
    // `is_image_file` is ported with `utils`; `extract` comes with a later phase
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
    // `load_mock_page`, `check_tei` and `extract` are not ported yet: `extract` comes with phase 3f,
    // `check_tei` and the TEI output with phase 6
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
}

#[test]
fn test_input() {
    // `detect_encoding`, `is_dubious_html`, `extract` and `bare_extraction` come with later phases
    let options = default_options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
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
fn test_links() {
    // the `extract` assertions come with phase 3f
    let options = default_options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let mut tree = xml_tree("<ref/>");
    let root = tree.root();
    assert_eq!(handle_textelem(&mut tree, root, &[], &ctx), Ok(None));
    let (mut tree, link) = lxml_fromstring("<a href=\"testlink.html\">Test link text.</a>");
    assert!(handle_formatting(&mut tree, link, &ctx).is_some());
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

// upstream's `extract` block (a `del` directly in a table cell) is phase 3f
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

// The `extract` lines of upstream's test wait for phase 3f. The first assert uses
// `method="html"`, which writes the same as the XML method on this input.
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
}
