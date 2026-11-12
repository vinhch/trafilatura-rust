use super::*;

fn options_with(configure: impl FnOnce(&mut Extractor)) -> Extractor {
    let mut options = options();
    configure(&mut options);
    options
}

/// The html5 tree of `html` after `tree_cleaning` and `convert_tags` under `options`, and the root
/// `tree_cleaning` returns.
fn cleaned(html: &str, options: &Extractor) -> (Tree, NodeId) {
    let mut tree = parse_document(html, &ParseLimits::default(), &Call::default()).unwrap();
    let root = tree.root();
    let call = Call::default();
    let ctx = ctx_for(options, &call);
    let root = tree_cleaning(&mut tree, root, &ctx).unwrap();
    convert_tags(&mut tree, root, &ctx, None).unwrap();
    (tree, root)
}

/// What `extract_body` returns for `tree`: the body, its text and whether `div` joined the
/// potential tags.
fn body_in(tree: &mut Tree, root: NodeId, options: &Extractor) -> (String, String, bool) {
    let call = Call::default();
    let (body, text, tags) = extract_body(tree, root, &ctx_for(options, &call)).unwrap();
    (show(tree, body), text, tags.contains(&"div"))
}

fn body_of(html: &str, options: &Extractor) -> (String, String, bool) {
    let (mut tree, root) = cleaned(html, options);
    body_in(&mut tree, root, options)
}

fn content_of(html: &str, options: &Extractor) -> (String, String) {
    let (mut tree, root) = cleaned(html, options);
    let call = Call::default();
    let (body, text) = extract_content(&mut tree, root, &ctx_for(options, &call)).unwrap();
    (show(&tree, body), text)
}

/// The comments body, its text and the page after the call.
fn comments_of(html: &str, options: &Extractor) -> (String, String, String) {
    let (mut tree, root) = cleaned(html, options);
    let call = Call::default();
    let (body, text) = extract_comments(&mut tree, root, &ctx_for(options, &call)).unwrap();
    (show(&tree, body), text, show(&tree, root))
}

const OUTSIDE_PARAGRAPH: &str =
    "This outside paragraph is deliberately long enough to pass the size gate. ";

#[test]
fn ptest_reads_whole_document() {
    // oracle: _extract(convert_tags(tree_cleaning(parse_document(page))), Extractor(fast=True)) with
    // min_extracted_size = 30 and page = <html><body><div class="post"><p>Short one.</p>
    // <p>Short two.</p></div><p>OUTSIDE * 12</p></body></html>: the frame's own paragraphs are 20
    // code points, the whole document's 20 + 900, so `div` is not added
    let page = format!(
        "<html><body><div class=\"post\"><p>Short one.</p><p>Short two.</p></div><p>{}</p></body></html>",
        OUTSIDE_PARAGRAPH.repeat(12)
    );
    let options = options_with(|options| options.min_extracted_size = 30);
    assert_eq!(
        body_of(&page, &options),
        (
            "<body><p>Short one.</p><p>Short two.</p></body>".to_string(),
            "Short one. Short two.".to_string(),
            false
        )
    );
}

#[test]
fn ptest_reads_document_of_detached_frame() {
    // oracle: _extract(...) with Extractor(fast=True, links=True) on <html><body><p>OUTSIDE * 12</p>
    // <div class="entry-content">Intro words LINKS</div></body></html>, LINKS eight
    // `<a href="/lN">link number N</a> `: link density deletes the frame from the document and
    // `_extract` goes on with it; `//p//text()` still reads the document, so `div` is not added
    // and every link is its own paragraph
    let links: String = (0..8)
        .map(|n| format!("<a href=\"/l{n}\">link number {n}</a> "))
        .collect();
    let options = options_with(|options| options.links = true);
    let page = format!(
        "<html><body><p>{}</p><div class=\"entry-content\">Intro words {links}</div></body></html>",
        OUTSIDE_PARAGRAPH.repeat(12)
    );
    let paragraphs: String = (0..8)
        .map(|n| format!("<p><ref target=\"/l{n}\">link number {n}</ref></p>"))
        .collect();
    let text: String = (0..8).map(|n| format!(" link number {n}")).collect();
    assert_eq!(
        body_of(&page, &options),
        (
            format!("<body><p>Intro words</p>{paragraphs}</body>"),
            format!("Intro words{text}"),
            false
        )
    );
    // oracle: the same page without the outside paragraph: `div` joins and the frame is one paragraph
    let page =
        format!("<html><body><div class=\"entry-content\">Intro words {links}</div></body></html>");
    let (body, _, has_div) = body_of(&page, &options);
    assert!(has_div);
    assert_eq!(body.matches("<p>").count(), 1);
}

