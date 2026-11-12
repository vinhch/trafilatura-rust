use super::*;

/// Runs `handle_textnode` on the `tag` child of `<p>` given `text` and `tail`; returns whether it
/// is kept, then the child's tag, text and tail afterwards.
fn textnode_case(
    tag: &str,
    attrs_list: &[(&str, &str)],
    text: Option<&str>,
    tail: Option<&str>,
    comments_fix: bool,
    preserve_spaces: bool,
) -> (bool, String, Option<String>, Option<String>) {
    let name = |n: &str| LocalName::from(n);
    let mut tree = Tree::new(name("p"), Ns::Html, vec![]);
    let child = tree.add_child(tree.root(), name(tag), Ns::Html, attrs(attrs_list));
    tree.node_mut(child).text = text.map(str::to_string);
    tree.node_mut(child).tail = tail.map(str::to_string);
    let options = options();
    let call = Call::default();
    let ctx = Ctx {
        options: Cow::Borrowed(&options),
        call: &call,
    };
    let kept = handle_textnode(&mut tree, child, &ctx, comments_fix, preserve_spaces).is_some();
    let node = tree.node(child);
    (
        kept,
        node.name.to_string(),
        node.text.clone(),
        node.tail.clone(),
    )
}

#[test]
fn textnode_empty_tail_stays_empty_string() {
    // oracle: run('<p><x/> </p>'): handle_textnode(n, Extractor()) is None, n.text None, n.tail ''
    let (kept, _, text, tail) = textnode_case("x", &[], None, Some(" "), true, false);
    assert!(!kept);
    assert_eq!(text, None);
    assert_eq!(tail.as_deref(), Some(""));
    // oracle: run('<p><x/> a </p>'): kept, n.text 'a', n.tail ''
    let (kept, _, text, tail) = textnode_case("x", &[], None, Some(" a "), true, false);
    assert!(kept);
    assert_eq!(text.as_deref(), Some("a"));
    assert_eq!(tail.as_deref(), Some(""));
}

#[test]
fn textnode_graphic_with_src_is_kept() {
    // oracle: run('<p><graphic src="a.jpg"/></p>'): kept, n.text None, n.tail None
    let (kept, _, text, tail) =
        textnode_case("graphic", &[("src", "a.jpg")], None, None, true, false);
    assert!(kept);
    assert_eq!((text, tail), (None, None));
    // oracle: run('<p><graphic/>t</p>'): kept, n.text 't', n.tail ''
    let (kept, _, text, tail) = textnode_case("graphic", &[], None, Some("t"), true, false);
    assert!(kept);
    assert_eq!(text.as_deref(), Some("t"));
    assert_eq!(tail.as_deref(), Some(""));
}

#[test]
fn textnode_done_is_dropped() {
    // oracle: run('<p><done>x</done></p>'): None, n.text 'x', n.tail None, untouched
    let (kept, _, text, tail) = textnode_case("done", &[], Some("x"), None, true, false);
    assert!(!kept);
    assert_eq!(text.as_deref(), Some("x"));
    assert_eq!(tail, None);
}

#[test]
fn textnode_lb_bypass() {
    // oracle: run('<p><lb/> a   b </p>', comments_fix=False): kept, n.text None, n.tail 'a b'
    let (kept, tag, text, tail) = textnode_case("lb", &[], None, Some(" a   b "), false, false);
    assert!(kept);
    assert_eq!(tag, "lb");
    assert_eq!(text, None);
    assert_eq!(tail.as_deref(), Some("a b"));
    // oracle: run('<p><lb/> a   b </p>', comments_fix=False, preserve_spaces=True): n.tail ' a   b '
    let (kept, _, text, tail) = textnode_case("lb", &[], None, Some(" a   b "), false, true);
    assert!(kept);
    assert_eq!(text, None);
    assert_eq!(tail.as_deref(), Some(" a   b "));
    // oracle: run('<p><lb/>   </p>', comments_fix=False): kept, n.tail None
    let (kept, _, _, tail) = textnode_case("lb", &[], None, Some("   "), false, false);
    assert!(kept);
    assert_eq!(tail, None);
}

#[test]
fn textnode_lb_becomes_p_with_comments_fix() {
    // oracle: run('<p><lb/>tail</p>', comments_fix=True): kept, n.tag 'p', n.text 'tail', n.tail ''
    let (kept, tag, text, tail) = textnode_case("lb", &[], None, Some("tail"), true, false);
    assert!(kept);
    assert_eq!(tag, "p");
    assert_eq!(text.as_deref(), Some("tail"));
    assert_eq!(tail.as_deref(), Some(""));
    // oracle: run('<p><lb/>tail</p>', comments_fix=True, preserve_spaces=True): same
    let (kept, tag, text, tail) = textnode_case("lb", &[], None, Some("tail"), true, true);
    assert!(kept);
    assert_eq!(tag, "p");
    assert_eq!(text.as_deref(), Some("tail"));
    assert_eq!(tail.as_deref(), Some(""));
}

