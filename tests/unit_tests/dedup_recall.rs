use super::*;

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
    tree.children(body)
        .map(|el| trim(&tree.itertext(el).collect::<String>()))
        .collect()
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

/// Shared by the recall-escalation tests: long enough to clear the minimum extraction size.
fn escalation_intro() -> String {
    format!(
        "<p>{}</p>",
        "Introductory paragraph with regular prose content that is moderately long and clearly \
         meaningful, providing enough text to exceed the minimum extraction size threshold used \
         by the extractor. "
            .repeat(2)
    )
}

const DFP_JSONLD: &str = concat!(
    r#"<script type="application/ld+json">{"@context":"https://schema.org","#,
    r#""@type":"DiscussionForumPosting","headline":"Test thread"}</script>"#
);

/// `_thread_doc`: article intro plus 8 replies in a comments-classed container. Unwrapped, there is
/// no article-region marker at all, so body selection stays broad.
fn thread_doc(jsonld: &str, reply_tag: &str, intro: &str, wrap: bool) -> String {
    let replies: String = (0..8)
        .map(|i| {
            format!(
                "<{reply_tag} class='comment-body'>Reply number {i} contains substantial discussion \
                 content with plenty of genuine words and enough length to be recognized as real text \
                 by a paragraph density classifier, not boilerplate at all today, said the \
                 commenter.</{reply_tag}>"
            )
        })
        .collect();
    let article = if wrap {
        format!("<article>{intro}</article>")
    } else {
        format!("<div>{intro}</div>")
    };
    format!(
        "<html><head>{jsonld}</head><body>{article}<div id='comments' class='comments-area'>{replies}</div></body></html>"
    )
}

#[test]
fn test_include_images_does_not_truncate() {
    // Regression #194/#842: a lead image plus one paragraph must not let _extract break early and
    // drop later content. Real config + a >250-char lead so wild-text recovery does not fire and
    // mask the early break.
    let lead = "This single lead paragraph is deliberately long enough to exceed the minimum extracted size. ".repeat(4);
    let continuation: String = (1..5)
        .map(|i| {
            format!(
                "<p>Continuation paragraph {i} that must also survive extraction in full here.</p>"
            )
        })
        .collect();
    let doc = format!(
        "<html><body><article><img src='/lead.jpg' alt='lead'><p>{lead}</p></article><div id='content'>{continuation}</div></body></html>"
    );
    let result = extract(&doc, &Kw::new().images(true)).unwrap_or_default();
    assert!(result.contains("/lead.jpg"));
    for i in 1..5 {
        assert!(
            result.contains(&format!("Continuation paragraph {i}")),
            "{i}"
        );
    }
}

#[test]
fn test_no_duplicate_content() {
    // Regression #768/#817: content must not be emitted twice (overlapping candidates / wild-text
    // recovery). Real config: ZERO_CONFIG's min_extracted_size=0 hides #817.
    let dup768 = "<!doctype html><body><main><article><div><br>Line that has to have at least 125 characters for the bug to appear so here is some filler text text text text text text text</div></article></main></body></html>";
    assert_eq!(
        extract(dup768, &Kw::new())
            .unwrap_or_default()
            .matches("Line that has to have")
            .count(),
        1
    );
    let dup817 = "<html><body><div id='content'><p>Authoritative taxonomy of but let us leave it as it is 1 2 3</p></div><p>some text long enough not to skip and printed twice on this line some text long enough not to skip and printed twice on this line</p></body></html>";
    assert_eq!(
        extract(dup817, &Kw::new())
            .unwrap_or_default()
            .matches("Authoritative taxonomy")
            .count(),
        1
    );
    // #879: a small <article>/<main> body (below min_extracted_size) triggers wild-text recovery,
    // which must not re-append the paragraphs the main pass already collected (interleaved dupes)
    let dup879 = concat!(
        "<html><body><nav>menu chrome</nav><article><h1>The Example Chronicle</h1>",
        "<p>First synthetic paragraph of adequate length for extraction to engage properly.</p>",
        "<p>Second synthetic paragraph, also long enough to matter for the extractor.</p>",
        "</article><footer>footer chrome</footer></body></html>"
    );
    for doc in [dup879.to_string(), dup879.replace("article>", "main>")] {
        let out = extract(&doc, &Kw::new()).unwrap_or_default();
        assert_eq!(out.matches("First synthetic paragraph").count(), 1);
        assert_eq!(out.matches("Second synthetic paragraph").count(), 1);
    }
}

