use super::*;

/// Attribute pairs in source order.
type Attrs<'a> = &'a [(&'a str, &'a str)];

/// `<xml>` with one child per `(name, attributes, text)`, in the HTML namespace.
fn xml_with(children: &[(&str, Attrs<'_>, &str)]) -> (Tree, NodeId) {
    let mut tree = Tree::new(LocalName::from("xml"), Ns::Html, vec![]);
    let root = tree.root();
    for &(name, child_attrs, text) in children {
        let child = tree.add_child(root, LocalName::from(name), Ns::Html, attrs(child_attrs));
        tree.node_mut(child).text = Some(text.to_owned());
    }
    (tree, root)
}

/// `etree.tostring(convert_to_html(body), encoding="unicode")`, the XML method.
fn html_of(tree: &mut Tree, body: NodeId) -> Result<String, ExtractError> {
    let html = convert_to_html(tree, body, &Call::default())?;
    Ok(tree.tostring(html, false, false))
}

fn invalid_value() -> Result<String, ExtractError> {
    Err(ExtractError::Rejected(Rejection::InvalidValue))
}

#[test]
fn html_tag_mapping_inverts_rend_mapping() {
    // a dict inversion keeps the first key's place and the last key's value
    let mut inverted: Vec<(&str, &str)> = Vec::new();
    for &(tag, rend) in REND_TAG_MAPPING {
        match inverted.iter_mut().find(|(key, _)| *key == rend) {
            Some(slot) => slot.1 = tag,
            None => inverted.push((rend, tag)),
        }
    }
    assert_eq!(inverted, HTML_TAG_MAPPING);
    // oracle: list(HTML_TAG_MAPPING.items())
    assert_eq!(
        HTML_TAG_MAPPING,
        [
            ("#i", "i"),
            ("#b", "strong"),
            ("#u", "u"),
            ("#t", "var"),
            ("#sub", "sub"),
            ("#sup", "sup"),
        ]
    );
}

#[test]
fn html_conversion_rends_match_python() {
    // `h` and the Arabic-Indic digit three, which Python `int` reads as 3
    let arabic_three = "h\u{663}";
    let page = |inner: &str| Ok(format!("<html><body>{inner}</body></html>"));
    // `<xml><{name} {attributes}>a</{name}></xml>` through `convert_to_html`
    let cases: [(&str, Attrs<'_>, Result<String, ExtractError>); 13] = [
        // oracle: etree.tostring(convert_to_html(etree.fromstring("<xml><hi>a</hi></xml>")), encoding="unicode")
        ("hi", &[], page("<i>a</i>")),
        // oracle: the same call on <xml><hi rend="#b">a</hi></xml>
        ("hi", &[("rend", "#b")], page("<strong>a</strong>")),
        // oracle: the same call on <xml><hi rend="zz">a</hi></xml>, KeyError
        ("hi", &[("rend", "zz")], invalid_value()),
        // oracle: the same call on <xml><hi rend="">a</hi></xml>, KeyError: the `#i` default is for
        // an absent attribute only
        ("hi", &[("rend", "")], invalid_value()),
        // oracle: the same call on <xml><head rend="h9">a</head></xml>
        ("head", &[("rend", "h9")], page("<h9>a</h9>")),
        // oracle: the same call on <xml><head rend="h 2">a</head></xml>
        ("head", &[("rend", "h 2")], page("<h2>a</h2>")),
        // oracle: the same call on <xml><head rend="h" + chr(0x663) + ">a</head></xml>
        ("head", &[("rend", arabic_three)], page("<h3>a</h3>")),
        // oracle: the same call on <xml><head rend="hx">a</head></xml>, ValueError
        ("head", &[("rend", "hx")], invalid_value()),
        // oracle: the same call on <xml><head rend="">a</head></xml>, ValueError
        ("head", &[("rend", "")], invalid_value()),
        // oracle: the same call on <xml><head>a</head></xml>
        ("head", &[], page("<h3>a</h3>")),
        // oracle: the same call on <xml><cell role="head">a</cell></xml>
        ("cell", &[("role", "head")], page("<th>a</th>")),
        // oracle: the same call on <xml><cell role="data">a</cell></xml>
        ("cell", &[("role", "data")], page("<td>a</td>")),
        // oracle: the same call on <xml><cell>a</cell></xml>
        ("cell", &[], page("<td>a</td>")),
    ];
    for (name, child_attrs, expected) in cases {
        let (mut tree, root) = xml_with(&[(name, child_attrs, "a")]);
        assert_eq!(html_of(&mut tree, root), expected, "{name} {child_attrs:?}");
    }
}

#[test]
fn convert_to_html_attributes() {
    // the child holds the text `t`
    let cases: [(&str, Attrs<'_>, &str); 5] = [
        // oracle: the same call on <xml><ref class="c" target="u" x="1">t</ref></xml>
        (
            "ref",
            &[("class", "c"), ("target", "u"), ("x", "1")],
            r#"<a class="c" x="1" href="u">t</a>"#,
        ),
        // oracle: the same call on <xml><ref href="h" target="u">t</ref></xml>
        (
            "ref",
            &[("href", "h"), ("target", "u")],
            r#"<a href="u">t</a>"#,
        ),
        // oracle: the same call on <xml><ref>t</ref></xml>
        ("ref", &[], r#"<a href="">t</a>"#),
        // oracle: the same call on <xml><graphic src="s" alt="a" title="t" other="o">t</graphic></xml>:
        // an `img` keeps every attribute
        (
            "graphic",
            &[("src", "s"), ("alt", "a"), ("title", "t"), ("other", "o")],
            r#"<img src="s" alt="a" title="t" other="o">t</img>"#,
        ),
        // oracle: the same call on <xml><p class="k" id="n">t</p></xml>: an unconverted element
        (
            "p",
            &[("class", "k"), ("id", "n")],
            r#"<p class="k" id="n">t</p>"#,
        ),
    ];
    for (name, child_attrs, expected) in cases {
        let (mut tree, root) = xml_with(&[(name, child_attrs, "t")]);
        assert_eq!(
            html_of(&mut tree, root),
            Ok(format!("<html><body>{expected}</body></html>")),
            "{name} {child_attrs:?}"
        );
    }
}

#[test]
fn convert_to_html_drops_attributes_of_converted_elements() {
    // oracle: the same call on <xml><list rend="ol"><item id="i">x</item></list></xml>
    let mut tree = Tree::new(LocalName::from("xml"), Ns::Html, vec![]);
    let root = tree.root();
    let list = tree.add_child(
        root,
        LocalName::from("list"),
        Ns::Html,
        attrs(&[("rend", "ol")]),
    );
    let item = tree.add_child(
        list,
        LocalName::from("item"),
        Ns::Html,
        attrs(&[("id", "i")]),
    );
    tree.node_mut(item).text = Some("x".into());
    assert_eq!(
        html_of(&mut tree, root).as_deref(),
        Ok("<html><body><ul><li>x</li></ul></body></html>")
    );
}

#[test]
fn convert_to_html_renames_any_root_to_body() {
    // oracle: the same call on <list rend="ol"><item>x</item></list>: the root becomes `ul` and
    // loses its attribute, then is renamed `body`
    let mut tree = Tree::new(LocalName::from("list"), Ns::Html, attrs(&[("rend", "ol")]));
    let root = tree.root();
    let item = tree.add_child(root, LocalName::from("item"), Ns::Html, vec![]);
    tree.node_mut(item).text = Some("x".into());
    assert_eq!(
        html_of(&mut tree, root).as_deref(),
        Ok("<html><body><li>x</li></body></html>")
    );
    // oracle: the same call on <xml id="r"><p class="k" id="n">x</p></xml>: an unconverted root
    // keeps its attributes
    let (mut tree, root) = xml_with(&[("p", &[("class", "k"), ("id", "n")], "x")]);
    tree.set(root, "id", "r");
    assert_eq!(
        html_of(&mut tree, root).as_deref(),
        Ok(r#"<html><body id="r"><p class="k" id="n">x</p></body></html>"#)
    );
}

#[test]
fn build_html_output_meta() {
    let build = |meta: Option<&[(&str, &str)]>| {
        let mut tree = Tree::new(LocalName::from("xml"), Ns::Html, vec![]);
        let root = tree.root();
        let p = tree.add_child(root, LocalName::from("p"), Ns::Html, vec![]);
        tree.node_mut(p).text = Some("a & b".into());
        let list = tree.add_child(root, LocalName::from("list"), Ns::Html, vec![]);
        let item = tree.add_child(list, LocalName::from("item"), Ns::Html, vec![]);
        tree.node_mut(item).text = Some("x".into());
        build_html_output(&mut tree, root, meta, &Call::default())
    };
    // oracle: with BODY = "<xml><p>a &amp; b</p><list><item>x</item></list></xml>" and a Document
    // doc whose body is etree.fromstring(BODY): build_html_output(doc, False)
    assert_eq!(
        build(None).as_deref(),
        Ok(
            "<html>\n  <body>\n    <p>a &amp; b</p>\n    <ul>\n      <li>x</li>\n    </ul>\n  </body>\n</html>"
        )
    );
    // oracle: the same Document with no field set: build_html_output(doc, True)
    assert_eq!(
        build(Some(&[])).as_deref(),
        Ok(
            "<html>\n  <head/>\n  <body>\n    <p>a &amp; b</p>\n    <ul>\n      <li>x</li>\n    </ul>\n  </body>\n</html>"
        )
    );
    // oracle: the same Document with title="T" and author="A": build_html_output(doc, True)
    assert_eq!(
        build(Some(&[("title", "T"), ("author", "A")])).as_deref(),
        Ok(
            "<html>\n  <head>\n    <meta name=\"title\" content=\"T\"/>\n    <meta name=\"author\" content=\"A\"/>\n  </head>\n  <body>\n    <p>a &amp; b</p>\n    <ul>\n      <li>x</li>\n    </ul>\n  </body>\n</html>"
        )
    );
}

#[test]
fn build_html_output_stops_when_cancelled() {
    let token = crate::cancel::CancelToken::new();
    token.cancel();
    let (mut tree, root) = xml_with(&[("p", &[], "x")]);
    assert_eq!(
        build_html_output(&mut tree, root, None, &Call::default().cancel(&token)),
        Err(ExtractError::Cancelled)
    );
}

#[test]
fn tree_cleaning_marks_surviving_dropped_text() {
    let page = format!(
        "<html><head><title>t</title></head><body><form><aside><p>Short aside paragraph here.</p></aside><div>{}</div></form><script>{}</script></body></html>",
        "Form body sentence with several words in it. ".repeat(8),
        "var a = 1;".repeat(200),
    );
    let parse = |drop_script_text| {
        let limits = ParseLimits {
            drop_script_text,
            ..ParseLimits::default()
        };
        parse_document(&page, &limits, &Call::default()).unwrap()
    };
    let cleaned = |drop_script_text, recall| {
        let mut tree = parse(drop_script_text);
        let root = tree.root();
        let options = Extractor::new(
            Config::default(),
            Format::Python,
            false,
            recall,
            None,
            false,
            false,
            Default::default(),
            Default::default(),
        );
        let ctx = Ctx {
            options: Cow::Owned(options),
            call: &Call::default(),
        };
        tree_cleaning(&mut tree, root, &ctx).unwrap();
        tree.needs_script_text
    };
    assert!(cleaned(true, true));
    assert!(!cleaned(true, false));
    assert!(!cleaned(false, true));
}
