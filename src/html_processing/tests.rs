use std::borrow::Cow;
use std::sync::Arc;

use super::*;
use crate::dom::{DumpMode, LocalName, Ns, ParseLimits, dump, parse_document, parse_fragment};
use crate::settings::Extractor;
use crate::settings::{Config, Format};

fn options() -> Extractor {
    Extractor::new(
        Config::default(),
        Format::Python,
        false,
        false,
        None,
        false,
        false,
        Default::default(),
        Default::default(),
    )
}

fn attrs(pairs: &[(&str, &str)]) -> Vec<(LocalName, Arc<str>)> {
    pairs
        .iter()
        .map(|(key, value)| (LocalName::from(*key), Arc::from(*value)))
        .collect()
}

/// `<html><body><p>a <math ...>...</math>{tail}</p></body></html>` with every element in the HTML
/// namespace, the shape libxml2 gives upstream; returns the tree and the `p`.
fn math_page(math_attrs: &[(&str, &str)], tex: Option<&str>, tail: &str) -> (Tree, NodeId) {
    let name = |n: &str| LocalName::from(n);
    let mut tree = Tree::new(name("html"), Ns::Html, vec![]);
    let body = tree.add_child(tree.root(), name("body"), Ns::Html, vec![]);
    let p = tree.add_child(body, name("p"), Ns::Html, vec![]);
    tree.add_text(p, "a ");
    let math = tree.add_child(p, name("math"), Ns::Html, attrs(math_attrs));
    match tex {
        Some(tex) => {
            let semantics = tree.add_child(math, name("semantics"), Ns::Html, vec![]);
            let annotation = tree.add_child(
                semantics,
                name("annotation"),
                Ns::Html,
                attrs(&[("encoding", "application/x-tex")]),
            );
            tree.add_text(annotation, tex);
        }
        None => {
            let mi = tree.add_child(math, name("mi"), Ns::Html, vec![]);
            tree.add_text(mi, "q");
        }
    }
    tree.add_text(p, tail);
    (tree, p)
}

/// The text of the paragraph of `math_page` after `tree_cleaning`, and whether a `math` is left.
fn cleaned_paragraph(math_attrs: &[(&str, &str)], tex: Option<&str>, tail: &str) -> (String, bool) {
    let (mut tree, p) = math_page(math_attrs, tex, tail);
    let options = options();
    let call = Call::default();
    let ctx = Ctx {
        options: Cow::Borrowed(&options),
        call: &call,
    };
    let root = tree.root();
    tree_cleaning(&mut tree, root, &ctx).unwrap();
    (tree.text_content(p), tree.find(root, "math").is_some())
}

#[test]
fn recover_math_on_html_namespace_math() {
    // the annotation holds the original LaTeX and takes precedence over alttext
    let (text, has_math) = cleaned_paragraph(&[("alttext", "rendered")], Some("x^2"), " b");
    assert_eq!(text, "a \\(x^2\\) b");
    assert!(!has_math);
    // alttext is used when there is no LaTeX annotation
    let (text, _) = cleaned_paragraph(&[("alttext", "y_1")], None, " b");
    assert_eq!(text, "a \\(y_1\\) b");
    // display="block" yields block delimiters
    let (text, _) = cleaned_paragraph(&[("display", "block"), ("alttext", "z")], None, "");
    assert_eq!(text, "a \\[z\\]");
    // nothing to recover: the element is dropped
    let (text, has_math) = cleaned_paragraph(&[], None, " b");
    assert_eq!(text, "a  b");
    assert!(!has_math);
    // the tail of the formula survives alongside the recovered source
    let (text, _) = cleaned_paragraph(&[("alttext", "u")], None, " tail");
    assert_eq!(text, "a \\(u\\) tail");
}

