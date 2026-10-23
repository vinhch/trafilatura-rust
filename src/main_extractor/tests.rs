use std::borrow::Cow;
use std::sync::Arc;

use super::*;
use crate::cancel::CancelToken;
use crate::dom::{DumpMode, ParseLimits, dump, dump_from, parse_document};
use crate::html_processing::{convert_tags, tree_cleaning};
use crate::settings::{Config, Format, TAG_CATALOG};

fn options_with_recall(recall: bool) -> Extractor {
    Extractor::new(
        Config::default(),
        Format::Python,
        false,
        recall,
        None,
        false,
        false,
        Default::default(),
        Default::default(),
    )
}

fn options() -> Extractor {
    options_with_recall(false)
}

fn ctx_for<'a>(options: &'a Extractor, call: &'a Call<'a>) -> Ctx<'a> {
    Ctx {
        options: Cow::Borrowed(options),
        call,
    }
}

/// `etree.fromstring(source)`: tags with double-quoted attributes, self-closing tags, text and
/// tail; the first tag is the root.
fn xml(source: &str) -> Tree {
    let mut tree: Option<Tree> = None;
    let mut stack: Vec<NodeId> = Vec::new();
    for piece in source.split('<').skip(1) {
        let (tag, text) = piece.split_once('>').unwrap();
        if tag.starts_with('/') {
            stack.pop();
        } else {
            let (tag, self_closing) = match tag.strip_suffix('/') {
                Some(tag) => (tag, true),
                None => (tag, false),
            };
            let (name, rest) = tag.split_once(' ').unwrap_or((tag, ""));
            let attrs: Vec<(LocalName, Arc<str>)> = rest
                .split("\" ")
                .filter(|pair| !pair.is_empty())
                .map(|pair| {
                    let (key, value) = pair.split_once("=\"").unwrap();
                    (LocalName::from(key), Arc::from(value.trim_end_matches('"')))
                })
                .collect();
            let id = match (tree.as_mut(), stack.last()) {
                (Some(tree), Some(&parent)) => {
                    tree.add_child(parent, LocalName::from(name), Ns::Html, attrs)
                }
                _ => {
                    let new = Tree::new(LocalName::from(name), Ns::Html, attrs);
                    let root = new.root();
                    tree = Some(new);
                    root
                }
            };
            if !self_closing {
                stack.push(id);
            }
        }
        if let (Some(tree), Some(&top)) = (tree.as_mut(), stack.last()) {
            tree.add_text(top, text);
        }
    }
    tree.unwrap()
}

/// `etree.tostring(element, with_tail=False)`.
fn show(tree: &Tree, id: NodeId) -> String {
    let node = tree.node(id);
    let mut out = format!("<{}", node.name);
    for (key, value) in &node.attrs {
        out.push_str(&format!(" {key}=\"{value}\""));
    }
    if node.text.is_none() && tree.first_child(id).is_none() {
        out.push_str("/>");
        return out;
    }
    out.push('>');
    out.push_str(node.text.as_deref().unwrap_or(""));
    for child in tree.children(id) {
        out.push_str(&show(tree, child));
        out.push_str(tree.node(child).tail.as_deref().unwrap_or(""));
    }
    out.push_str(&format!("</{}>", node.name));
    out
}

/// The result of `handle_paragraphs(source, potential_tags)` and the source after the call.
fn paragraph_in(source: &str, potential_tags: &[&str]) -> (String, String) {
    let mut tree = xml(source);
    let root = tree.root();
    let options = options();
    let call = Call::default();
    let processed = handle_paragraphs(&mut tree, root, potential_tags, &ctx_for(&options, &call))
        .unwrap()
        .unwrap();
    (show(&tree, processed), show(&tree, root))
}

/// The result of `handle_paragraphs(source, TAG_CATALOG)` and the source after the call.
fn paragraph(source: &str) -> (String, String) {
    paragraph_in(source, TAG_CATALOG)
}