#[test]
fn ptest_empty_node_set_adds_div_at_zero_size() {
    // oracle: _extract(...) with min_extracted_size = 0 on the page below: no `p`, no text node,
    // so `not ptest` adds `div` although `0 < 0` is false
    let options = options_with(|options| options.min_extracted_size = 0);
    assert_eq!(
        body_of(
            "<html><body><article><div>Some words in a div here</div><lb/></article></body></html>",
            &options
        ),
        (
            "<body><p>Some words in a div here</p></body>".to_string(),
            "Some words in a div here".to_string(),
            true
        )
    );
}

#[test]
fn ptest_empty_text_node_counts_as_a_node() {
    // oracle: _extract(...) with min_extracted_size = 0 on the page below after
    // `tree.find(".//p").text = ""` gives <body/>, '' and no `div`; with `= None` it gives
    // <body><p>Some words in a div here</p></body> and `div`
    let options = options_with(|options| options.min_extracted_size = 0);
    let page = "<html><body><article><p>x</p><div>Some words in a div here</div><lb/></article></body></html>";
    for (text, expected, has_div) in [
        (Some(String::new()), "<body/>", false),
        (None, "<body><p>Some words in a div here</p></body>", true),
    ] {
        let (mut tree, root) = cleaned(page, &options);
        let paragraph = tree.find(root, "p").unwrap();
        tree.node_mut(paragraph).text = text;
        let (body, _, div_added) = body_in(&mut tree, root, &options);
        assert_eq!((body.as_str(), div_added), (expected, has_div));
    }
}

#[test]
fn frame_with_foreign_element_is_not_single_run() {
    // oracle: _extract(...) with min_extracted_size = 0 and formatting=True; an svg is in `.//*`,
    // so `tags <= INLINE_FORMATTABLE | {"lb"}` fails and the frame is walked element by element
    let tight = options_with(|options| {
        options.min_extracted_size = 0;
        options.formatting = true;
    });
    let plain = "<html><body><div class=\"post\">Lead text <b>bold</b> text <i>it</i> more</div></body></html>";
    assert_eq!(
        body_of(plain, &tight).0,
        "<body><p>Lead text <hi rend=\"#b\">bold</hi> text <hi rend=\"#i\">it</hi> more</p></body>"
    );
    let foreign = "<html><body><div class=\"post\">Lead text <b>bold</b> text <svg><title>t</title></svg> <i>it</i> more</div></body></html>";
    // oracle: scripts/oracle.sh, _extract(tree_cleaned, Extractor(min size 0, formatting)) on the foreign page:
    // the svg is an unknown element on the libxml2 tree, so the paragraph is one run with a double space
    assert_eq!(
        body_of(foreign, &tight).0,
        "<body><p>Lead text <hi rend=\"#b\">bold</hi> text  <hi rend=\"#i\">it</hi> more</p></body>"
    );
    // oracle: the first form, `tags == {"lb"}`, with a long outside paragraph so `div` is not
    // added: a frame of line breaks is one run (nothing handles it), with an svg it is not
    let outside = OUTSIDE_PARAGRAPH.repeat(12);
    let options = options();
    assert_eq!(
        body_of(
            &format!("<html><body><p>{outside}</p><div class=\"post\">text one<br>text two</div></body></html>"),
            &options
        )
        .0,
        "<body/>"
    );
    assert_eq!(
        body_of(
            &format!("<html><body><p>{outside}</p><div class=\"post\">text one<br>text two<svg><title>t</title></svg></div></body></html>"),
            &options
        )
        .0,
        "<body/>"
    );
}

#[test]
fn lead_text_inserted_only_when_frame_accepted() {
    // oracle: _extract(...) on each frame, default options
    let options = options();
    assert_eq!(
        body_of(
            "<html><body><div class=\"post\">Lead words here<p>First paragraph with text.</p><p>Second paragraph text.</p></div></body></html>",
            &options
        )
        .0,
        concat!(
            "<body><p>Lead words here</p><p>First paragraph with text.</p>",
            "<p>Second paragraph text.</p></body>"
        )
    );
    assert_eq!(
        body_of(
            "<html><body><div class=\"post\">Lead words here<p>Only one paragraph.</p></div></body></html>",
            &options
        )
        .0,
        "<body><p>Only one paragraph.</p></body>"
    );
}