/// Runs `handle_textnode` (`light` false) or `process_node` (`light` true) on a child named `tag`
/// in namespace `ns`; returns whether it is kept, then its text and tail afterwards.
fn foreign_case(
    ns: Ns,
    tag: &str,
    text: Option<&str>,
    tail: Option<&str>,
    light: bool,
) -> (bool, Option<String>, Option<String>) {
    let mut tree = Tree::new(LocalName::from("p"), Ns::Html, vec![]);
    let child = tree.add_child(tree.root(), LocalName::from(tag), ns, vec![]);
    tree.node_mut(child).text = text.map(str::to_string);
    tree.node_mut(child).tail = tail.map(str::to_string);
    let options = options();
    let call = Call::default();
    let ctx = Ctx {
        options: Cow::Borrowed(&options),
        call: &call,
    };
    let kept = if light {
        process_node(&mut tree, child, &ctx)
    } else {
        handle_textnode(&mut tree, child, &ctx, true, false)
    }
    .is_some();
    let node = tree.node(child);
    (kept, node.text.clone(), node.tail.clone())
}

#[test]
fn foreign_namespace_elements_are_kept_as_text() {
    let s = |value: &str| Some(value.to_string());
    // oracle: p = etree.Element('p'); c = etree.SubElement(p, '{http://www.w3.org/1998/Math/MathML}math')
    // with c.text, c.tail = 'x', None / None, ' t ' / 'x', ' t ': handle_textnode(c, Extractor()) is kept,
    // leaving (text, tail) = ('x', None) / ('t', '') / ('x', 't')
    assert_eq!(
        foreign_case(Ns::MathMl, "math", Some("x"), None, false),
        (true, s("x"), None)
    );
    assert_eq!(
        foreign_case(Ns::MathMl, "math", None, Some(" t "), false),
        (true, s("t"), s(""))
    );
    assert_eq!(
        foreign_case(Ns::MathMl, "math", Some("x"), Some(" t "), false),
        (true, s("x"), s("t"))
    );
    // oracle: same elements through process_node(c, Extractor()): kept, ('x', None) / ('t', None) / ('x', 't')
    assert_eq!(
        foreign_case(Ns::MathMl, "math", Some("x"), None, true),
        (true, s("x"), None)
    );
    assert_eq!(
        foreign_case(Ns::MathMl, "math", None, Some(" t "), true),
        (true, s("t"), None)
    );
    assert_eq!(
        foreign_case(Ns::MathMl, "math", Some("x"), Some(" t "), true),
        (true, s("x"), s("t"))
    );
    // oracle: an element tagged '{http://www.w3.org/2000/svg}done' with text 'x' is kept by
    // handle_textnode, because its tag is not 'done'
    assert_eq!(
        foreign_case(Ns::Svg, "done", Some("x"), None, false),
        (true, s("x"), None)
    );
    assert_eq!(
        foreign_case(Ns::Svg, "done", Some("x"), None, true),
        (true, s("x"), None)
    );
}

const COUNT_SOURCES: [&str; 6] = [
    "<p>a<b>b</b> c</p>",
    "<p>  <b> </b> </p>",
    "<p></p>",
    "<p>\x1cx\u{a0}<i>y</i>\u{a0}</p>",
    "<div> lead <a>link text</a>tail  <span>w</span></div>",
    "<p>a<b></b>b</p>",
];

/// The parsed `source` in a body, and the first element of that body.
fn first_body_element(source: &str) -> (Tree, NodeId) {
    let html = format!("<html><body>{source}</body></html>");
    let tree = parse_document(&html, &ParseLimits::default(), &Call::default()).unwrap();
    let body = tree.find(tree.root(), "body").unwrap();
    let first = tree.iterdescendants(body, &[]).next().unwrap();
    (tree, first)
}

#[test]
fn trimmed_len_matches_trim() {
    for source in COUNT_SOURCES {
        let (tree, element) = first_body_element(source);
        assert_eq!(
            trimmed_len(&tree, element),
            trim(&tree.text_content(element)).chars().count(),
            "{source:?}"
        );
    }
}

#[test]
fn char_len_matches_text_content() {
    for source in COUNT_SOURCES {
        let (tree, element) = first_body_element(source);
        assert_eq!(
            char_len(&tree, element),
            tree.text_content(element).chars().count(),
            "{source:?}"
        );
    }
}
