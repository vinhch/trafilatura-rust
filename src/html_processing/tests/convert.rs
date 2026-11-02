use super::*;

/// `html` parsed as a fragment; the tree and its first element.
fn fragment(html: &str) -> (Tree, NodeId) {
    let tree = parse_fragment(html, &ParseLimits::default(), &Call::default()).unwrap();
    let first = tree.children(tree.root()).next().unwrap();
    (tree, first)
}

/// `etree.tostring(element, with_tail=False)`: attributes in order, `<x/>` for an empty element.
fn xml(tree: &Tree, element: NodeId) -> String {
    let node = tree.node(element);
    let mut out = format!("<{}", node.name);
    for (key, value) in &node.attrs {
        out.push_str(&format!(" {key}=\"{value}\""));
    }
    if node.text.is_none() && tree.first_child(element).is_none() {
        out.push_str("/>");
        return out;
    }
    out.push('>');
    out.push_str(node.text.as_deref().unwrap_or(""));
    for child in tree.children(element) {
        out.push_str(&xml(tree, child));
        out.push_str(tree.node(child).tail.as_deref().unwrap_or(""));
    }
    out.push_str(&format!("</{}>", node.name));
    out
}

/// `html` parsed as a fragment, `convert_tags` run on its first element with the options set by
/// `set`; the serialization of that element.
fn converted(html: &str, set: impl FnOnce(&mut Extractor)) -> String {
    let (mut tree, root) = fragment(html);
    let mut options = options();
    set(&mut options);
    let call = Call::default();
    let ctx = Ctx {
        options: Cow::Borrowed(&options),
        call: &call,
    };
    convert_tags(&mut tree, root, &ctx, None).unwrap();
    xml(&tree, root)
}

#[test]
fn convert_lists_numbers_dd_dt() {
    let call = Call::default();
    // oracle: convert_lists(etree.fromstring("<dl><dt>a</dt><dd>b</dd><dt>c</dt></dl>"))
    let (mut tree, dl) = fragment("<dl><dt>a</dt><dd>b</dd><dt>c</dt></dl>");
    convert_lists(&mut tree, dl, &call).unwrap();
    assert_eq!(
        xml(&tree, dl),
        r#"<list rend="dl"><item rend="dt-1">a</item><item rend="dd-1">b</item><item rend="dt-2">c</item></list>"#
    );
    // oracle: convert_lists(etree.fromstring("<ul><li>a</li><li>b<ol><li>c</li></ol></li></ul>"))
    let (mut tree, ul) = fragment("<ul><li>a</li><li>b<ol><li>c</li></ol></li></ul>");
    convert_lists(&mut tree, ul, &call).unwrap();
    assert_eq!(
        xml(&tree, ul),
        r#"<list rend="ul"><item>a</item><item>b<ol><item>c</item></ol></item></list>"#
    );
}