#[test]
fn rebuilt_block_tail_becomes_paragraph() {
    // oracle: _extract(...) on the frame below, default options: the text after the quote and the
    // list is a paragraph of its own
    let (body, text, _) = body_of(
        concat!(
            "<html><body><div class=\"post\"><p>One para text here.</p>",
            "<blockquote>quoted words</blockquote>tail after the quote",
            "<ul><li>item</li></ul>tail after list<p>Another para.</p></div></body></html>"
        ),
        &options(),
    );
    assert_eq!(
        body,
        concat!(
            "<body><p>One para text here.</p><quote>quoted words</quote>",
            "<p>tail after the quote</p><list rend=\"ul\"><item>item</item></list>",
            "<p>tail after list</p><p>Another para.</p></body>"
        )
    );
    assert_eq!(
        text,
        "One para text here. quoted words tail after the quote item tail after list Another para."
    );
}

#[test]
fn trailing_heads_removed() {
    // oracle: _extract(...) on the frame below, default options
    assert_eq!(
        body_of(
            "<html><body><div class=\"post\"><p>Para one text.</p><p>Para two text.</p><h2>Trailing heading</h2></div></body></html>",
            &options()
        ),
        (
            "<body><p>Para one text.</p><p>Para two text.</p></body>".to_string(),
            "Para one text. Para two text.".to_string(),
            true
        )
    );
}

#[test]
fn lb_whitespace_tail_cleared_outside_code() {
    // oracle: extract_content(...) with min_extracted_size = 0 on each page. Stripping the nested
    // `div` leaves "\n    \n   " as the tail of the `lb`: cleared in a paragraph, kept in code. The
    // text is taken before the cleanup.
    let options = options_with(|options| options.min_extracted_size = 0);
    assert_eq!(
        content_of(
            "<html><body><article><div>Words in the outer block<br>\n   <div class=\"clearfix\"> </div>\n   </div><div>Second block words here</div></article></body></html>",
            &options
        ),
        (
            "<body><p>Words in the outer block<lb/></p><p>Second block words here</p></body>"
                .to_string(),
            "Words in the outer block \n      \n    Second block words here".to_string()
        )
    );
    assert_eq!(
        content_of(
            "<html><body><article><pre>function() { x }<br>\n   <div class=\"clearfix\"> </div>\n   </pre><div>Second block words here</div></article></body></html>",
            &options
        ),
        (
            "<body><code>function() { x }<lb/>\n    \n   </code><p>Second block words here</p></body>"
                .to_string(),
            "function() { x } \n      \n    Second block words here".to_string()
        )
    );
}

#[test]
fn recovery_gate_runs_on_empty_body() {
    // oracle: extract_content(...) with min_extracted_size = 0, so only the empty body opens the
    // recovery gate: the page has no BODY_XPATH frame
    let options = options_with(|options| options.min_extracted_size = 0);
    assert_eq!(
        content_of(
            "<html><body><div><p>Wild paragraph one outside any frame.</p><p>Wild paragraph two outside any frame.</p></div></body></html>",
            &options
        ),
        (
            concat!(
                "<body><p>Wild paragraph one outside any frame.</p>",
                "<p>Wild paragraph two outside any frame.</p></body>"
            )
            .to_string(),
            "Wild paragraph one outside any frame. Wild paragraph two outside any frame."
                .to_string()
        )
    );
}

#[test]
fn recovery_gate_runs_on_short_body_and_not_at_the_minimum() {
    // oracle: extract_content(...) with min_extracted_size unset, 36 and 37. _extract keeps the two
    // short paragraphs ("Short first line. Short second line.", 36 characters): recovery runs on a
    // non-empty body below the minimum, not at it
    let page = "<html><body><article><p>Short first line.</p><p>Short second line.</p></article><div><p>Wild text after the article frame, long enough to count.</p></div></body></html>";
    let framed = (
        "<body><p>Short first line.</p><p>Short second line.</p></body>".to_string(),
        "Short first line. Short second line.".to_string(),
    );
    let recovered = (
        concat!(
            "<body><p>Short first line.</p><p>Short second line.</p>",
            "<p>Wild text after the article frame, long enough to count.</p></body>"
        )
        .to_string(),
        "Short first line. Short second line. Wild text after the article frame, long enough to count."
            .to_string(),
    );
    assert_eq!(body_of(page, &options()).1.chars().count(), 36);
    assert_eq!(content_of(page, &options()), recovered);
    let at_minimum = options_with(|options| options.min_extracted_size = 36);
    assert_eq!(content_of(page, &at_minimum), framed);
    let above_minimum = options_with(|options| options.min_extracted_size = 37);
    assert_eq!(content_of(page, &above_minimum), recovered);
}

