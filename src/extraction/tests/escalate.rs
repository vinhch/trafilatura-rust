use super::*;

type RetryResult = Result<(NodeId, String), ExtractError>;

/// Retry lengths with whether the retry replaces the first pass.
type Retries = &'static [(usize, bool)];

/// A retry that returns `text` in a new `body`.
fn retry_with(
    text: String,
) -> impl FnOnce(&mut Tree, NodeId, &Ctx<'_>, Option<&str>) -> RetryResult {
    move |tree, _, _, _| Ok((tree.new_element("body"), text))
}

/// `escalate` on a one-paragraph page, current text of `current_len` characters, a balanced
/// first pass on a page that is no forum.
fn run_escalate(
    options: &Extractor,
    current_len: usize,
    retry: impl FnOnce(&mut Tree, NodeId, &Ctx<'_>, Option<&str>) -> RetryResult,
    call: &Call<'_>,
) -> Result<Option<(NodeId, String)>, ExtractError> {
    let mut tree = load("<html><body><p>x</p></body></html>");
    let root = tree.root();
    let body = tree.new_element("body");
    let ctx = Ctx {
        options: Cow::Borrowed(options),
        call,
    };
    escalate(
        &mut tree,
        root,
        &ctx,
        None,
        false,
        "",
        (body, "c".repeat(current_len)),
        retry,
    )
}

#[test]
fn escalate_swallows_rejection() {
    // oracle: the cascade with `core._recall_retry` raising ValueError keeps the first-pass text:
    //   `except Exception` leaves r_text empty, so the floor fails
    let options = cascade_options(false, false, true);
    let result = run_escalate(
        &options,
        100,
        |_, _, _, _| Err(ExtractError::Rejected(Rejection::TreeTooLarge)),
        &Call::default(),
    );
    assert_eq!(result, Ok(None));
}

#[test]
fn escalate_swallows_invalid_value() {
    // oracle: the cascade with `core._recall_retry` raising ValueError("Invalid tag name 'o:p'")
    //   keeps the first-pass text: `except Exception` leaves r_text empty, so the floor fails
    let options = cascade_options(false, false, true);
    let result = run_escalate(
        &options,
        100,
        |_, _, _, _| Err(ExtractError::Rejected(Rejection::InvalidValue)),
        &Call::default(),
    );
    assert_eq!(result, Ok(None));
}

#[test]
fn escalate_propagates_cancel() {
    let options = cascade_options(false, false, true);
    for error in [ExtractError::Cancelled, ExtractError::DeadlineExceeded] {
        let failure = error.clone();
        let result = run_escalate(
            &options,
            100,
            move |_, _, _, _| Err(failure),
            &Call::default(),
        );
        assert_eq!(result, Err(error));
    }
}

#[test]
fn escalate_acceptance_rules() {
    // oracle: the cascade with extract_content, baseline and html2txt stubbed and `core._recall_retry`
    //   returning r characters, for a first pass of cur characters and MIN_EXTRACTED_SIZE m: the
    //   retry replaces the first pass == (r >= m and r > 1.5 * cur)
    // (cur, m, [(r, replaced)])
    let rows: &[(usize, usize, Retries)] = &[
        (
            100,
            150,
            &[
                (149, false),
                (150, false),
                (151, true),
                (249, true),
                (300, true),
            ],
        ),
        (
            100,
            250,
            &[
                (149, false),
                (151, false),
                (249, false),
                (250, true),
                (251, true),
            ],
        ),
        (
            200,
            250,
            &[
                (249, false),
                (250, false),
                (299, false),
                (300, false),
                (301, true),
            ],
        ),
        (
            200,
            150,
            &[
                (150, false),
                (151, false),
                (299, false),
                (300, false),
                (301, true),
            ],
        ),
        (1, 1, &[(0, false), (1, false), (2, true), (149, true)]),
    ];
    for &(current, min_size, retries) in rows {
        let mut options = cascade_options(false, false, true);
        options.min_extracted_size = min_size;
        for &(retried, accepted) in retries {
            let result = run_escalate(
                &options,
                current,
                retry_with("r".repeat(retried)),
                &Call::default(),
            )
            .unwrap();
            assert_eq!(
                result.map(|(_, text)| text.len()),
                accepted.then_some(retried),
                "current {current}, minimum {min_size}, retry {retried}"
            );
        }
    }
}

#[test]
fn escalate_counts_characters() {
    // lengths are code points: 150 two-byte characters are 150, not 300, so they fail the floor of
    // 150 only by the ratio (150 is not above 1.5 x 100)
    let mut options = cascade_options(false, false, true);
    options.min_extracted_size = 150;
    for (count, accepted) in [(150, false), (151, true)] {
        let result = run_escalate(
            &options,
            100,
            retry_with("\u{e9}".repeat(count)),
            &Call::default(),
        )
        .unwrap();
        assert_eq!(result.is_some(), accepted, "{count}");
    }
}

#[test]
fn escalate_input_follows_forum_and_comments() {
    const TREE_PAGE: &str = "<html><body><article><p>Main</p></article>\
        <div id=\"comments\"><p>Div comment text</p></div>\
        <ol class=\"comments\"><li>List reply text</li></ol></body></html>";
    // oracle: core._recall_retry stubbed to record whether its input tree holds the comment div and
    //   the comment list, and the focus of its options, behind the stubbed cascade of
    //   escalate_acceptance_rules; a forum's temp_comments is already reset to "" at that point
    // (is_forum, comments, temp_comments, div kept, list kept)
    let rows = [
        (false, true, "c", false, false),
        (false, true, "", false, true),
        (true, true, "", true, true),
        (true, false, "", true, true),
    ];
    for (is_forum, comments, temp_comments, div_kept, list_kept) in rows {
        let options = cascade_options(false, false, comments);
        let call = Call::default();
        let ctx = Ctx {
            options: Cow::Borrowed(&options),
            call: &call,
        };
        let mut tree = load(TREE_PAGE);
        let root = tree.root();
        let body = tree.new_element("body");
        let mut seen = None;
        let result = escalate(
            &mut tree,
            root,
            &ctx,
            None,
            is_forum,
            temp_comments,
            (body, "c".repeat(100)),
            |tree, esc_root, retry_ctx, _| {
                let text = tree.text_content(esc_root);
                seen = Some((
                    text.contains("Div comment text"),
                    text.contains("List reply text"),
                    retry_ctx.options.focus,
                ));
                Ok((tree.new_element("body"), "r".repeat(400)))
            },
        )
        .unwrap();
        assert!(result.is_some());
        assert_eq!(
            seen,
            Some((div_kept, list_kept, Focus::Recall)),
            "{is_forum} {comments} {temp_comments:?}"
        );
        // the caller's options keep their focus and its tree keeps the comments
        assert_eq!(options.focus, Focus::Balanced);
        assert!(tree.text_content(root).contains("Div comment text"));
    }
}