#[test]
fn convert_quotes_detects_code() {
    let call = Call::default();
    // oracle: convert_quotes(etree.fromstring(source)) for each source
    let cases = [
        ("<pre><span>x</span></pre>", "<code><span>x</span></code>"),
        (
            r#"<pre><b></b><span class="hljs-k" data-x="1">x</span><i></i></pre>"#,
            "<code><b/><span>x</span><i/></code>",
        ),
        ("<pre>if x {</pre>", "<code>if x {</code>"),
        (r#"<pre>f("a")</pre>"#, r#"<code>f("a")</code>"#),
        ("<pre>f('a')</pre>", "<code>f('a')</code>"),
        ("<pre>a\n    b</pre>", "<code>a\n    b</code>"),
        ("<pre>plain text</pre>", "<quote>plain text</quote>"),
        (
            "<pre><span>a</span><span>b</span></pre>",
            "<quote><span>a</span><span>b</span></quote>",
        ),
        (
            r#"<pre><b><span class="other" id="s">a</span></b></pre>"#,
            r#"<quote><b><span class="other" id="s">a</span></b></quote>"#,
        ),
        // only a pre can be code
        ("<blockquote>{x}</blockquote>", "<quote>{x}</quote>"),
        ("<q>f('a')</q>", "<quote>f('a')</quote>"),
    ];
    for (source, expected) in cases {
        let (mut tree, elem) = fragment(source);
        convert_quotes(&mut tree, elem, &call).unwrap();
        assert_eq!(xml(&tree, elem), expected, "{source}");
    }
    // each indicator on its own marks code
    for indicator in CODE_INDICATORS {
        let (mut tree, elem) = fragment("<pre></pre>");
        tree.node_mut(elem).text = Some(format!("x{indicator}y"));
        convert_quotes(&mut tree, elem, &call).unwrap();
        assert_eq!(tree.tag(elem), Some("code"), "{indicator:?}");
    }
}

#[test]
fn convert_headings_keeps_only_rend() {
    // oracle: convert_headings(etree.fromstring('<h2 id="a" class="b">T</h2>'))
    let (mut tree, elem) = fragment(r#"<h2 id="a" class="b">T</h2>"#);
    convert_headings(&mut tree, elem);
    assert_eq!(xml(&tree, elem), r#"<head rend="h2">T</head>"#);
}

#[test]
fn convert_link_resolves_against_base() {
    // oracle: convert_link(etree.fromstring('<a href="{href}" class="c">t</a>'), base) for each pair
    let cases = [
        (
            "https://ex.com/a/",
            Some("../x"),
            r#"<ref target="https://ex.com/x">t</ref>"#,
        ),
        // a base that cannot be parsed: safe_relative_url is empty, the raw target stays
        ("http://[::1", Some("/a"), r#"<ref target="/a">t</ref>"#),
        // no base
        ("", Some("../x"), r#"<ref target="../x">t</ref>"#),
        // an empty href is falsy: no target
        ("https://ex.com/a/", Some(""), "<ref>t</ref>"),
        // no href: no target
        ("https://ex.com/a/", None, "<ref>t</ref>"),
    ];
    for (base, href, expected) in cases {
        let source = match href {
            Some(href) => format!(r#"<a href="{href}" class="c">t</a>"#),
            None => r#"<a class="c">t</a>"#.to_string(),
        };
        let (mut tree, elem) = fragment(&source);
        convert_link(&mut tree, elem, base);
        assert_eq!(xml(&tree, elem), expected, "{base:?} {href:?}");
    }
}

#[test]
fn faq_question_becomes_h3_head() {
    // oracle: convert_tags(html.fromstring(source), Extractor(formatting=True)); the class test is
    // a substring test, so `xschema-faq-questiony` matches too
    let source = r#"<div><p>q <strong class="a schema-faq-question b" id="x">Q?</strong> <strong class="plain">s</strong> <strong class="xschema-faq-questiony">t</strong></p></div>"#;
    assert_eq!(
        converted(source, |options| options.formatting = true),
        r##"<div><p>q <head rend="h3">Q?</head> <hi rend="#b">s</hi> <head rend="h3">t</head></p></div>"##
    );
}

#[test]
fn empty_sub_sup_deleted_tail_kept() {
    // oracle: convert_tags(html.fromstring(source), Extractor(formatting=True)); a sub with a
    // blank text or a child is not empty
    let source =
        "<div><p>a<sup></sup>b<sub></sub>c<sup>d</sup><sub> </sub>e<sub><i>x</i></sub></p></div>";
    assert_eq!(
        converted(source, |options| options.formatting = true),
        r##"<div><p>abc<hi rend="#sup">d</hi><hi rend="#sub"> </hi>e<hi rend="#sub"><hi rend="#i">x</hi></hi></p></div>"##
    );
}

#[test]
fn formatting_off_strips_rend_tags() {
    // oracle: convert_tags(html.fromstring(source), Extractor(formatting=False))
    let source = "<div><p>a <b>x</b><i>y</i><sub>s</sub> z<kbd>k</kbd></p></div>";
    assert_eq!(converted(source, |_| {}), "<div><p>a xys zk</p></div>");
}

#[test]
fn image_only_link_removed() {
    // oracle: convert_tags(html.fromstring(source), Extractor(images=True, links=True)); the
    // graphics follow the tail of the link, which joins the text before it
    let set = |options: &mut Extractor| {
        options.images = true;
        options.links = true;
    };
    let source = r#"<div><p>t <a href="/x"><img src="a.jpg"/><img src="b.jpg"/></a> u</p></div>"#;
    assert_eq!(
        converted(source, set),
        r#"<div><p>t  u<graphic src="a.jpg"/><graphic src="b.jpg"/></p></div>"#
    );
    // a link with text stays
    let source = r#"<div><p>t <a href="/x">w<img src="a.jpg"/></a> u</p></div>"#;
    assert_eq!(
        converted(source, set),
        r#"<div><p>t <ref target="/x">w</ref> u<graphic src="a.jpg"/></p></div>"#
    );
}

#[test]
fn convert_tags_stops_when_cancelled() {
    let (mut tree, root) = fragment("<div><p><a>a</a></p></div>");
    let token = crate::CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    let options = options();
    let ctx = Ctx {
        options: Cow::Borrowed(&options),
        call: &call,
    };
    assert_eq!(
        convert_tags(&mut tree, root, &ctx, None),
        Err(ExtractError::Cancelled)
    );
}

#[test]
fn convert_tags_checks_call_between_walks() {
    // no element matches any walk, so no per-element tick runs
    let (mut tree, root) = fragment("<div><p>text</p></div>");
    let token = crate::CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    let options = options();
    let ctx = Ctx {
        options: Cow::Borrowed(&options),
        call: &call,
    };
    assert_eq!(
        convert_tags(&mut tree, root, &ctx, None),
        Err(ExtractError::Cancelled)
    );
}

#[test]
fn convert_line_breaks_renames_and_keeps_attributes() {
    // oracle: convert_line_breaks(etree.fromstring(source)) for each source
    for (source, expected) in [
        ("<br>", "<lb/>"),
        (r#"<hr class="c">"#, r#"<lb class="c"/>"#),
    ] {
        let (mut tree, elem) = fragment(source);
        convert_line_breaks(&mut tree, elem);
        assert_eq!(xml(&tree, elem), expected, "{source}");
    }
}

#[test]
fn convert_deletions_sets_overstrike() {
    // oracle: convert_deletions(etree.fromstring('<{tag} class="c">x</{tag}>')) for s, strike, del
    for tag in ["s", "strike", "del"] {
        let (mut tree, elem) = fragment(&format!(r#"<{tag} class="c">x</{tag}>"#));
        convert_deletions(&mut tree, elem);
        assert_eq!(
            xml(&tree, elem),
            r#"<del class="c" rend="overstrike">x</del>"#,
            "{tag}"
        );
    }
}

#[test]
fn convert_details_renames_summaries() {
    // oracle: convert_details(etree.fromstring(source)); the nested details stays, its summary changes
    let source = "<details><summary>a</summary>b<details><summary>c</summary>d</details></details>";
    let (mut tree, elem) = fragment(source);
    convert_details(&mut tree, elem, &Call::default()).unwrap();
    assert_eq!(
        xml(&tree, elem),
        "<div><head>a</head>b<details><head>c</head>d</details></div>"
    );
}

#[test]
fn convert_tags_dispatches_every_conversion() {
    // oracle: convert_tags(html.fromstring(source), Extractor())
    let source = concat!(
        r#"<div><h1 id="a">1</h1><h2>2</h2><h3>3</h3><h4>4</h4><h5>5</h5><h6>6</h6>"#,
        "a<br>b<hr>c<blockquote>bq</blockquote><pre>pre</pre><q>q</q><s>s</s><strike>st</strike>",
        "<del>d</del><details><summary>sum</summary>det</details>",
        "<ul><li>a<ol><li>b</li></ol></li></ul><dl><dt>t</dt><dd>d</dd></dl></div>",
    );
    let expected = concat!(
        r#"<div><head rend="h1">1</head><head rend="h2">2</head><head rend="h3">3</head>"#,
        r#"<head rend="h4">4</head><head rend="h5">5</head><head rend="h6">6</head>"#,
        "a<lb/>b<lb/>c<quote>bq</quote><quote>pre</quote><quote>q</quote>",
        r#"<del rend="overstrike">s</del><del rend="overstrike">st</del><del rend="overstrike">d</del>"#,
        "<div><head>sum</head>det</div>",
        r#"<list rend="ul"><item>a<list rend="ol"><item>b</item></list></item></list>"#,
        r#"<list rend="dl"><item rend="dt-1">t</item><item rend="dd-1">d</item></list></div>"#,
    );
    assert_eq!(converted(source, |_| {}), expected);
}

#[test]
fn every_conversion_tag_changes_under_convert_tags() {
    // a tag listed in CONVERSION_TAGS without an arm in `convert` would stay as it is
    for &tag in CONVERSION_TAGS {
        let source = if matches!(tag, "br" | "hr") {
            format!("<div><{tag}></div>")
        } else {
            format!("<div><{tag}>x</{tag}></div>")
        };
        let (mut tree, root) = fragment(&source);
        let before = xml(&tree, root);
        let options = options();
        let call = Call::default();
        let ctx = Ctx {
            options: Cow::Borrowed(&options),
            call: &call,
        };
        convert_tags(&mut tree, root, &ctx, None).unwrap();
        // `del` keeps its name and gains `rend`, so compare the whole element
        assert_ne!(xml(&tree, root), before, "{tag} was not converted");
    }
}

#[test]
fn convert_tags_resolves_links_against_the_page_url() {
    // oracle: convert_tags(html.fromstring(source), Extractor(links=True), url) for each url; the base
    // is the scheme and host (and port) of the page, its path, query and fragment do not count
    let source = concat!(
        r#"<div><p><a href="../x" class="k">1</a><a href="/y">2</a><a href="z">3</a>"#,
        r#"<a href="//cdn.x/a">4</a><a href="">5</a><a>6</a></p></div>"#,
    );
    let convert = |url: Option<&str>, links: bool| {
        let (mut tree, root) = fragment(source);
        let mut options = options();
        options.links = links;
        let call = Call::default();
        let ctx = Ctx {
            options: Cow::Borrowed(&options),
            call: &call,
        };
        convert_tags(&mut tree, root, &ctx, url).unwrap();
        xml(&tree, root)
    };
    let with_base = |host: &str, cdn: &str| {
        format!(
            concat!(
                r#"<div><p><ref target="{0}/x">1</ref><ref target="{0}/y">2</ref>"#,
                r#"<ref target="{0}/z">3</ref><ref target="{1}">4</ref><ref>5</ref><ref>6</ref></p></div>"#
            ),
            host, cdn
        )
    };
    assert_eq!(
        convert(Some("https://ex.com/a/b.html"), true),
        with_base("https://ex.com", "http://cdn.x/a")
    );
    assert_eq!(
        convert(Some("https://ex.com:8080/p/q.html?a=1#frag"), true),
        with_base("https://ex.com:8080", "http://cdn.x/a")
    );
    // no usable base (an unparsable url, an empty url, none): targets stay as written
    let raw = concat!(
        r#"<div><p><ref target="../x">1</ref><ref target="/y">2</ref><ref target="z">3</ref>"#,
        r#"<ref target="//cdn.x/a">4</ref><ref>5</ref><ref>6</ref></p></div>"#,
    );
    for url in [Some("http://[::1"), Some(""), None] {
        assert_eq!(convert(url, true), raw, "{url:?}");
    }
    // oracle: convert_tags(html.fromstring('<div><p><a href="../x">1</a></p></div>'),
    // Extractor(links=False), "https://ex.com/a/b.html"): the link is kept as ref, untouched
    let (mut tree, root) = fragment(r#"<div><p><a href="../x">1</a></p></div>"#);
    let options = options();
    let call = Call::default();
    let ctx = Ctx {
        options: Cow::Borrowed(&options),
        call: &call,
    };
    convert_tags(&mut tree, root, &ctx, Some("https://ex.com/a/b.html")).unwrap();
    assert_eq!(
        xml(&tree, root),
        r#"<div><p><ref href="../x">1</ref></p></div>"#
    );
}

#[test]
fn formatting_maps_every_rend_tag() {
    // oracle: convert_tags(html.fromstring(source), Extractor(formatting=True)); attributes are dropped
    let source = concat!(
        r#"<div><p><em class="x">1</em><i>2</i><b id="y">3</b><strong>4</strong><u>5</u>"#,
        "<kbd>6</kbd><samp>7</samp><tt>8</tt><var>9</var><sub>a</sub><sup>b</sup></p></div>",
    );
    assert_eq!(
        converted(source, |options| options.formatting = true),
        concat!(
            r##"<div><p><hi rend="#i">1</hi><hi rend="#i">2</hi><hi rend="#b">3</hi>"##,
            r##"<hi rend="#b">4</hi><hi rend="#u">5</hi><hi rend="#t">6</hi><hi rend="#t">7</hi>"##,
            r##"<hi rend="#t">8</hi><hi rend="#t">9</hi><hi rend="#sub">a</hi><hi rend="#sup">b</hi></p></div>"##,
        )
    );
}