#[test]
fn done_tail_stripped_from_result_body() {
    // oracle: extract_content(...) with min_extracted_size = 0 and formatting=True. The `post` frame
    // copies its paragraph (the source becomes `done`); the wider `article` frame then moves the
    // text-bearing wrapper div whole, `done` source and its tail inside. The returned text is taken
    // before the cleanup, so it still holds the tail
    let options = options_with(|options| {
        options.min_extracted_size = 0;
        options.formatting = true;
    });
    let (body, text) = content_of(
        "<html><body><article><div>Wrapper text <div class=\"post\"><p>Narrow <b>bold</b> paragraph.</p>Tail text after done.</div></div></article></body></html>",
        &options,
    );
    assert_eq!(
        body,
        "<body><p>Narrow <hi rend=\"#b\">bold</hi> paragraph.</p><p>Wrapper text </p></body>"
    );
    assert_eq!(
        text,
        "Narrow  bold  paragraph. Wrapper text  Narrow  bold  paragraph. Tail text after done."
    );
}

#[test]
fn non_div_frame_with_inline_children_is_walked() {
    // oracle: _extract(...) with min_extracted_size = 0 and formatting=True; an `article` frame of
    // inline children is not a single run (only a `div` frame is), so each inline element is walked
    let options = options_with(|options| {
        options.min_extracted_size = 0;
        options.formatting = true;
    });
    assert_eq!(
        body_of(
            "<html><body><article>Lead text <b>bold</b> text <i>it</i> more</article></body></html>",
            &options
        )
        .0,
        concat!(
            "<body><p>Lead text</p><p><hi rend=\"#b\">bold</hi>text</p>",
            "<p><hi rend=\"#i\">it</hi>more</p></body>"
        )
    );
}

#[test]
fn adjacent_duplicate_dropped_only_above_min_length() {
    // oracle: extract_content(...) on a frame of the same paragraph twice and a unique one
    let long = "A paragraph repeated twice in a row, longer than fifty characters.";
    let short = "Short repeat.";
    assert!(long.chars().count() > MIN_DUPLICATE_LENGTH);
    assert!(short.chars().count() <= MIN_DUPLICATE_LENGTH);
    let page = |text: &str| {
        format!(
            "<html><body><div class=\"post\"><p>{text}</p><p>{text}</p><p>Another unique paragraph.</p></div></body></html>"
        )
    };
    let options = options();
    assert_eq!(
        content_of(&page(long), &options).0,
        format!("<body><p>{long}</p><p>Another unique paragraph.</p></body>")
    );
    assert_eq!(
        content_of(&page(short), &options).0,
        format!("<body><p>{short}</p><p>{short}</p><p>Another unique paragraph.</p></body>")
    );
}

#[test]
fn comments_text_keeps_empty_tail_space() {
    // oracle: extract_comments(...) on the page below: `handle_textnode` leaves the former `lb`
    // with tail "", which `" ".join` counts as a piece
    let (body, text, _) = comments_of(
        "<html><body><div id='comments'><p>One comment here</p><br>Second line<p>third</p></div></body></html>",
        &options(),
    );
    assert_eq!(
        body,
        "<body><p>One comment here</p><p>Second line</p><p>third</p></body>"
    );
    assert_eq!(text, "One comment here Second line  third");
}

#[test]
fn comments_subtree_is_deleted_and_inline_tags_stripped() {
    // oracle: extract_comments(...) with links=True: `ref` and `span` are stripped, the attributes
    // cleared and the matched section deleted from the page
    let options = options_with(|options| options.links = true);
    let (body, text, page) = comments_of(
        "<html><body><p>keep this</p><div id='comments'><p class='c1'>First comment text</p><p>Second <span>comment</span> <a href='/x'>link</a> text</p></div></body></html>",
        &options,
    );
    assert_eq!(
        body,
        "<body><p>First comment text</p><p>Second comment link text</p></body>"
    );
    assert_eq!(text, "First comment text Second comment link text");
    assert_eq!(page, "<html><body><p>keep this</p></body></html>");
    // a discarded block is not extracted
    let (body, _, _) = comments_of(
        "<html><body><p>keep this</p><div id='comments'><p>First comment text</p><div class='comments-title'>Discard</div></div></body></html>",
        &options,
    );
    assert_eq!(body, "<body><p>First comment text</p></body>");
    // an empty section leaves the page alone
    let (body, text, page) = comments_of(
        "<html><body><p>keep this</p><div id='comments'>   </div></body></html>",
        &options,
    );
    assert_eq!((body.as_str(), text.as_str()), ("<body/>", ""));
    assert_eq!(
        page,
        "<html><body><p>keep this</p><div id=\"comments\">   </div></body></html>"
    );
    // a match inside an earlier match is moved out with it
    let (body, text, page) = comments_of(
        "<html><body><div class='comments'><div class='comment'><p>Outer text</p><p>Inner text</p></div></div></body></html>",
        &options,
    );
    assert_eq!(body, "<body><p>Outer text</p><p>Inner text</p></body>");
    assert_eq!(text, "Outer text Inner text");
    assert_eq!(page, "<html><body/></html>");
}

