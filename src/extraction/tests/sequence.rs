use super::*;
use crate::cancel::assert_every_check_stops;

#[derive(Debug)]
struct Run {
    tree: Tree,
    body: NodeId,
    text: String,
    comments_body: NodeId,
    comments: String,
}

fn run_sequence(html: &str, options: &Extractor, call: &Call<'_>) -> Result<Run, ExtractError> {
    let mut tree = load(html);
    let root = tree.root();
    let ctx = Ctx {
        options: Cow::Borrowed(options),
        call,
    };
    let (body, text, comments_body, comments) =
        trafilatura_sequence(&mut tree, root, &ctx, None, &ParseLimits::default())?;
    Ok(Run {
        tree,
        body,
        text,
        comments_body,
        comments,
    })
}

fn sequence(html: &str, options: &Extractor) -> Run {
    run_sequence(html, options, &Call::default()).unwrap()
}

/// A forum page's JSON-LD head: the page type that makes the forum branch run.
fn forum_head() -> &'static str {
    "<script type=\"application/ld+json\">{\"@context\":\"https://schema.org\",\
        \"@type\":\"DiscussionForumPosting\",\"headline\":\"T\"}</script>"
}

/// A comment thread: the posts the forum salvage recovers.
fn thread_div() -> &'static str {
    "<div id=\"comments\"><p>One comment here</p><br>Second line<p>third</p></div>"
}

/// An `<article>` of `sentences` sentences of `Main` prose.
fn article_page(sentences: usize) -> String {
    format!("<article><p>{}</p></article>", prose("Main", sentences))
}

/// A `teaser` block of `sentences` sentences, outside the main article.
fn teaser(sentences: usize) -> String {
    format!(
        "<div class=\"teaser\"><p>{}</p></div>",
        prose("Teaser", sentences)
    )
}

/// An article of four sentences followed by a 40-sentence teaser: the escalation accepts it.
fn esc_accept_page() -> String {
    format!(
        "<article><p>{}</p>{}</article>",
        prose("Main", 4),
        teaser(40)
    )
}

