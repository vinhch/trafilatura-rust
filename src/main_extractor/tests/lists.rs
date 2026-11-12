use super::*;

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
fn list_item_svg_title_text_is_dropped() {
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
    // oracle: scripts/oracle.sh 'from lxml.html import fromstring; from parity.treedump import dump_tree;
    //   from trafilatura.utils import HTML_PARSER; from trafilatura.settings import Extractor;
    //   from trafilatura.htmlprocessing import tree_cleaning, convert_tags; from trafilatura.main_extractor import handle_lists;
    //   o = Extractor(); t = convert_tags(tree_cleaning(fromstring(<page>, parser=HTML_PARSER), o), o);
    //   print(dump_tree(handle_lists(t.find(".//list"), o))); print(dump_tree(t))'
    assert_eq!(
        dump_from(&tree, processed, DumpMode::Exact).0,
        "list rend=\"ul\"\n  item\n    #text \"a b\"\n"
    );
    assert_eq!(
        dump(&tree, DumpMode::Exact).0,
        "html\n  body\n    done rend=\"ul\"\n      done\n        #text \"a b\"\n"
    );
}

#[test]
fn quote_svg_title_text_is_dropped() {
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
    // oracle: scripts/oracle.sh 'from lxml.html import fromstring; from parity.treedump import dump_tree;
    //   from trafilatura.utils import HTML_PARSER; from trafilatura.settings import Extractor;
    //   from trafilatura.htmlprocessing import tree_cleaning, convert_tags; from trafilatura.main_extractor import handle_quotes;
    //   o = Extractor(); t = convert_tags(tree_cleaning(fromstring(<page>, parser=HTML_PARSER), o), o);
    //   print(dump_tree(handle_quotes(t.find(".//quote"), o))); print(dump_tree(t))'
    assert_eq!(
        dump_from(&tree, processed, DumpMode::Exact).0,
        "quote\n  #text \"a  b\"\n"
    );
    assert_eq!(
        dump(&tree, DumpMode::Exact).0,
        "html\n  body\n    quote\n      #text \"a  b\"\n"
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

#[test]
fn list_item_tail_replaces_whitespace_only_tail_of_last_child() {
    // oracle: handle_lists(etree.fromstring('<list><item><hi>x</hi>  </item>Tail words</list>'),
    // Extractor()): the item's tail replaces a tail without text characters instead of joining it
    assert_eq!(
        list("<list><item><hi>x</hi>  </item>Tail words</list>"),
        (
            Some("<list><item><hi>x</hi>Tail words</item></list>".to_string()),
            "<done><done><done>x</done>  </done>Tail words</done>".to_string()
        )
    );
}