#[test]
fn extract_entry_points_check_cancellation() {
    let options = options();
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    let html = "<html><body><div class=\"post\"><p>One.</p><p>Two.</p></div></body></html>";
    let (mut tree, root) = cleaned(html, &options);
    assert_eq!(
        extract_body(&mut tree, root, &ctx_for(&options, &call)).map(|_| ()),
        Err(ExtractError::Cancelled)
    );
    assert_eq!(
        extract_content(&mut tree, root, &ctx_for(&options, &call)).map(|_| ()),
        Err(ExtractError::Cancelled)
    );
    assert_eq!(
        extract_comments(&mut tree, root, &ctx_for(&options, &call)).map(|_| ()),
        Err(ExtractError::Cancelled)
    );
}

const LEAD: &str = "The harbor market opens early and the fishermen carry baskets of fresh herring along the old stone quay while gulls circle above the nets and the cold morning light slowly warms the wet cobbles.";
const TRAIL: &str = "By noon the stalls are empty, the quay falls quiet again, and the last boats drift back toward the narrow mouth of the river as the tide turns and the evening wind begins to rise.";
const SHORT_NOTE: &str = "A short note about the river mill and the boats that pass it each morning, written while the fog still lay thick over the water and the first lamps were lit.";
const SVG_TITLE: &str = "<svg><title>t</title></svg>";

/// `extract` in text format on a full page, through the public API.
fn extract_txt(page: &str) -> Result<String, ExtractError> {
    let extractor = crate::api::Options::new()
        .format(crate::api::Format::Txt)
        .build();
    extractor.extract(&crate::api::Input::text(page), &Call::default())
}

/// A page whose article holds `middle` between two paragraphs.
fn article_page(middle: &str) -> String {
    format!(
        "<html><head><title>t</title></head><body><article><p>{LEAD}</p>{middle}<p>{TRAIL}</p></article></body></html>"
    )
}

#[test]
fn define_newelem_rejects_colon_tag() {
    // oracle: extract(page, options=Extractor(output_format="txt", fast=True, dedup=False)) inside
    //   html5_parsers() is None: define_newelem raises ValueError("Invalid tag name 'o:p'") (or
    //   'http:') in the main pass, which bare_extraction catches
    for middle in [
        "<ul><li><o:p>x</o:p> y</li></ul>",
        "<ul><li>a <o:p>x</o:p> y</li><li>c</li></ul>",
        "<blockquote>a <o:p>x</o:p> b</blockquote>",
        "<blockquote>a <http://example.com/x>b</blockquote>",
    ] {
        assert_eq!(
            extract_txt(&article_page(middle)),
            Err(ExtractError::Rejected(Rejection::InvalidValue)),
            "{middle}"
        );
    }
}

#[test]
fn define_newelem_drops_svg_title_text() {
    // oracle: scripts/oracle.sh, extract(page, output_format="txt", fast=True, zero min size): the svg
    // title is an unknown element on the libxml2 tree, so its text goes and the rest of the item stays
    let want = format!("{LEAD}\n- a b\n- c\n{TRAIL}");
    assert_eq!(
        extract_txt(&article_page(&format!(
            "<ul><li>a {SVG_TITLE} b</li><li>c</li></ul>"
        ))),
        Ok(want)
    );
    let want = format!("{LEAD}\na  b\n{TRAIL}");
    assert_eq!(
        extract_txt(&article_page(&format!(
            "<blockquote>a {SVG_TITLE} b</blockquote>"
        ))),
        Ok(want)
    );
}