#[test]
fn cleaning_loop_skips_like_lxml() {
    let mut tree = parse_document(
        "<aside><aside>in</aside></aside><aside>after</aside><p>x</p>",
        &ParseLimits::default(),
        &Call::default(),
    )
    .unwrap();
    let options = options();
    let call = Call::default();
    let ctx = Ctx {
        options: Cow::Borrowed(&options),
        call: &call,
    };
    let root = tree.root();
    tree_cleaning(&mut tree, root, &ctx).unwrap();
    // oracle: dump_tree(tree_cleaning(parse_document(html), Extractor())): deleting the outer aside
    // leaves the walk at the detached inner one, so the second aside is never visited
    let expected = "html\n  body\n    aside\n      #text \"after\"\n    p\n      #text \"x\"\n";
    assert_eq!(dump(&tree, DumpMode::Exact).0, expected);
}

/// `.//div[@class='widget']`
fn widget_divs(tree: &Tree, context: NodeId, call: &Call<'_>) -> Result<Vec<NodeId>, ExtractError> {
    descendants(tree, context, call, |tree, id| {
        tree.tag(id) == Some("div") && tree.get(id, "class") == Some("widget")
    })
}

/// Whether `prune_unwanted_nodes(body, [widget_divs], with_backup=True)` removes the widgets of
/// `<body><p>{kept}</p>{widgets}</body>`.
fn backup_prunes(kept: usize, widgets: &str) -> bool {
    let html = format!(
        "<html><body><p>{}</p>{widgets}</body></html>",
        "k".repeat(kept)
    );
    let call = Call::default();
    let mut tree = parse_document(&html, &ParseLimits::default(), &call).unwrap();
    let root = tree.root();
    let body = tree.find(root, "body").unwrap();
    let before = tree.text_content(body);
    prune_unwanted_nodes(&mut tree, body, &[widget_divs], true, &call).unwrap();
    tree.text_content(body) != before
}

#[test]
fn backup_guard_boundary() {
    // pruned only when `old_len - removed > old_len / 7`, a float division: old_len 70 gives 10.0
    // (kept == 10 is not enough), old_len 71 gives 10.14 (kept == 10 is not enough either)
    // oracle: prune_unwanted_nodes(body, [XPath(".//div[@class='widget']")], with_backup=True) over
    // parse_document("<html><body><p>" + "k" * kept + "</p><div class='widget'>" + "w" * widget
    // + "</div></body></html>"), true when body.text_content() changed
    for (kept, widget, pruned) in [
        (10, 60, false), // old_len 70: exactly old_len - old_len / 7 removed
        (11, 59, true),  // one character less removed
        (9, 61, false),  // one character more removed
        (10, 61, false), // old_len 71
        (11, 60, true),
        (12, 59, true),
    ] {
        let widgets = format!("<div class='widget'>{}</div>", "w".repeat(widget));
        assert_eq!(
            backup_prunes(kept, &widgets),
            pruned,
            "kept {kept}, widget {widget}"
        );
    }
}

#[test]
fn backup_counts_nested_match_once() {
    // A widget inside a widget is deleted with its outer match, so its text counts once: 59 removed
    // of 70 leaves 11 > 10.0 and prunes; counting the inner text twice would remove 89 and keep.
    // oracle: same expression as backup_guard_boundary with the widget
    // "<div class='widget'>" + "o" * outer + "<div class='widget'>" + "n" * inner + "</div></div>"
    for (kept, outer, inner, pruned) in [(11, 30, 29, true), (11, 0, 59, true), (10, 30, 30, false)]
    {
        let widgets = format!(
            "<div class='widget'>{}<div class='widget'>{}</div></div>",
            "o".repeat(outer),
            "n".repeat(inner)
        );
        assert_eq!(
            backup_prunes(kept, &widgets),
            pruned,
            "kept {kept}, outer {outer}, inner {inner}"
        );
    }
    // the outer match is the only deletion: nothing of the nested one is left to find
    let call = Call::default();
    let html = "<html><body><p>keep keep</p><div class='widget'>a<div class='widget'>b</div></div></body></html>";
    let mut tree = parse_document(html, &ParseLimits::default(), &call).unwrap();
    let root = tree.root();
    let body = tree.find(root, "body").unwrap();
    prune_unwanted_nodes(&mut tree, body, &[widget_divs], true, &call).unwrap();
    assert_eq!(tree.text_content(body), "keep keep");
    assert!(tree.find(body, "div").is_none());
}

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