#[test]
fn test_short_document_keeps_structure() {
    // Regression #896: the baseline rescue must not flatten a valid short extraction it cannot
    // improve on.
    let doc = concat!(
        "<html><body><article><h1>Notice Title</h1>",
        "<p>This municipal notice concerns the closure of the main road for repairs.</p>",
        "<ul><li>Detour signposted east</li><li>Works end on Friday</li></ul>",
        "</article></body></html>"
    );
    // real config: the doc is below min_extracted_size, so the rescue engages (ZERO_CONFIG hides it)
    let result = extract(doc, &Kw::new().markdown().formatting(true)).unwrap_or_default();
    assert!(result.contains("# Notice Title"));
    assert!(result.contains("- Detour signposted east"));
}

#[test]
fn test_short_document_baseline_rescue_accepted() {
    // The other side of the #896 gate: when the rescue does add text, it must still win.
    // The sentences are distinct; no cache is passed, so duplicate_test flags nothing (D-22). The empty
    // <body> keeps the main extractor at zero, so the rescue is what supplies the text.
    let body = (0..6)
        .map(|i| format!("Sentence {i} of a body that arrives only as structured data."))
        .collect::<Vec<_>>()
        .join(" ");
    let doc = format!(
        r#"<html><body><script type="application/ld+json">{{"@type": "Article", "articleBody": "{body}"}}</script></body></html>"#
    );
    assert!(
        extract(&doc, &Kw::new())
            .unwrap_or_default()
            .contains("Sentence 3 of a body")
    );
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
fn test_no_duplicate_content_nonadjacent() {
    // Regression T6 (non-adjacent case): the duplicate is not the element immediately preceding it
    // in result_body once genuine wild text is recovered in between: an adjacent-only dedup check
    // would miss it.
    let dup = format!(
        "{} short duplicate description text for the list item here right now please.",
        "X".repeat(30)
    );
    let wild = format!(
        "{} this is genuinely separate wild text living outside the article container elsewhere in the page body content over here, quite far removed from it.",
        "Y".repeat(30)
    );
    let html = format!(
        "<html><body><p>{wild}</p><article><dl><dt>Term</dt><dd><p>{dup}</p></dd></dl></article></body></html>"
    );
    let result = extract(&html, &Kw::new()).unwrap_or_default();
    assert_eq!(result.matches(&dup).count(), 1);
    assert_eq!(result.matches(&wild).count(), 1);
    assert!(result.contains("Term"));
}

#[test]
fn test_no_duplicate_content_short_elements() {
    // Regression #634: on short pages, recovery must not re-add elements the main pass already
    // extracted, even when their text is below the substring-dedup length gate.
    let body = concat!(
        r#"<p>this is sample intro</p><h3 class="wp-block-heading">intro 2</h3><p>table below</p>"#,
        r#"<figure class="wp-block-table"><table><tbody><tr><td>a</td><td>b</td><td></td></tr>"#,
        "<tr><td>f</td><td>s</td><td>s</td></tr><tr><td>g</td><td></td><td>b</td></tr></tbody></table></figure>",
        "<p>header table below</p>",
        r#"<figure class="wp-block-table"><table><thead><tr><th>b</th><th>s</th><th>h</th></tr></thead>"#,
        "<tbody><tr><td>a</td><td>b</td><td></td></tr><tr><td>f</td><td>s</td><td>s</td></tr>",
        "<tr><td>g</td><td></td><td>b</td></tr></tbody></table></figure>",
        "<p>list below</p><ul><li>this is 1</li><li>this is 2</li><li>this is 3</li></ul>",
        "<p>numbered list below</p><ol><li>this is 1</li><li>this is 2</li><li>this is 3</li></ol>"
    );
    let doc = format!(r#"<html><body><div class="entry-content">{body}</div></body></html>"#);
    let result = extract(&doc, &Kw::new()).unwrap_or_default();
    assert_eq!(result.matches("this is sample intro").count(), 1);
}

#[test]
fn test_recover_wild_text_inline_formatting_dedup() {
    // Regression: the recovery dedup must render accumulated text per element with plain
    // concatenation, exactly like the candidates: ' '.join across all text nodes invented spaces
    // at inline-tag boundaries (Hyper<b>link</b>ed -> 'Hyper link ed'), so a paragraph folded into
    // a list item escaped the substring check and was emitted twice.
    let para = concat!(
        "This paragraph has Hyper<b>link</b>ed formatting inside and needs to be comfortably ",
        "longer than the fifty character dedup gate to be caught by the substring check."
    );
    let doc = format!(
        "<html><body><article><dl><dt>Term one</dt><dd><p>{para}</p></dd></dl></article></body></html>"
    );
    let result = extract(&doc, &Kw::new().formatting(true)).unwrap_or_default();
    assert_eq!(result.matches("formatting inside").count(), 1);
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

#[test]
fn test_prune_boilerplate_table_after_nested() {
    // prune_unwanted_sections must not skip a boilerplate table that follows a deleted table
    // containing a nested one.
    let options = default_options();
    let call = Call::default();
    let ctx = ctx_for(&options, &call);
    let page = boilerplate_page();
    // oracle: len(prune_unwanted_sections(fromstring(page), {"table", "p"}, Extractor(tables=True))
    //   .findall(".//table")) is 0, and the p is kept
    let (mut tree, root) = lxml_fromstring(&page);
    prune_unwanted_sections(&mut tree, root, &["table", "p"], &ctx, false).unwrap();
    assert_eq!(tree.iter(root, &["table"]).count(), 0);
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
fn test_recall_escalation() {
    // A short balanced extraction covering little of the page is retried in recall mode: wild
    // paragraphs block the baseline rescue while the bulk of the content sits in bare divs.
    let ps: String = (0..3)
        .map(|i| {
            format!("<p>Wild paragraph number {i} directly under body, with enough words to pass the paragraph checks in place.</p>")
        })
        .collect();
    let divs: String = (0..30)
        .map(|i| {
            format!("<div>Main content block {i} living in a bare div element with plenty of meaningful words to matter here today.</div>")
        })
        .collect();
    let doc = format!("<html><body>{ps}{divs}</body></html>");
    let result = extract(&doc, &Kw::new()).unwrap_or_default();
    assert_eq!(result.matches("Wild paragraph").count(), 3);
    assert_eq!(result.matches("Main content block").count(), 30);
}

#[test]
fn test_recall_escalation_justext_comment_scoping() {
    // Regression: justext has no article-region scoping of its own and will absorb a reader-comments
    // thread as if it were content (AEB regression: a short article inviting comments, justext
    // grabbed the replies too): comment containers must be stripped before it runs, UNLESS this is
    // a thread-forum (schema.org DiscussionForumPosting), where those same containers hold the
    // actual posts, not boilerplate replies (WCXB regression fixed by _forum_thread_page). The
    // fast=True case is the rule-based retry alone (justext is skipped in fast mode): it must apply
    // the same comment-scoping, or a reader-comments thread leaks into the body precisely when the
    // caller asked to exclude comments.
    let intro = escalation_intro();
    let kw = Kw::new().comments(false);
    // blog_excludes_comments, deviation: D-1, upstream runs it with `fast=False` (the value is the
    // same in fast mode)
    let result = extract(&thread_doc("", "div", &intro, true), &kw).unwrap_or_default();
    assert!(result.contains("Introductory paragraph"));
    assert_eq!(result.matches("Reply number").count(), 0);
    // forum_keeps_posts, deviation: D-1, upstream asserts 8 replies with `fast=False`; the rule-based
    // retry of fast mode keeps none; oracle: extract(doc, output_format="txt",
    // include_comments=False) with fast=True: result.count("Reply number") == 0
    let result = extract(&thread_doc(DFP_JSONLD, "div", &intro, true), &kw).unwrap_or_default();
    assert!(result.contains("Introductory paragraph"));
    assert_eq!(result.matches("Reply number").count(), 0);
    // blog_excludes_comments_fast
    let result = extract(&thread_doc("", "div", &intro, true), &kw).unwrap_or_default();
    assert!(result.contains("Introductory paragraph"));
    assert_eq!(result.matches("Reply number").count(), 0);
}

#[test]
fn test_recall_escalation_no_comment_doubling() {
    // Regression: with include_comments=True on a thread-forum page, posts must not appear in both
    // text and comments (the escalation stage sees the raw tree, whose comment containers
    // extract_comments already consumed). Policy: comment capture on a DFP page is re-routed into
    // the body (the 'comments' are the posts); partial captures must not lose the rest of the
    // thread either (WCXB regression: pruning all containers because a fragment was captured
    // tanked 3 forum pages). <p> replies are capturable, <div> replies are not (extract_comments
    // rejects divs).
    let intro = escalation_intro();
    // txt output concatenates text and comments: exactly one occurrence per post overall
    let result =
        extract(&thread_doc(DFP_JSONLD, "p", &intro, true), &Kw::new()).unwrap_or_default();
    assert!(result.contains("Introductory paragraph"));
    assert_eq!(result.matches("Reply number").count(), 8);
    // capture_missed, deviation: D-1, upstream asserts 8 with `fast=False`; fast mode keeps no reply
    // of the uncaptured div thread; oracle: extract(doc, output_format="txt",
    // include_comments=True) with fast=True: result.count("Reply number") == 0
    let result =
        extract(&thread_doc(DFP_JSONLD, "div", &intro, true), &Kw::new()).unwrap_or_default();
    assert!(result.contains("Introductory paragraph"));
    assert_eq!(result.matches("Reply number").count(), 0);
}

#[test]
fn test_dfp_long_opening_post_keeps_replies() {
    // Regression: the DFP re-route emptied the captured comments betting on the
    // stage-4 escalation to restore the posts into the body, but the escalation is gated on
    // len_text < ESCALATION_MAX_LENGTH: a thread whose opening post alone exceeds the gate lost
    // every reply from both channels. Captured posts must survive regardless of gates.
    let long_op: String = (0..16)
        .map(|i| {
            format!(
                "<p>Opening post paragraph {i} lays out the question in full detail, with context, \
                 history and several worked examples that make the thread starter alone longer than \
                 the escalation length gate, so no rescue pass can be relied upon for the replies.</p>"
            )
        })
        .collect();
    let result =
        extract(&thread_doc(DFP_JSONLD, "p", &long_op, true), &Kw::new()).unwrap_or_default();
    assert!(result.contains("Opening post paragraph"));
    assert_eq!(result.matches("Reply number").count(), 8);
}

#[test]
fn test_dfp_precision_keeps_posts() {
    // Regression: precision mode pruned REMOVE_COMMENTS_XPATH from the very tree
    // the forum re-route had just restored the posts into, and precision closes the stage-3/4
    // rescues, so a DFP page returned no posts at all.
    let doc = thread_doc(DFP_JSONLD, "p", &escalation_intro(), true);
    let result = extract(&doc, &Kw::new().focus(Focus::Precision)).unwrap_or_default();
    assert_eq!(result.matches("Reply number").count(), 8);
}

#[test]
fn test_recall_escalation_blog_comment_leak() {
    // Regression: on a wrapper-less blog layout (broad body selection) with
    // include_comments=True and an uncapturable div-based reply thread, the recall retry kept the
    // comment containers (its prune gate checked options.comments, the justext branch checked
    // len_comments) and emitted the reader comments as main body text.
    let doc = thread_doc("", "div", &escalation_intro(), false);
    // deviation: D-1, upstream runs the "full" case with `fast=False` (the value is the same in
    // fast mode), then the "fast" case, which is this one
    let result = extract(&doc, &Kw::new()).unwrap_or_default();
    assert!(result.contains("Introductory paragraph"));
    assert_eq!(result.matches("Reply number").count(), 0);
}

#[test]
fn test_escalation_retry_no_comment_capture() {
    // Regression: the stage-4 retry re-ran extract_comments although the caller
    // discards the retry's comments triple: a container matching COMMENTS_XPATH but not
    // REMOVE_COMMENTS_XPATH had its text captured and deleted inside the retry, so it vanished from
    // both channels. The retry must not capture comments at all.
    let intro: String = (0..4)
        .map(|i| {
            format!("<li>Point number {i} of the short visible article summary text here.</li>")
        })
        .collect();
    let cont: String = (0..12)
        .map(|i| {
            format!(
                "<p>Continuation paragraph {i} with substantial article prose that only the \
                 recall retry can recover from inside the form wrapper element.</p>"
            )
        })
        .collect();
    let replies: String = (0..8)
        .map(|i| {
            format!(
                "<p>Reply {i} ZQXJKVREPLY says something reasonably long about the topic at hand \
                 in this discussion thread below the article body.</p>"
            )
        })
        .collect();
    let doc = format!(
        "<html><body><article><h1>Title of the page</h1><ul>{intro}</ul><form>{cont}<div class='user-comment-area'>{replies}</div></form></article></body></html>"
    );
    let result = extract(&doc, &Kw::new()).unwrap_or_default();
    assert!(result.contains("Continuation paragraph 5"));
    assert!(result.contains("ZQXJKVREPLY"));
}

#[test]
fn test_main_pass_excludes_comments_when_disabled() {
    // Regression: with include_comments=False, comment containers must be pruned before the MAIN
    // pass, not only in precision mode: balanced/recall let the div-ladder pull a short article's
    // comment thread into the body (a bare-div thread the comment extractor cannot capture).
    let intro = "<p>Short intro under the escalation and rescue thresholds here.</p>";
    let replies: String = (0..8)
        .map(|i| {
            format!("<div>Reader comment number {i} that must never appear when comments are excluded, long enough to matter.</div>")
        })
        .collect();
    let doc = format!(
        "<html><body><article>{intro}</article><div id='comments' class='comments-area'>{replies}</div></body></html>"
    );
    // WordPress comment lists: still ol/ul on the raw tree
    let items = replies.replace("<div>", "<li>").replace("</div>", "</li>");
    let doc_lists = [("ol", "commentlist"), ("ul", "comment-list")].map(|(tag, class)| {
        format!(
            "<html><body><article>{intro}</article><{tag} class='{class}'>{items}</{tag}></body></html>"
        )
    });
    // deviation: D-1, upstream runs every page with `fast=False` and with `fast=True`; the values
    // are the same
    for page in std::iter::once(&doc).chain(&doc_lists) {
        let result = extract(page, &Kw::new().comments(false)).unwrap_or_default();
        assert!(result.contains("Short intro"));
        assert_eq!(result.matches("Reader comment number").count(), 0);
    }
}

#[test]
fn test_main_pass_excludes_details_wrapped_comments() {
    // Regression (#850): comment containers wrapped in <details> must also be pruned when
    // include_comments=False: REMOVE_COMMENTS_XPATH matched only div/list/section, so a
    // <details id='comments'> thread leaked into the body outside precision mode. A <details>
    // without a comment id/class must be kept (no over-pruning).
    let body = format!(
        "<article>{}</article>",
        "<p>Real article paragraph with enough content to be extracted normally here.</p>"
            .repeat(3)
    );
    let comments: String = format!(
        "<details id='comments'><summary>Comments</summary>{}</details>",
        (0..6)
            .map(|i| format!(
                "<p>Reader comment number {i} that must never leak into the body text.</p>"
            ))
            .collect::<String>()
    );
    let doc = format!("<html><body>{body}{comments}</body></html>");
    // deviation: D-1, upstream runs this with `fast=False` and with `fast=True`; the values are the
    // same
    let result = extract(&doc, &Kw::new().comments(false)).unwrap_or_default();
    assert!(result.contains("Real article paragraph"));
    assert_eq!(result.matches("Reader comment number").count(), 0);
    // a non-comment <details> is legitimate content and must survive
    let faq = "<details class='faq'><summary>More</summary><p>Kept expandable content paragraph that is genuine.</p></details>";
    let doc_faq = format!("<html><body>{body}{faq}</body></html>");
    assert!(
        extract(&doc_faq, &Kw::new())
            .unwrap_or_default()
            .contains("Kept expandable content")
    );
    // "commentary" is content, not reader comments
    for attr in ["id", "class"] {
        let commentary = format!(
            "<section {attr}='commentary'><p>Expert commentary paragraph that is part of the article.</p></section>"
        );
        let doc_commentary = format!("<html><body>{body}{commentary}</body></html>");
        assert!(
            extract(&doc_commentary, &Kw::new().comments(false))
                .unwrap_or_default()
                .contains("Expert commentary"),
            "{attr}"
        );
    }
}

#[test]
fn test_infinite_scroll_container_with_h1_kept() {
    // Regression: the #911 prune deleted the real article when the scroll container holds the h1
    // (BGR).
    let real = "<p>Real article paragraph with enough content to be extracted normally here.</p>"
        .repeat(4);
    let appended =
        "<p>Appended follow-up story text that belongs to another article entirely.</p>".repeat(4);
    // deviation: D-1, upstream runs the checks below with `fast=False` and with `fast=True`; the
    // values are the same
    let doc = format!(
        "<html><body><div class='site-content infinite-scroll-container'><article><h1>Real title</h1>{real}</article></div></body></html>"
    );
    assert!(
        extract(&doc, &Kw::new())
            .unwrap_or_default()
            .contains("Real article paragraph")
    );
    // still pruned: an infinite-scroll container without an h1, an mvp container even with one
    for (class, head) in [("infinite-scroll", "h2"), ("mvp-post-add-box", "h1")] {
        let doc = format!(
            "<html><body><article><h1>Real title</h1>{real}<div class='{class}'><{head}>Next story</{head}>{appended}</div></article></body></html>"
        );
        let result = extract(&doc, &Kw::new()).unwrap_or_default();
        assert!(result.contains("Real article paragraph"), "{class}");
        assert!(!result.contains("Appended follow-up"), "{class}");
    }
}

#[test]
fn test_markdown_inline_rendering() {
    // Inline markers, links and list markers in markdown (regressions #843, A5, marker
    // composition).
    for (body, expected) in [
        ("<ul><li><p>x <b>bold</b> y</p></li></ul>", "- x **bold** y"),
        ("<p>a <b> x </b> b</p>", "a  **x**  b"),
        ("<p>a <i> x </i> b</p>", "a  *x*  b"),
        ("<p>a <del> x </del> b</p>", "a  ~~x~~  b"),
        ("<p>a <code> x </code> b</p>", "a  `x`  b"),
        ("<p>a <b>   </b> b</p>", "a     b"),
        (
            "<p>see <a href='/x'> bold link </a> ok</p>",
            "see  [bold link](/x)  ok",
        ),
        (
            "<ol><li>one</li><li>two</li><li>three</li></ol>",
            "1. one\n2. two\n3. three",
        ),
        ("<ol><li>only</li></ol>", "1. only"),
        ("<ul><li>a</li><li>b</li></ul>", "- a\n- b"),
        (
            "<ul><li>a<ul><li>b</li><li>c</li></ul></li><li>d</li></ul>",
            "- a\n  - b\n  - c\n- d",
        ),
        ("<ul><li>a<ol><li>b</li></ol></li></ul>", "- a\n  1. b"),
        (
            "<ul><li><ul><li>b</li></ul></li><li>d</li></ul>",
            "  - b\n- d",
        ),
        ("<ul><li><p>a</p></li><li>b</li></ul>", "- a\n- b"),
        ("<ul><li><br/>a</li><li>b<br/>c</li></ul>", "- a\n- b\nc"),
        (
            r#"<ul><li><a href="/x"></a></li><li><a href="/y">b</a></li><li>c</li></ul>"#,
            "- [b](/y)\n- c",
        ),
        (
            "<ul><li>see <a href='http://x.com'><b>bold link</b></a> here</li></ul>",
            "- see [**bold link**](http://x.com) here",
        ),
        (
            "<ul><li>see <a href='http://x.com'>link <b>bold</b></a> here</li></ul>",
            "- see [link **bold**](http://x.com) here",
        ),
        (
            "<p>see <a href='http://x.com'><b>bold</b></a> more</p>",
            "see [**bold**](http://x.com) more",
        ),
        (
            "<p>text <b><i>nested</i></b> end</p>",
            "text ***nested*** end",
        ),
        ("<p><b>prefix <i>italic</i></b></p>", "**prefix *italic***"),
        (
            "<ul><li>text <b><i>nested</i></b> end</li></ul>",
            "- text ***nested*** end",
        ),
        (
            "<blockquote><b>bold</b> text here</blockquote>",
            "**bold** text here",
        ),
        ("<del>gone</del>", "~~gone~~"),
        (
            "<ul><li>text <del>struck</del> more</li></ul>",
            "- text ~~struck~~ more",
        ),
        (
            "<blockquote>text <del>struck</del> more</blockquote>",
            "text ~~struck~~ more",
        ),
        (
            "<ul><li>use <code>func()</code> here</li></ul>",
            "- use `func()` here",
        ),
        (
            "<p>before <b>bold <del>struck</del></b></p>",
            "before **bold ~~struck~~**",
        ),
        (
            "<ol><li>Foo <em>bar</em> baz.</li></ol>",
            "1. Foo *bar* baz.",
        ),
        (
            "<blockquote><p>A <b>bold</b> word</p></blockquote>",
            "A **bold** word",
        ),
        (
            "<blockquote><p>see <a href='http://x.com'>link</a></p></blockquote>",
            "see [link](http://x.com)",
        ),
        (
            "<blockquote><p>text</p><img src='x.jpg' alt='img'/></blockquote>",
            "text\n\n![img](x.jpg)",
        ),
        (
            "<ul><li><img src='/i.jpg' alt='a'></li><li>plain</li></ul>",
            "- ![a](/i.jpg)\n- plain",
        ),
    ] {
        assert_eq!(
            md_inline(body, Kw::zero().links(true).images(true)),
            format!("{INTRO}\n\n{expected}"),
            "{body}"
        );
    }
}

#[test]
fn test_loose_text_tail_not_squished() {
    // Regression #661: a block's loose-text tail must not mash onto the next sibling block.
    let html = r#"<!DOCTYPE html><html lang="en-us"><body><main><section>
        <p>First</p>
        This gets Squished
        <div>
            <h4>There should be a space</h4>
            <p>Another sentence</p>
            This also gets Squished
        </div>
        <div>
            <h4>Where is the space</h4>
            <p>This sentence has to be long enough. If it's long enough the duplication stops, but if it's not long enough then you'll get an extra first. This sentence has to be long enough.</p>
        </div>
    </section></main></body></html>"#;
    let result = extract(html, &Kw::new()).unwrap_or_default();
    assert!(!result.contains("SquishedThere"));
    assert!(!result.contains("SquishedWhere"));
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

#[test]
fn test_table_caption() {
    // Table caption is emitted as a header row above the table body.
    let result = table_md(
        "<table><caption>My Caption</caption><tr><td>a</td><td>b</td></tr></table>",
        Kw::zero(),
    )
    .unwrap();
    assert!(result.contains("My Caption"));
    // caption row appears before the body row
    assert!(result.find("My Caption").unwrap() < result.find("| a |").unwrap());
    // separator must span all columns (2), not just the caption cell (1)
    assert!(result.contains("|---|---|"));
}

#[test]
fn test_table_caption_image_kept() {
    // An image in a caption is emitted once, its caption text is not repeated.
    let words = "Article words go here and they are plentiful. ".repeat(3);
    let doc = format!(
        "<html><body><article><p>Intro {words}</p><table><caption><img src='https://e.com/c.png' alt='Chart'/> Caption words</caption><tr><td>Cell one {words}</td><td>Cell two</td></tr></table><p>Outro {words}</p></article></body></html>"
    );
    let result = extract(&doc, &Kw::new().images(true).markdown()).unwrap();
    assert_eq!(result.matches("![Chart](https://e.com/c.png)").count(), 1);
    assert_eq!(result.matches("Caption words").count(), 1);
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
fn test_image_tail_not_duplicated() {
    // Regression: an image's tail text must not be emitted twice outside a table cell.
    let result = extract_doc(
        "<ul><li>a <img src='i.jpg' alt='A'/> b</li></ul>",
        true,
        &Kw::zero().images(true),
    )
    .unwrap();
    assert!(result.ends_with("a ![A](i.jpg) b"));
}