#[test]
fn colon_tag_in_paragraph_is_stripped() {
    // oracle: the same call returns the text below: handle_paragraphs strips `o:p` before any
    //   element is built, so nothing raises
    let page = article_page(
        "<p>a <o:p>x</o:p> b, and then some more words so that the paragraph is long enough to stay.</p>",
    );
    let want = format!(
        "{LEAD}\na x b, and then some more words so that the paragraph is long enough to stay.\n{TRAIL}"
    );
    assert_eq!(extract_txt(&page), Ok(want));
}

#[test]
fn recall_retry_rejection_keeps_first_pass() {
    // oracle: the same call returns SHORT_NOTE. The first pass keeps only the paragraph; the recall
    //   retry (core._recall_retry, patched to record the raise) meets `<o:p>` in the list on the
    //   fallback branch of process_nested_elements and raises ValueError, which its caller
    //   swallows
    let fill = "The quiet river carries small boats past the old mill. ".repeat(55);
    let page = format!(
        "<html><head><title>t</title></head><body><div><p>{SHORT_NOTE}</p></div><div class='x'><ul><li><o:p>x</o:p> more words here</li></ul>{fill}</div></body></html>"
    );
    assert_eq!(extract_txt(&page), Ok(SHORT_NOTE.to_string()));
}

/// The elements `recover_wild_text` leaves in a body that starts with one `p` per `existing` text.
fn recovered_after(page: &str, existing: &[&str]) -> Vec<String> {
    let mut tree = parse_document(page, &ParseLimits::default(), &Call::default()).unwrap();
    let root = tree.root();
    let body = tree.new_element("body");
    for text in existing {
        let paragraph = tree.new_element("p");
        tree.node_mut(paragraph).text = Some(text.to_string());
        tree.append(body, paragraph);
    }
    let options = options();
    let call = Call::default();
    recover_wild_text(&mut tree, root, body, &ctx_for(&options, &call), None, None).unwrap();
    tree.children(body).map(|el| show(&tree, el)).collect()
}

#[test]
fn adjacent_duplicate_of_exactly_min_length_is_kept() {
    // oracle: extract_content(...) with Extractor(fast=True) on a frame of a 50-character paragraph
    // twice and a unique one: both copies stay
    let repeated = "Fifty characters exactly, repeated in a row, okay.";
    assert_eq!(repeated.chars().count(), MIN_DUPLICATE_LENGTH);
    let page = format!(
        "<html><body><div class=\"post\"><p>{repeated}</p><p>{repeated}</p><p>Another unique paragraph.</p></div></body></html>"
    );
    assert_eq!(
        content_of(&page, &options()),
        (
            format!(
                "<body><p>{repeated}</p><p>{repeated}</p><p>Another unique paragraph.</p></body>"
            ),
            format!("{repeated} {repeated} Another unique paragraph.")
        )
    );
}

#[test]
fn recovered_substring_of_exactly_min_length_is_kept() {
    // oracle: recover_wild_text(parse_document(page), etree.Element("body"), Extractor(fast=True))
    // with page = <html><body><div><p>BIG</p></div><div><p>SUB</p></div></body></html>, SUB a
    // 50-character substring of BIG: both paragraphs are recovered
    let big =
        "The old harbour market opens early and the fishermen carry baskets of fresh herring.";
    let sub = "harbour market opens early and the fishermen carry";
    assert_eq!(sub.chars().count(), MIN_DUPLICATE_LENGTH);
    let page = format!("<html><body><div><p>{big}</p></div><div><p>{sub}</p></div></body></html>");
    assert_eq!(
        recovered_after(&page, &[]),
        [format!("<p>{big}</p>"), format!("<p>{sub}</p>")]
    );
}

#[test]
fn recovered_text_spanning_two_existing_elements_is_kept() {
    // oracle: recover_wild_text(parse_document(page), body, Extractor(fast=True)) with a body of
    // <p>A</p><p>B</p> and page = <html><body><div><p>AB</p></div></body></html>: the existing
    // elements are newline-joined, so AB is no substring of them and is appended
    let first = "aaaa bbbb cccc dddd eeee ffff gggg hhhh";
    let second = "iiii jjjj kkkk llll mmmm nnnn oooo pppp";
    let page = format!("<html><body><div><p>{first}{second}</p></div></body></html>");
    assert_eq!(
        recovered_after(&page, &[first, second]),
        [
            format!("<p>{first}</p>"),
            format!("<p>{second}</p>"),
            format!("<p>{first}{second}</p>")
        ]
    );
}
