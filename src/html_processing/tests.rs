use std::borrow::Cow;
use std::sync::Arc;

use super::cases::{CASES, Case};
use super::*;
use crate::dom::{
    DumpMode, LocalName, Ns, ParseLimits, dump, dump_from, parse_document, parse_fragment,
};
use crate::error::Rejection;
use crate::settings::Extractor;
use crate::settings::{Config, Format};
use crate::xpaths::{
    COMMENTS_DISCARD_XPATH, DISCARD_IMAGE_ELEMENTS, OVERALL_DISCARD_XPATH, PRECISION_DISCARD_XPATH,
    RAW_TREE_PRUNE_XPATH, REMOVE_COMMENTS_AND_LISTS_XPATH, REMOVE_COMMENTS_XPATH,
    TEASER_DISCARD_XPATH,
};

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

fn selector_list(name: &str) -> &'static [Selector] {
    match name {
        "OVERALL_DISCARD_XPATH" => OVERALL_DISCARD_XPATH,
        "RAW_TREE_PRUNE_XPATH" => RAW_TREE_PRUNE_XPATH,
        "TEASER_DISCARD_XPATH" => TEASER_DISCARD_XPATH,
        "PRECISION_DISCARD_XPATH" => PRECISION_DISCARD_XPATH,
        "DISCARD_IMAGE_ELEMENTS" => DISCARD_IMAGE_ELEMENTS,
        "REMOVE_COMMENTS_XPATH" => REMOVE_COMMENTS_XPATH,
        "REMOVE_COMMENTS_AND_LISTS_XPATH" => REMOVE_COMMENTS_AND_LISTS_XPATH,
        "COMMENTS_DISCARD_XPATH" => COMMENTS_DISCARD_XPATH,
        other => panic!("unknown selector list {other}"),
    }
}

fn case_flag(text: &str, op: &str) -> bool {
    match text {
        "true" => true,
        "false" => false,
        other => panic!("{op}: bad flag {other}"),
    }
}

/// Runs the operation of `case` on `tree` with the case's options and returns the root of the result:
/// the document root, or what `tree_cleaning` returns.
fn run_case(case: &Case, tree: &mut Tree) -> NodeId {
    let mut options = options();
    options.tables = case.options.tables;
    options.images = case.options.images;
    options.links = case.options.links;
    options.formatting = case.options.formatting;
    let focus = match case.options.focus {
        "balanced" => Focus::Balanced,
        "precision" => Focus::Precision,
        "recall" => Focus::Recall,
        other => panic!("bad focus {other}"),
    };
    options.focus = focus;
    let call = Call::default();
    let ctx = Ctx {
        options: Cow::Borrowed(&options),
        call: &call,
    };
    let document = tree.root();
    let target = if case.scope.is_empty() {
        document
    } else {
        tree.find(document, case.scope).unwrap()
    };
    let parts: Vec<&str> = case.op.split(':').collect();
    match parts.as_slice() {
        ["tree_cleaning", edits @ ..] => {
            let mut cleaned = MANUALLY_CLEANED.to_vec();
            for edit in edits.iter().flat_map(|edits| edits.split(',')) {
                let (sign, tag) = edit.split_at(1);
                match sign {
                    "+" => cleaned.push(tag),
                    "-" => cleaned.retain(|listed| *listed != tag),
                    _ => panic!("bad edit {edit}"),
                }
            }
            return tree_cleaning_with(tree, target, &ctx, &cleaned, MANUALLY_STRIPPED).unwrap();
        }
        ["prune_html"] => prune_html(tree, target, focus, &call).unwrap(),
        ["convert_tags"] => convert_tags(tree, target, &ctx, None).unwrap(),
        ["prune_unwanted_nodes", list, rest @ ..] => {
            let with_backup = rest == ["backup"];
            prune_unwanted_nodes(tree, target, selector_list(list), with_backup, &call).unwrap();
        }
        ["delete_by_link_density", tag, backtracking, precision] => delete_by_link_density(
            tree,
            target,
            tag,
            case_flag(backtracking, case.op),
            case_flag(precision, case.op),
            &call,
        )
        .unwrap(),
        _ => panic!("unknown op {}", case.op),
    }
    document
}

#[test]
fn cases_match_upstream() {
    assert!(!CASES.is_empty());
    let mut parser_differences = Vec::new();
    let mut mismatches = Vec::new();
    for (index, case) in CASES.iter().enumerate() {
        let mut tree =
            parse_document(case.html, &ParseLimits::default(), &Call::default()).unwrap();
        let (parsed, normalized) = dump(&tree, DumpMode::Normalized);
        if normalized || parsed != case.dump_in {
            parser_differences.push(format!(
                "case {index} {}\n{}\nrust:\n{parsed}\nupstream:\n{}",
                case.op, case.html, case.dump_in
            ));
            continue;
        }
        let returned = run_case(case, &mut tree);
        let (actual, _) = dump_from(&tree, returned, DumpMode::Normalized);
        if actual != case.dump_out {
            let options = &case.options;
            mismatches.push(format!(
                "case {index} {} {:?} tables={} images={} links={} formatting={} focus={}\n{}\nrust:\n{actual}\nupstream:\n{}",
                case.op,
                case.scope,
                options.tables,
                options.images,
                options.links,
                options.formatting,
                options.focus,
                case.html,
                case.dump_out
            ));
        }
    }
    assert!(
        parser_differences.is_empty(),
        "{} of {} snippets parse differently from html5lib (change the snippet)\n{}",
        parser_differences.len(),
        CASES.len(),
        parser_differences.join("\n")
    );
    assert!(
        mismatches.is_empty(),
        "{} of {} cases differ from upstream\n{}",
        mismatches.len(),
        CASES.len(),
        mismatches.join("\n")
    );
}

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
