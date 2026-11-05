use super::*;

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

const COLSPAN_CONTENT_BASE: &str = "<tr><td>a</td><td>b</td><td>c</td></tr>";

const COLSPAN_CONTENT_ROW: &str = "<tr><td>a</td><td colspan='2'><p>b</p><p>c</p></td></tr>";

#[test]
fn test_extraction_options() {
    // The justext and readability lines of upstream's test are the slow path (D-1), not ported.
    let my_html = r#"<html><head><meta http-equiv="content-language" content="EN"/></head><body><div="article-body"><p>Text.<!-- comment --><?php echo "This is a PHP processing instruction"; ?></p></div></body></html>"#;
    // oracle: extract(my_html, output_format="python") raises ValueError
    assert_eq!(
        Kw::new()
            .format(ApiFormat::Python)
            .options()
            .build()
            .extract(&Input::text(my_html), &Call::default()),
        Err(ExtractError::Rejected(Rejection::InvalidValue))
    );
    assert!(extract(my_html, &Kw::new().config(new_config())).is_none());
    assert!(extract(my_html, &Kw::zero()).is_some());
    // oracle: extract(my_html, target_language="de", config=ZERO_CONFIG) is None. Upstream's
    // fast=True line gives the same value, so this one assertion covers both.
    assert!(
        or_none(
            Options::new()
                .target_language("de")
                .config(zero_config())
                .build()
                .extract(&Input::text(my_html), &Call::default())
        )
        .is_none()
    );

    // oracle: bare_extraction(my_html, with_metadata=True).date, with the config of each call
    let page = format!(
        "<html><head/><body>{}<p>Posted on 1st Dec 2019<.</p></body></html>",
        "<p>ABC def ghi jkl.</p>".repeat(1000)
    );
    let date_of = |options: Options| {
        options
            .build()
            .bare_extraction(&Input::text(&page), &Call::default())
            .unwrap()
            .date
    };
    assert!(date_of(Options::new().config(zero_config()).with_metadata(true)).is_some());
    assert_eq!(
        date_of(Options::new().config(new_config()).with_metadata(true)),
        None
    );
    assert_eq!(
        date_of(Options::new().config(new_config()).with_metadata(false)),
        None
    );
}

// The XML lines of `test_extraction_options`.
#[test]
#[ignore = "phase 6"]
fn test_extraction_options_phase_6_lines() {
    let my_html = r#"<html><head><meta http-equiv="content-language" content="EN"/></head><body><div="article-body"><p>Text.<!-- comment --><?php echo "This is a PHP processing instruction"; ?></p></div></body></html>"#;
    // oracle: extract(my_html, only_with_metadata=False, output_format="xml", config=ZERO_CONFIG)
    assert!(extract_xml(my_html, &Kw::zero()).is_some());
    // oracle: extract(my_html, only_with_metadata=True, output_format="xml", config=ZERO_CONFIG)
    assert!(extract_xml(my_html, &Kw::zero().only_with_metadata(true)).is_none());
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
fn test_url_blacklist() {
    // oracle: extract(doc, config=ZERO_CONFIG) is not None; with url_blacklist={"https://example.org/page"} it is None
    let doc = r#"<html><head><link rel="canonical" href="https://example.org/page"/></head><body><article><p>Some real article body text here.</p></article></body></html>"#;
    assert!(extract(doc, &Kw::zero()).is_some());
    let blocked = Kw::zero()
        .options()
        .url_blacklist(["https://example.org/page"]);
    assert!(or_none(blocked.build().extract(&Input::text(doc), &Call::default())).is_none());
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
    assert!(!defaults.links && !defaults.images);
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

#[test]
fn test_cell_context_follows_real_ancestry() {
    // Cell handling must follow real ancestry, not 'a cell exists somewhere' (#767).
    let source = "<body><table><row><cell><p>inside</p></cell></row></table><p>outside</p></body>";
    assert_eq!(xmltotxt(source, true), "| inside | \n\noutside\n");
    assert_eq!(xmltotxt(source, false), "| inside | \noutside");
}

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