type Handler = fn(&mut Tree, NodeId, &Ctx<'_>) -> Result<Option<NodeId>, ExtractError>;

/// The result of a list or quote handler on `etree.fromstring(source)` and the source after the
/// call.
fn handled(source: &str, handler: Handler) -> (Option<String>, String) {
    let mut tree = xml(source);
    let root = tree.root();
    let options = options();
    let call = Call::default();
    let processed = handler(&mut tree, root, &ctx_for(&options, &call)).unwrap();
    (processed.map(|id| show(&tree, id)), show(&tree, root))
}

fn list(source: &str) -> (Option<String>, String) {
    handled(source, handle_lists)
}

fn quote(source: &str) -> (Option<String>, String) {
    handled(source, handle_quotes)
}

/// The `tag` element of the html5 tree of `html` after `tree_cleaning` and `convert_tags` with
/// default options.
fn converted(html: &str, tag: &str) -> (Tree, NodeId) {
    let mut tree = parse_document(html, &ParseLimits::default(), &Call::default()).unwrap();
    let root = tree.root();
    let options = options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let root = tree_cleaning(&mut tree, root, &ctx).unwrap();
    convert_tags(&mut tree, root, &ctx, None).unwrap();
    let element = tree.find(root, tag).unwrap();
    (tree, element)
}

#[test]
fn quote_tags_are_the_catalog_plus_ref_and_graphic() {
    let mut expected: Vec<&str> = TAG_CATALOG.to_vec();
    expected.extend(["ref", "graphic"]);
    assert_eq!(QUOTE_TAGS, expected);
}

#[test]
fn paragraph_walk_stops_after_stripped_hi() {
    // oracle: etree.tostring(handle_paragraphs(etree.fromstring(s), TAG_CATALOG, Extractor()))
    // for s = '<p>A <hi>b<lb/>c</hi> e <hi>f</hi> g</p>'; the source is left as
    // <done>A <done>b c</done> e <hi>f</hi> g</done>
    let (processed, source) = paragraph("<p>A <hi>b<lb/>c</hi> e <hi>f</hi> g</p>");
    assert_eq!(processed, "<p>A <hi>b c</hi> e </p>");
    assert_eq!(source, "<done>A <done>b c</done> e <hi>f</hi> g</done>");
    // oracle: the same for '<p>A <hi>b</hi> e <hi>f</hi> g</p>'
    let (processed, source) = paragraph("<p>A <hi>b</hi> e <hi>f</hi> g</p>");
    assert_eq!(processed, "<p>A <hi>b</hi> e <hi>f</hi> g</p>");
    assert_eq!(source, "<done>A <done>b</done> e <done>f</done> g</done>");
}

#[test]
fn paragraph_strips_foreign_descendants() {
    // oracle: dump_tree(handle_paragraphs(p, TAG_CATALOG, Extractor())) with
    // p = parse_document("<p>a <svg><title>t</title></svg> b</p>").find('.//p'): 'p', '#text "a t b"';
    // dump_tree(doc) then has the document as html > head, body > done > #text "a t b"
    let mut tree = parse_document(
        "<p>a <svg><title>t</title></svg> b</p>",
        &ParseLimits::default(),
        &Call::default(),
    )
    .unwrap();
    let p = tree.find(tree.root(), "p").unwrap();
    let options = options();
    let call = Call::default();
    let processed = handle_paragraphs(&mut tree, p, TAG_CATALOG, &ctx_for(&options, &call))
        .unwrap()
        .unwrap();
    assert_eq!(
        dump_from(&tree, processed, DumpMode::Exact).0,
        "p\n  #text \"a t b\"\n"
    );
    assert_eq!(
        dump(&tree, DumpMode::Exact).0,
        "html\n  head\n  body\n    done\n      #text \"a t b\"\n"
    );
}

#[test]
fn textnode_comments_fix_false_keeps_empty_lb_before_inline() {
    let options = options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    // oracle: handle_textnode(p[0], Extractor(), comments_fix=fix, preserve_spaces=False) on
    // etree.fromstring('<p><lb/><hi>x</hi></p>'): the lb for fix False, None for True
    let mut tree = xml("<p><lb/><hi>x</hi></p>");
    let lb = tree.first_child(tree.root()).unwrap();
    assert_eq!(handle_textnode(&mut tree, lb, &ctx, false, false), Some(lb));
    assert_eq!(handle_textnode(&mut tree, lb, &ctx, true, false), None);
    // oracle: etree.tostring(handle_paragraphs(etree.fromstring('<p>a<lb/><hi>x</hi></p>'), TAG_CATALOG, ...))
    assert_eq!(
        paragraph("<p>a<lb/><hi>x</hi></p>").0,
        "<p>a<lb/><hi>x</hi></p>"
    );
}

#[test]
fn paragraph_empty_join_is_empty_text_node() {
    // oracle: p = etree.fromstring('<div><p><hi>Bold</hi> rest</p>Some tail text here</div>')[0];
    // r = handle_paragraphs(p, TAG_CATALOG, Extractor()); r.text == '' and
    // list(r.itertext()) == ['', 'Bold', ' rest']
    let mut tree = xml("<div><p><hi>Bold</hi> rest</p>Some tail text here</div>");
    let p = tree.first_child(tree.root()).unwrap();
    let options = options();
    let call = Call::default();
    let processed = handle_paragraphs(&mut tree, p, TAG_CATALOG, &ctx_for(&options, &call))
        .unwrap()
        .unwrap();
    assert_eq!(tree.node(processed).text.as_deref(), Some(""));
    assert_eq!(
        tree.itertext_lxml(processed).collect::<Vec<_>>(),
        ["", "Bold", " rest"]
    );
}

#[test]
fn paragraph_strips_children_tags_at_every_depth() {
    // oracle: handle_paragraphs(etree.fromstring(s), TAG_CATALOG, Extractor()) for
    // s = '<p>x<hi>a<quote>b<hi>c</hi></quote></hi>z</p>': <p>x<hi>a b</hi>z</p>, and the source
    // is left as <done>x<done>a b<hi>c</hi></done>z</done>
    let (processed, source) = paragraph("<p>x<hi>a<quote>b<hi>c</hi></quote></hi>z</p>");
    assert_eq!(processed, "<p>x<hi>a b</hi>z</p>");
    assert_eq!(source, "<done>x<done>a b<hi>c</hi></done>z</done>");
}

#[test]
fn paragraph_walk_checks_cancellation() {
    let mut tree = xml("<p>a<hi>b</hi></p>");
    let root = tree.root();
    let options = options();
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    assert_eq!(
        handle_paragraphs(&mut tree, root, TAG_CATALOG, &ctx_for(&options, &call)),
        Err(ExtractError::Cancelled)
    );
}

#[test]
fn code_block_needs_direct_code_child() {
    // oracle: is_code_block_element(el) for each source below
    let check = |source: &str, on_child: bool| {
        let tree = xml(source);
        let element = if on_child {
            tree.first_child(tree.root()).unwrap()
        } else {
            tree.root()
        };
        is_code_block_element(&tree, element)
    };
    assert!(!check("<quote><p><code>x</code></p></quote>", false));
    assert!(check("<quote><code>x</code></quote>", false));
    assert!(check("<quote> <code>x</code> </quote>", false));
    assert!(!check("<quote><code>x</code>tail</quote>", false));
    assert!(!check("<quote>t<code>x</code></quote>", false));
    assert!(!check("<quote><code>x</code><hi>y</hi></quote>", false));
    assert!(check("<quote lang=\"py\">x</quote>", false));
    assert!(check(
        "<div class=\"highlight\"><quote>x</quote></div>",
        true
    ));
    assert!(!check("<div class=\"x\"><quote>x</quote></div>", true));
}

/// `handle_image` on `<graphic src=.. alt="a" title="t"/>` with the tail `tl`.
fn image(src: &str, url: Option<&str>) -> Result<Option<String>, ExtractError> {
    let mut tree = xml(&format!("<graphic src=\"{src}\" alt=\"a\" title=\"t\"/>"));
    let root = tree.root();
    tree.node_mut(root).tail = Some("tl".to_string());
    let mut options = options();
    options.url = url.map(str::to_string);
    let processed = handle_image(&mut tree, root, Some(&options))?;
    Ok(processed.map(|id| {
        format!(
            "{}{}",
            show(&tree, id),
            tree.node(id).tail.as_deref().unwrap()
        )
    }))
}

#[test]
fn image_urljoin_error_is_invalid_value() {
    // oracle: etree.tostring(handle_image(el, options)) with options.url = url
    assert_eq!(
        image("//[::1/x.jpg", Some("https://example.org/")),
        Err(ExtractError::Rejected(Rejection::InvalidValue))
    );
    assert_eq!(
        image("/x.jpg", Some("https://example.org/"))
            .unwrap()
            .as_deref(),
        Some("<graphic src=\"https://example.org/x.jpg\" alt=\"a\" title=\"t\"/>tl")
    );
    assert_eq!(
        image("//x/y.jpg", None).unwrap().as_deref(),
        Some("<graphic src=\"http://x/y.jpg\" alt=\"a\" title=\"t\"/>tl")
    );
    // a source starting with "http" is kept, url or not; a relative one without url stays as is
    assert_eq!(
        image("httpfoo.jpg", Some("https://example.org/"))
            .unwrap()
            .as_deref(),
        Some("<graphic src=\"httpfoo.jpg\" alt=\"a\" title=\"t\"/>tl")
    );
    assert_eq!(
        image("x.jpg", None).unwrap().as_deref(),
        Some("<graphic src=\"x.jpg\" alt=\"a\" title=\"t\"/>tl")
    );
}

#[test]
fn titles_copy_drops_done_children() {
    // oracle: r = handle_titles(e, Extractor()) for e = etree.fromstring(
    // '<head>T<done>old</done>X<hi>new</hi>tail</head>'): <head>T<hi>new</hi>tail</head>, and the
    // source is left as <head>T<done>old</done>X<done>new</done>tail</head>
    let mut tree = xml("<head>T<done>old</done>X<hi>new</hi>tail</head>");
    let root = tree.root();
    let options = options();
    let call = Call::default();
    let title = handle_titles(&mut tree, root, &ctx_for(&options, &call))
        .unwrap()
        .unwrap();
    assert_eq!(show(&tree, title), "<head>T<hi>new</hi>tail</head>");
    assert_eq!(
        show(&tree, root),
        "<head>T<done>old</done>X<done>new</done>tail</head>"
    );
    // oracle: handle_titles(etree.fromstring('<head><done>Lead. </done>Teaser</head>'), ...) is None
    let mut tree = xml("<head><done>Lead. </done>Teaser</head>");
    let root = tree.root();
    assert_eq!(
        handle_titles(&mut tree, root, &ctx_for(&options, &call)),
        Ok(None)
    );
}

#[test]
fn other_elements_w3_code_becomes_code() {
    let options = options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    // oracle: r = handle_other_elements(e, {"div"}, Extractor()) for
    // e = etree.fromstring('<div class="w3-code notranslate">x<p>y</p>z</div>'):
    // <code class="w3-code notranslate">x<p>y</p>z</code>, source <done class="w3-code notranslate">x<done>y</done>z</done>
    let mut tree = xml("<div class=\"w3-code notranslate\">x<p>y</p>z</div>");
    let root = tree.root();
    let code = handle_other_elements(&mut tree, root, &["div"], &ctx)
        .unwrap()
        .unwrap();
    assert_eq!(
        show(&tree, code),
        "<code class=\"w3-code notranslate\">x<p>y</p>z</code>"
    );
    assert_eq!(
        show(&tree, root),
        "<done class=\"w3-code notranslate\">x<done>y</done>z</done>"
    );
    // oracle: a w3-code div is a code block even with an empty potential_tags
    let mut tree = xml("<div class=\"w3-code\">x</div>");
    let root = tree.root();
    let code = handle_other_elements(&mut tree, root, &[], &ctx)
        .unwrap()
        .unwrap();
    assert_eq!(show(&tree, code), "<code class=\"w3-code\">x</code>");
    // oracle: a div in potential_tags becomes a p without attributes; otherwise, and for any other
    // tag, None with the source untouched
    let mut tree = xml("<div id=\"a\" class=\"b\"> x </div>");
    let root = tree.root();
    let paragraph = handle_other_elements(&mut tree, root, &["div"], &ctx)
        .unwrap()
        .unwrap();
    assert_eq!(show(&tree, paragraph), "<p> x </p>");
    let mut tree = xml("<div id=\"a\"> x </div>");
    let root = tree.root();
    assert_eq!(handle_other_elements(&mut tree, root, &[], &ctx), Ok(None));
    assert_eq!(show(&tree, root), "<div id=\"a\"> x </div>");
    let mut tree = xml("<section>x</section>");
    let root = tree.root();
    assert_eq!(
        handle_other_elements(&mut tree, root, &["div"], &ctx),
        Ok(None)
    );
}

#[test]
fn paragraph_nested_inline_carry() {
    // oracle: etree.tostring(handle_paragraphs(etree.fromstring(s), TAG_CATALOG, Extractor())) for
    // s = '<p>a<hi>b<hi>c</hi>d</hi></p>': the same markup, and the source is left as
    // <done>a<done>b<done>c</done>d</done></done>
    let (processed, source) = paragraph("<p>a<hi>b<hi>c</hi>d</hi></p>");
    assert_eq!(processed, "<p>a<hi>b<hi>c</hi>d</hi></p>");
    assert_eq!(source, "<done>a<done>b<done>c</done>d</done></done>");
    // oracle: the same for '<p>a<hi>b<hi>c</hi>d</hi>e</p>'
    assert_eq!(
        paragraph("<p>a<hi>b<hi>c</hi>d</hi>e</p>").0,
        "<p>a<hi>b<hi>c</hi>d</hi>e</p>"
    );
    // oracle: the same for '<p>a<ref target="t">r<hi>x</hi></ref>b</p>' with {*TAG_CATALOG, "ref"}
    let mut tags = TAG_CATALOG.to_vec();
    tags.push("ref");
    assert_eq!(
        paragraph_in("<p>a<ref target=\"t\">r<hi>x</hi></ref>b</p>", &tags).0,
        "<p>a<ref target=\"t\">r<hi>x</hi></ref>b</p>"
    );
}

#[test]
fn paragraph_graphic_is_rebuilt_by_handle_image() {
    // oracle: etree.tostring(handle_paragraphs(etree.fromstring(s), {*TAG_CATALOG, "graphic"},
    // Extractor())) for s = '<p>a<graphic src="x.jpg" class="c"/>b</p>': the image is rebuilt with
    // src only and keeps its tail; the source is left as <done>a<done src="x.jpg" class="c"/>b</done>
    let mut tags = TAG_CATALOG.to_vec();
    tags.push("graphic");
    let (processed, source) = paragraph_in("<p>a<graphic src=\"x.jpg\" class=\"c\"/>b</p>", &tags);
    assert_eq!(processed, "<p>a<graphic src=\"x.jpg\"/>b</p>");
    assert_eq!(source, "<done>a<done src=\"x.jpg\" class=\"c\"/>b</done>");
}

#[test]
fn list_walk_stops_after_stripped_first_child() {
    // oracle: etree.tostring(handle_lists(etree.fromstring(s), Extractor())) for
    // s = '<list><item><p><foo>x</foo> y</p><p>z <hi>w</hi></p></item></list>'; the source is left
    // as <done><done><done>x y</done><p>z <hi>w</hi></p></done></done>
    let (processed, source) =
        list("<list><item><p><foo>x</foo> y</p><p>z <hi>w</hi></p></item></list>");
    assert_eq!(
        processed.as_deref(),
        Some("<list><item><p>x y</p></item></list>")
    );
    assert_eq!(
        source,
        "<done><done><done>x y</done><p>z <hi>w</hi></p></done></done>"
    );
    // oracle: the same with <hi>x</hi> in place of <foo>x</foo>: both paragraphs stay
    let (processed, source) =
        list("<list><item><p><hi>x</hi> y</p><p>z <hi>w</hi></p></item></list>");
    assert_eq!(
        processed.as_deref(),
        Some("<list><item><p><hi>x</hi> y</p><p>z <hi>w</hi></p></item></list>")
    );
    assert_eq!(
        source,
        "<done><done><done><done>x</done> y</done><done>z <done>w</done></done></done></done>"
    );
}

#[test]
fn nested_list_first_item_not_repeated() {
    // oracle: etree.tostring(handle_lists(etree.fromstring(s), Extractor())) for
    // s = '<list><item>a<list><item>b<list><item>c</item></list></item></list></item></list>'
    let markup =
        "<list><item>a<list><item>b<list><item>c</item></list></item></list></item></list>";
    let (processed, source) = list(markup);
    assert_eq!(processed.as_deref(), Some(markup));
    assert_eq!(
        source,
        "<done><done>a<done><done>b<done><done>c</done></done></done></done></done></done>"
    );
}

#[test]
fn list_item_foreign_title_kept_with_namespace() {
    // oracle: p = parse_document('<html><body><ul><li>a <svg><title>t</title></svg> b</li></ul></body></html>')
    // after tree_cleaning and convert_tags; r = handle_lists(p.find('.//list'), Extractor());
    // dump_tree(r) and dump_tree(p)
    let (mut tree, list) = converted(
        "<html><body><ul><li>a <svg><title>t</title></svg> b</li></ul></body></html>",
        "list",
    );
    let options = options();
    let call = Call::default();
    let processed = handle_lists(&mut tree, list, &ctx_for(&options, &call))
        .unwrap()
        .unwrap();
    assert_eq!(
        dump_from(&tree, processed, DumpMode::Exact).0,
        "list rend=\"ul\"\n  item\n    #text \"a \"\n    title@svg\n      #text \"t\"\n    #text \"b\"\n"
    );
    assert_eq!(
        dump(&tree, DumpMode::Exact).0,
        "html\n  body\n    done rend=\"ul\"\n      done\n        #text \"a \"\n        done\n          done\n            #text \"t\"\n          #text \"b\"\n"
    );
}

#[test]
fn quote_foreign_title_kept_with_namespace() {
    // oracle: the same with '<blockquote>a <svg><title>t</title></svg> b</blockquote>' and
    // handle_quotes(p.find('.//quote'), Extractor())
    let (mut tree, quote) = converted(
        "<html><body><blockquote>a <svg><title>t</title></svg> b</blockquote></body></html>",
        "quote",
    );
    let options = options();
    let call = Call::default();
    let processed = handle_quotes(&mut tree, quote, &ctx_for(&options, &call))
        .unwrap()
        .unwrap();
    assert_eq!(
        dump_from(&tree, processed, DumpMode::Exact).0,
        "quote\n  #text \"a \"\n  title@svg\n    #text \"t\"\n  #text \"b\"\n"
    );
    assert_eq!(
        dump(&tree, DumpMode::Exact).0,
        "html\n  body\n    quote\n      #text \"a \"\n      done\n        done\n          #text \"t\"\n        #text \"b\"\n"
    );
}

#[test]
fn append_block_spacing() {
    // oracle: etree.tostring(handle_quotes(etree.fromstring(s), Extractor())) for each s below
    let cases = [
        // a dropped block still separates the text around it
        (
            "<quote>a<list><item/></list>b</quote>",
            "<quote>a b</quote>",
        ),
        (
            "<quote>a<list><item/></list> b</quote>",
            "<quote>a b</quote>",
        ),
        (
            "<quote>a <list><item/></list>b</quote>",
            "<quote>a b</quote>",
        ),
        ("<quote>a<p><hi/></p> b</quote>", "<quote>a b</quote>"),
        // nothing before it: no space is invented
        ("<quote><list><item/></list>b</quote>", "<quote>b</quote>"),
        ("<quote><list><item/></list> b</quote>", "<quote> b</quote>"),
        // after a kept child the text joins that child's tail
        (
            "<quote><hi>x</hi><list><item/></list>b</quote>",
            "<quote><hi>x</hi> b</quote>",
        ),
        (
            "<quote><hi>x</hi>a<list><item/></list>b</quote>",
            "<quote><hi>x</hi>a b</quote>",
        ),
        (
            "<quote><hi>x</hi>\n<list><item/></list>b</quote>",
            "<quote><hi>x</hi>\nb</quote>",
        ),
        // a dropped image does not
        ("<quote>a<graphic/>b</quote>", "<quote>ab</quote>"),
        (
            "<quote><hi>x</hi><graphic/>b</quote>",
            "<quote><hi>x</hi>b</quote>",
        ),
        (
            "<quote>a<p>q</p><graphic/>b</quote>",
            "<quote>a<p>q</p>b</quote>",
        ),
        // a whitespace-only tail is not kept
        ("<quote>a<list><item/></list> </quote>", "<quote>a</quote>"),
        ("<quote>a<graphic/></quote>", "<quote>a</quote>"),
        // a no-break space is a space for Python's isspace
        (
            "<quote>a\u{a0}<list><item/></list>b</quote>",
            "<quote>a\u{a0}b</quote>",
        ),
        (
            "<quote>a<list><item/></list>\u{a0}b</quote>",
            "<quote>a\u{a0}b</quote>",
        ),
        (
            "<quote>a<list><item/></list>\u{a0}</quote>",
            "<quote>a</quote>",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(quote(source).0.as_deref(), Some(expected), "{source}");
    }
}

#[test]
fn nested_walk_checks_cancellation() {
    let options = options();
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    let mut tree = xml("<quote>a<hi>b</hi></quote>");
    let root = tree.root();
    assert_eq!(
        handle_quotes(&mut tree, root, &ctx_for(&options, &call)),
        Err(ExtractError::Cancelled)
    );
    let mut tree = xml("<list><item>a<hi>b</hi></item></list>");
    let root = tree.root();
    assert_eq!(
        handle_lists(&mut tree, root, &ctx_for(&options, &call)),
        Err(ExtractError::Cancelled)
    );
}

#[test]
fn nested_block_tail_merges_into_last_child() {
    // oracle: etree.tostring(handle_quotes(etree.fromstring(s), Extractor())) for
    // s = '<quote><div><hi>B</hi></div>tail</quote>': the div is dropped and its tail rides on the
    // hi that follows it
    assert_eq!(
        quote("<quote><div><hi>B</hi></div>tail</quote>")
            .0
            .as_deref(),
        Some("<quote><hi>B</hi>tail</quote>")
    );
}

#[test]
fn nested_quote_is_stripped() {
    // oracle: the same for s = '<quote>a<quote>b</quote></quote>'
    assert_eq!(
        quote("<quote>a<quote>b</quote></quote>").0.as_deref(),
        Some("<quote>ab</quote>")
    );
}

/// `Tree` with a `td` root carrying `attr="value"`.
fn cell_with(attr: &str, value: &str) -> (Tree, NodeId) {
    let mut tree = Tree::new(LocalName::from("td"), Ns::Html, Vec::new());
    let cell = tree.root();
    tree.set(cell, attr, value);
    (tree, cell)
}

fn colspan_of(value: &str) -> Result<usize, ExtractError> {
    let (tree, cell) = cell_with("colspan", value);
    span(&tree, cell, "colspan")
}

#[test]
fn span_reads_nd_digits() {
    // oracle: _span(td, "colspan") for td.set("colspan", value)
    assert_eq!(colspan_of("\u{663}"), Ok(3));
    assert_eq!(colspan_of("\u{661}\u{662}"), Ok(12));
    assert_eq!(colspan_of("\u{FF11}"), Ok(1));
    assert_eq!(colspan_of("3"), Ok(3));
    assert_eq!(colspan_of("12"), Ok(12));
    // not Nd, or not only digits: 1
    for value in ["\u{b2}", "1\u{b2}", "2x", "", "-2", " 2"] {
        assert_eq!(colspan_of(value), Ok(1), "{value:?}");
    }
    // a missing attribute is 1
    let tree = Tree::new(LocalName::from("td"), Ns::Html, Vec::new());
    assert_eq!(span(&tree, tree.root(), "colspan"), Ok(1));
    // the attribute asked for, not another one
    let (tree, cell) = cell_with("rowspan", "7");
    assert_eq!(span(&tree, cell, "colspan"), Ok(1));
    assert_eq!(span(&tree, cell, "rowspan"), Ok(7));
}

#[test]
fn span_zero() {
    // oracle: _span(td, "colspan") is 0 for "0" and 7 for "0" * 97 + "7"
    assert_eq!(colspan_of("0"), Ok(0));
    assert_eq!(colspan_of(&format!("{}7", "0".repeat(97))), Ok(7));
}

#[test]
fn span_caps_at_100() {
    // oracle: _span(td, "colspan") is 100 for "100", "101" and "1" * 4300, 0 for "0" * 4300
    assert_eq!(colspan_of("100"), Ok(100));
    assert_eq!(colspan_of("101"), Ok(100));
    assert_eq!(colspan_of(&"1".repeat(4300)), Ok(100));
    assert_eq!(colspan_of(&"0".repeat(4300)), Ok(0));
}

#[test]
fn span_too_many_digits_is_invalid_value() {
    // oracle: _span(td, "colspan") raises ValueError for "0" * 4301, "٣" * 4301 and "1" * 4301
    let invalid = Err(ExtractError::Rejected(Rejection::InvalidValue));
    assert_eq!(colspan_of(&"0".repeat(4301)), invalid);
    assert_eq!(colspan_of(&"\u{663}".repeat(4301)), invalid);
    assert_eq!(colspan_of(&"1".repeat(4301)), invalid);
}

/// `handle_table(etree.fromstring(source), {*TAG_CATALOG, "table"}, Extractor(recall=recall))`:
/// the result and the source after the call.
fn table_with(source: &str, recall: bool) -> Result<(Option<String>, String), ExtractError> {
    let mut tree = xml(source);
    let root = tree.root();
    let options = options_with_recall(recall);
    let call = Call::default();
    let tags: Vec<&str> = TAG_CATALOG.iter().copied().chain(["table"]).collect();
    let processed = handle_table(&mut tree, root, &tags, &ctx_for(&options, &call))?;
    Ok((processed.map(|id| show(&tree, id)), show(&tree, root)))
}

fn table(source: &str) -> Option<String> {
    table_with(source, false).unwrap().0
}

#[test]
fn table_colspan_zero_row() {
    // oracle: etree.tostring(handle_table(etree.fromstring(s), {*TAG_CATALOG, "table"}, Extractor()))
    // colspan 0 adds no cell and counts for nothing in max_cols
    assert_eq!(
        table("<table><tr><td colspan=\"0\">a</td><td>b</td></tr></table>").as_deref(),
        Some("<table><row><cell>a</cell><cell>b</cell></row></table>")
    );
    assert_eq!(
        table("<table><tr><td colspan=\"0\">a</td></tr><tr><td>b</td><td>c</td></tr></table>")
            .as_deref(),
        Some(
            "<table><row><cell>a</cell><cell/></row><row><cell>b</cell><cell>c</cell></row></table>"
        )
    );
}

#[test]
fn table_colspan_pads_up_to_the_cap() {
    // oracle: len(handle_table(etree.fromstring(s), ...)[0]) is 100 for
    // s = '<table><tr><td colspan="500">a</td></tr></table>'
    let processed = table("<table><tr><td colspan=\"500\">a</td></tr></table>").unwrap();
    assert_eq!(processed.matches("<cell").count(), 100);
    // oracle: a colspan row shorter than the widest row is padded to it
    assert_eq!(
        table(
            "<table><tr><td colspan=\"2\">a</td></tr><tr><td>b</td><td>c</td><td>d</td></tr></table>"
        )
        .as_deref(),
        Some(
            "<table><row><cell>a</cell><cell/><cell/></row><row><cell>b</cell><cell>c</cell><cell>d</cell></row></table>"
        )
    );
}

#[test]
fn table_span_beyond_the_digit_limit_is_invalid_value() {
    // oracle: handle_table raises ValueError for colspan or rowspan of "1" * 4301
    let invalid = Err(ExtractError::Rejected(Rejection::InvalidValue));
    for attr in ["colspan", "rowspan"] {
        let source = format!(
            "<table><tr><td {attr}=\"{}\">a</td></tr></table>",
            "1".repeat(4301)
        );
        assert_eq!(table_with(&source, false), invalid, "{attr}");
    }
}

#[test]
fn table_rowspan_phantoms() {
    // oracle: etree.tostring(handle_table(etree.fromstring(s), {*TAG_CATALOG, "table"}, Extractor()))
    let cases = [
        // a rowspan leaves a placeholder in the continuation row
        (
            "<table><tr><td rowspan=\"2\">x</td><td>a</td></tr><tr><td>b</td></tr></table>",
            "<table><row><cell>x</cell><cell>a</cell></row><row><cell/><cell>b</cell></row></table>",
        ),
        // rowspan and colspan occupy every spanned column
        (
            "<table><tr><td rowspan=\"2\" colspan=\"2\">big</td><td>c</td></tr><tr><td>x</td></tr></table>",
            "<table><row><cell>big</cell><cell/><cell>c</cell></row><row><cell/><cell/><cell>x</cell></row></table>",
        ),
        // the placeholder lasts rowspan - 1 rows, then the column is free
        (
            "<table><tr><td>a</td><td rowspan=\"3\">b</td></tr><tr><td>c</td></tr><tr><td>d</td></tr><tr><td>e</td></tr></table>",
            "<table><row><cell>a</cell><cell>b</cell></row><row><cell>c</cell><cell/></row><row><cell>d</cell><cell/></row><row><cell>e</cell><cell/></row></table>",
        ),
        // an empty row consumes one row of a pending rowspan and is dropped
        (
            "<table><tr><td rowspan=\"2\">x</td></tr><tr></tr><tr><td>y</td></tr></table>",
            "<table><row><cell>x</cell></row><row><cell>y</cell></row></table>",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(table(source).as_deref(), Some(expected), "{source}");
    }
}

#[test]
fn table_header_row_once() {
    // oracle: etree.tostring(handle_table(etree.fromstring(s), {*TAG_CATALOG, "table"}, Extractor()))
    // only the first row holding a th is a header row; its th cells are the head cells
    let cases = [
        (
            "<table><tr><th>a</th></tr><tr><th>b</th><td>c</td></tr></table>",
            "<table><row><cell role=\"head\">a</cell><cell/></row><row><cell>b</cell><cell>c</cell></row></table>",
        ),
        (
            "<table><tr><td>a</td><th>b</th></tr><tr><th>c</th><th>d</th></tr></table>",
            "<table><row><cell>a</cell><cell role=\"head\">b</cell></row><row><cell>c</cell><cell>d</cell></row></table>",
        ),
    ];
    for (source, expected) in cases {
        assert_eq!(table(source).as_deref(), Some(expected), "{source}");
    }
}

#[test]
fn table_captions() {
    // oracle: r = handle_table(t, TAG_CATALOG, Extractor()) for t = etree.fromstring(s); the pairs
    // are etree.tostring(r) and etree.tostring(t)
    let (processed, source) = table_with(
        "<table><caption> Cap <b>x</b> </caption><tr><td>a</td><td>b</td></tr></table>",
        false,
    )
    .unwrap();
    assert_eq!(
        processed.as_deref(),
        Some(
            "<table><row><cell role=\"head\">Cap  x</cell><cell/></row><row><cell>a</cell><cell>b</cell></row></table>"
        )
    );
    assert_eq!(
        source,
        "<table><done> Cap <done>x</done> </done><done><done>a</done><done>b</done></done></table>"
    );
    // an image in a caption stays for the main loop, without its tail
    let (processed, source) = table_with(
        "<table><caption>C<graphic src=\"a.jpg\"/>t</caption><tr><td>a</td></tr></table>",
        false,
    )
    .unwrap();
    assert_eq!(
        processed.as_deref(),
        Some("<table><row><cell role=\"head\">C t</cell></row><row><cell>a</cell></row></table>")
    );
    assert_eq!(
        source,
        "<table><done>C<graphic src=\"a.jpg\"/></done><done><done>a</done></done></table>"
    );
    // each caption is a row of its own, wherever it sits
    assert_eq!(
        table("<table><caption>One</caption><caption>Two</caption><tr><td>a</td></tr></table>")
            .as_deref(),
        Some(
            "<table><row><cell role=\"head\">One</cell></row><row><cell role=\"head\">Two</cell></row><row><cell>a</cell></row></table>"
        )
    );
    assert_eq!(
        table("<table><tr><td>a</td></tr><caption>Late</caption></table>").as_deref(),
        Some("<table><row><cell role=\"head\">Late</cell></row><row><cell>a</cell></row></table>")
    );
}

#[test]
fn table_cell_content() {
    // oracle: r = handle_table(t, TAG_CATALOG, Extractor()) for t = etree.fromstring(s); the pairs
    // are etree.tostring(r) and etree.tostring(t)
    // a line break before text becomes a paragraph, inline elements are carried
    let (processed, source) =
        table_with("<table><tr><td>x<hi>y</hi><lb/>z</td></tr></table>", false).unwrap();
    assert_eq!(
        processed.as_deref(),
        Some("<table><row><cell>x<hi>y</hi><p>z</p></cell></row></table>")
    );
    assert_eq!(
        source,
        "<table><done><done>x<done>y</done><done/>z</done></done></table>"
    );
    // a link wrapping inline content is carried as a subtree
    let (processed, source) = table_with(
        "<table><tr><td><ref><hi>link</hi></ref></td></tr></table>",
        false,
    )
    .unwrap();
    assert_eq!(
        processed.as_deref(),
        Some("<table><row><cell><ref><hi>link</hi></ref></cell></row></table>")
    );
    assert_eq!(
        source,
        "<table><done><done><done><done>link</done></done></done></done></table>"
    );
    // nested tables are left in the source for the main loop and their tails join the cell
    let (processed, source) = table_with(
        "<table><tr><td>a<table><tr><td>b</td></tr></table>t1<table><tr><td>c</td></tr></table>t2</td></tr></table>",
        false,
    )
    .unwrap();
    assert_eq!(
        processed.as_deref(),
        Some("<table><row><cell>a t1 t2</cell></row></table>")
    );
    assert_eq!(
        source,
        "<table><done><done>a<table><tr><td>b</td></tr></table><table><tr><td>c</td></tr></table></done></done></table>"
    );
    // an unknown element between rows is renamed and left out
    let (processed, source) =
        table_with("<table><tr><td>a</td></tr><svg/></table>", false).unwrap();
    assert_eq!(
        processed.as_deref(),
        Some("<table><row><cell>a</cell></row></table>")
    );
    assert_eq!(source, "<table><done><done>a</done></done><done/></table>");
}

#[test]
fn table_cell_foreign_element() {
    // oracle: p = parse_document('<html><body><table><tr><td>a <svg><title>t</title></svg> b</td>
    // </tr></table></body></html>') after tree_cleaning and convert_tags;
    // r = handle_table(p.find('.//table'), TAG_CATALOG, Extractor()); dump_tree(r) and dump_tree(p).
    // The foreign elements and their tail are dropped, as handle_textelem finds no handler for them
    let (mut tree, table) = converted(
        "<html><body><table><tr><td>a <svg><title>t</title></svg> b</td></tr></table></body></html>",
        "table",
    );
    let options = options();
    let call = Call::default();
    let processed = handle_table(&mut tree, table, TAG_CATALOG, &ctx_for(&options, &call))
        .unwrap()
        .unwrap();
    assert_eq!(
        dump_from(&tree, processed, DumpMode::Exact).0,
        "table\n  row\n    cell\n      #text \"a \"\n"
    );
    assert_eq!(
        dump(&tree, DumpMode::Exact).0,
        "html\n  body\n    table\n      done\n        done\n          #text \"a \"\n          done\n            done\n              #text \"t\"\n          #text \" b\"\n"
    );
}

#[test]
fn table_walk_checks_cancellation() {
    let options = options();
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    let mut tree = xml("<table><tr><td>a<hi>b</hi></td></tr></table>");
    let root = tree.root();
    assert_eq!(
        handle_table(&mut tree, root, TAG_CATALOG, &ctx_for(&options, &call)),
        Err(ExtractError::Cancelled)
    );
}

/// `handle_textelem(element, potential_tags, Extractor())` where `element` is the root of
/// `etree.fromstring(source)`, or its first child.
fn textelem(source: &str, on_child: bool, potential_tags: &[&str]) -> Option<String> {
    let mut tree = xml(source);
    let element = if on_child {
        tree.first_child(tree.root()).unwrap()
    } else {
        tree.root()
    };
    let options = options();
    let call = Call::default();
    let processed = handle_textelem(
        &mut tree,
        element,
        potential_tags,
        &ctx_for(&options, &call),
    )
    .unwrap();
    processed.map(|id| show(&tree, id))
}

#[test]
fn textelem_dispatches_on_the_tag() {
    // oracle: etree.tostring(handle_textelem(element, potential_tags, Extractor())), None where
    // there is no element
    let catalog = TAG_CATALOG;
    let with_table: Vec<&str> = catalog.iter().copied().chain(["table"]).collect();
    let cases: [(&str, bool, &[&str], Option<&str>); 15] = [
        (
            "<list><item>a</item></list>",
            false,
            catalog,
            Some("<list><item>a</item></list>"),
        ),
        ("<quote>q</quote>", false, catalog, Some("<quote>q</quote>")),
        ("<head>h</head>", false, catalog, Some("<head>h</head>")),
        (
            "<p>p <hi>x</hi></p>",
            false,
            catalog,
            Some("<p>p <hi>x</hi></p>"),
        ),
        (
            "<div><lb/>tail text</div>",
            true,
            catalog,
            Some("<p>tail text</p>"),
        ),
        ("<div><lb/> </div>", true, catalog, None),
        ("<div><lb/></div>", true, catalog, None),
        (
            "<div><hi>x</hi></div>",
            true,
            catalog,
            Some("<p><hi>x</hi></p>"),
        ),
        ("<item><hi>x</hi></item>", true, catalog, Some("<hi>x</hi>")),
        ("<table><tr><td>a</td></tr></table>", false, catalog, None),
        (
            "<table><tr><td>a</td></tr></table>",
            false,
            &with_table,
            Some("<table><row><cell>a</cell></row></table>"),
        ),
        (
            "<graphic src=\"x.jpg\"/>",
            false,
            &["graphic"],
            Some("<graphic src=\"x.jpg\"/>"),
        ),
        ("<graphic src=\"x.jpg\"/>", false, &[], None),
        ("<div>text</div>", false, &["div"], Some("<p>text</p>")),
        ("<div>text</div>", false, &[], None),
    ];
    for (source, on_child, tags, expected) in cases {
        assert_eq!(
            textelem(source, on_child, tags).as_deref(),
            expected,
            "{source}"
        );
    }
}

/// Fills `outer` with `levels` nested `list > item` pairs, each item holding text.
fn list_chain(tree: &mut Tree, outer: NodeId, levels: usize) {
    let mut list = outer;
    for level in 0..levels {
        let item = tree.new_element("item");
        tree.node_mut(item).text = Some("x".to_string());
        tree.append(list, item);
        if level + 1 < levels {
            let inner = tree.new_element("list");
            tree.append(item, inner);
            list = inner;
        }
    }
}

fn new_tree(tag: &str) -> (Tree, NodeId) {
    let tree = Tree::new(LocalName::from(tag), Ns::Html, Vec::new());
    let root = tree.root();
    (tree, root)
}

#[test]
fn deep_nesting_fits_512k_stack() {
    let worker = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            let options = options();
            let call = Call::default();
            let ctx = ctx_for(&options, &call);
            let tags: Vec<&str> = TAG_CATALOG.iter().copied().chain(["table"]).collect();

            // 127 list levels, the deepest a 256-level tree holds
            let (mut tree, root) = new_tree("list");
            list_chain(&mut tree, root, 127);
            assert!(handle_lists(&mut tree, root, &ctx).unwrap().is_some());

            // 255 inline levels below a quote
            let (mut tree, root) = new_tree("quote");
            let mut parent = root;
            for _ in 0..255 {
                let hi = tree.new_element("hi");
                tree.node_mut(hi).text = Some("x".to_string());
                tree.append(parent, hi);
                parent = hi;
            }
            assert!(handle_quotes(&mut tree, root, &ctx).unwrap().is_some());

            // a list chain entered from a quote
            let (mut tree, root) = new_tree("quote");
            let outer = tree.new_element("list");
            tree.append(root, outer);
            list_chain(&mut tree, outer, 100);
            assert!(
                handle_textelem(&mut tree, root, &tags, &ctx)
                    .unwrap()
                    .is_some()
            );

            // a list chain entered from a table cell; only recall mode keeps a list there
            let recall = options_with_recall(true);
            let ctx = ctx_for(&recall, &call);
            let (mut tree, root) = new_tree("table");
            let row = tree.new_element("tr");
            tree.append(root, row);
            let cell = tree.new_element("td");
            tree.append(row, cell);
            let outer = tree.new_element("list");
            tree.append(cell, outer);
            list_chain(&mut tree, outer, 100);
            assert!(
                handle_textelem(&mut tree, root, &tags, &ctx)
                    .unwrap()
                    .is_some()
            );
        })
        .unwrap();
    worker.join().unwrap();
}