#[test]
fn sequence_matches_python() {
    let forum = forum_head();
    let thread = thread_div();
    let thread_text = "One comment here Second line  third";
    let article = article_page(5);
    let short = article_page(1);
    let article_ld = format!(
        "<script type=\"application/ld+json\">{{\"@type\": \"Article\", \"articleBody\": \"{}\"}}</script>",
        prose("Json", 6)
    );
    let esc_accept = esc_accept_page();
    let esc_ratio = format!(
        "<article><p>{}</p>{}</article><div><p>{}</p></div>",
        prose("Main", 4),
        teaser(1),
        prose("Outside", 40)
    );
    let main5 = prose("Main", 5).trim().to_owned();
    let main4 = prose("Main", 4).trim().to_owned();
    let main1 = prose("Main", 1).trim().to_owned();
    let json_text = prose("Json", 6).trim().to_owned();
    let esc_text = format!("{}{}", prose("Main", 4), prose("Teaser", 40))
        .trim()
        .to_owned();
    let with = |extra: &str, page: &str| format!("{page}{extra}");
    let cell_list = "<table><tr><td><ul><li>Cell list item alpha</li></ul></td></tr></table>";
    // oracle: core.trafilatura_sequence(utils.load_html(html), Extractor(output_format="txt",
    //   fast=True, precision=p, recall=r, comments=c)) inside html5_parsers(): `temp_text` and
    //   `temp_comments` of each page, built from prose() and page() as here
    // (label, html, precision, recall, comments, text, comments text)
    let rows: Vec<(&str, String, bool, bool, bool, String, &str)> = vec![
        // forum, posts missing from the body: the salvage appends them
        (
            "f1",
            full_page(&with(thread, &article), forum),
            false,
            false,
            true,
            format!("{main5} {thread_text}"),
            "",
        ),
        (
            "f2",
            full_page(&with(thread, &article), forum),
            true,
            false,
            true,
            format!("{main5} {thread_text}"),
            "",
        ),
        (
            "f3",
            full_page(&with(thread, &article), ""),
            false,
            false,
            true,
            main5.clone(),
            thread_text,
        ),
        (
            "f4",
            full_page(&with(thread, &article), forum),
            false,
            false,
            false,
            main5.clone(),
            "",
        ),
        (
            "f5",
            full_page(&with(thread, &article), ""),
            false,
            false,
            false,
            main5.clone(),
            "",
        ),
        // the baseline wins on a short forum page: no salvage
        (
            "f6",
            full_page(&with(thread, &short), forum),
            false,
            false,
            true,
            format!("{main1}\nOne comment here\nSecond line\nthird"),
            "",
        ),
        (
            "f7",
            full_page(&short, &article_ld),
            false,
            false,
            true,
            json_text,
            "",
        ),
        (
            "f8",
            full_page("<article><p>Short text only.</p></article>", ""),
            false,
            false,
            true,
            "Short text only.".to_owned(),
            "",
        ),
        (
            "f9",
            full_page(&with(thread, &article), ""),
            true,
            false,
            true,
            main5.clone(),
            thread_text,
        ),
        // escalation accepted and refused by the ratio, with a forum and with comments
        (
            "e1",
            full_page(&esc_accept, ""),
            false,
            false,
            true,
            esc_text.clone(),
            "",
        ),
        (
            "e2",
            full_page(&esc_ratio, ""),
            false,
            false,
            true,
            main4.clone(),
            "",
        ),
        (
            "e3",
            full_page(&with(thread, &esc_accept), forum),
            false,
            false,
            true,
            esc_text.clone(),
            "",
        ),
        (
            "e4",
            full_page(&with(thread, &esc_ratio), forum),
            false,
            false,
            true,
            format!("{main4} {thread_text}"),
            "",
        ),
        (
            "e5",
            full_page(&with(thread, &esc_accept), ""),
            false,
            false,
            true,
            esc_text.clone(),
            thread_text,
        ),
        (
            "e6",
            full_page(&with(thread, &esc_accept), ""),
            false,
            false,
            false,
            esc_text,
            "",
        ),
        (
            "e7",
            full_page(&with(thread, &esc_ratio), ""),
            false,
            false,
            true,
            main4.clone(),
            thread_text,
        ),
        // oracle: scripts/oracle.sh over the esc_accept page, the cascade trace of
        //   gen_extraction_cases.run_extract(page, "txt", Options(focus="precision", comments=False)) inside
        //   html5_parsers(): forum=False captured=False first=291 baseline=None page=None retry=None
        //   err=False final=main, so retry_ran is false; the text is the four Main sentences of the page
        // Balanced (e1) escalates to the teaser text, so this row is the one that proves the Balanced gate
        (
            "e8",
            full_page(&esc_accept, ""),
            true,
            false,
            false,
            main4,
            "",
        ),
        // recall: the rescue runs (focus is not precision)
        ("r1", full_page(&short, ""), false, true, true, main1, ""),
        // recall keeps a list in a table cell, balanced drops it
        (
            "r2",
            full_page(
                &format!("<article><p>{}</p>{cell_list}</article>", prose("Main", 5)),
                "",
            ),
            false,
            true,
            false,
            format!("{main5} Cell list item alpha"),
            "",
        ),
        (
            "r3",
            full_page(&with(thread, &article), forum),
            false,
            true,
            true,
            format!("{main5} {thread_text}"),
            "",
        ),
    ];
    for (label, html, precision, recall, comments, text, comments_text) in rows {
        let run = sequence(&html, &cascade_options(precision, recall, comments));
        assert_eq!(run.text, text, "{label} text");
        assert_eq!(run.comments, comments_text, "{label} comments");
    }
    // oracle: etree.tostring(body, encoding="unicode", with_tail=False) of f1, and of the comments
    //   body of f3
    let run = sequence(
        &full_page(&with(thread, &article), forum),
        &cascade_options(false, false, true),
    );
    let posts = "<p>One comment here</p><p>Second line</p><p>third</p>";
    assert_eq!(
        run.tree.tostring(run.body, false, false),
        format!("<body><p>{}</p>{posts}</body>", prose("Main", 5).trim())
    );
    assert_eq!(
        run.tree.tostring(run.comments_body, false, false),
        "<body/>"
    );
    let run = sequence(
        &full_page(&with(thread, &article), ""),
        &cascade_options(false, false, true),
    );
    assert_eq!(
        run.tree.tostring(run.comments_body, false, false),
        format!("<body>{posts}</body>")
    );
}

#[test]
fn sequence_returns_cancelled_for_a_cancelled_call() {
    let page = full_page(
        &format!("{}{}", article_page(5), thread_div()),
        forum_head(),
    );
    let token = CancelToken::new();
    token.cancel();
    let cancelled = Call::default().cancel(&token);
    let run = run_sequence(&page, &cascade_options(false, false, true), &cancelled);
    assert_eq!(run.err(), Some(ExtractError::Cancelled));
}

#[test]
fn every_sequence_check_stops_the_call() {
    let options = cascade_options(false, false, true);
    // f6 takes the baseline rescue, e1 accepts the escalation, f1 salvages the forum posts
    let baseline = full_page(
        &format!("{}{}", article_page(1), thread_div()),
        forum_head(),
    );
    let escalation = full_page(&esc_accept_page(), "");
    let salvage = full_page(
        &format!("{}{}", article_page(5), thread_div()),
        forum_head(),
    );
    // The totals are regression pins of the crate's check count: deleting a stage check moves
    // them. Update one only when a check is added or removed on purpose.
    assert_eq!(
        assert_every_check_stops(|call| run_sequence(&baseline, &options, call)),
        103,
        "f6"
    );
    assert_eq!(
        assert_every_check_stops(|call| run_sequence(&escalation, &options, call)),
        90,
        "e1"
    );
    assert_eq!(
        assert_every_check_stops(|call| run_sequence(&salvage, &options, call)),
        73,
        "f1"
    );
}
