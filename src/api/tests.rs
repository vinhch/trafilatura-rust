use std::thread;

use super::*;
use crate::baseline;
use crate::cancel::CancelToken;
use crate::dom::ParseLimits;
use crate::error::Rejection;
use crate::settings::{self, DEFAULT_MAX_INPUT_BYTES, DEFAULT_MAX_NODES};
use crate::utils::HtmlInput;

const PAGE: &str = concat!(
    "<html><head><title>T</title></head><body><article><h2>Heading</h2>",
    "<p>First paragraph with <b>bold</b> words and a <a href=\"x\">link</a> inside a longer sentence ",
    "that reads like prose.</p>",
    "<p>Second paragraph says something else entirely, so the page has two distinct blocks of text ",
    "to keep.</p></article></body></html>"
);

const FIRST: &str =
    "First paragraph with bold words and a link inside a longer sentence that reads like prose.";
const SECOND: &str = "Second paragraph says something else entirely, so the page has two distinct blocks of text to keep.";

fn rejected(rejection: Rejection) -> Result<String, ExtractError> {
    Err(ExtractError::Rejected(rejection))
}

fn text_of(extractor: &Extractor) -> String {
    extractor
        .extract(&Input::text(PAGE), &Call::default())
        .unwrap()
}

#[test]
fn default_formats_per_entry_point() {
    let extractor = Options::new().build();
    // oracle: extract(PAGE, fast=True)
    assert_eq!(text_of(&extractor), format!("Heading\n{FIRST}\n{SECOND}"));

    let document = extractor
        .bare_extraction(&Input::text(PAGE), &Call::default())
        .unwrap();
    // oracle: bare_extraction(PAGE, fast=True).text
    assert_eq!(
        document.text.as_deref(),
        Some(format!("Heading\n{FIRST}\n{SECOND}").as_str())
    );
    // oracle: bare_extraction(PAGE, fast=True).comments
    assert_eq!(document.comments.as_deref(), Some(""));
    // oracle: bare_extraction(PAGE, fast=True).raw_text
    assert_eq!(document.raw_text, document.text);
}

#[test]
fn explicit_format_applies_to_both() {
    let extractor = Options::new().format(Format::Markdown).build();
    // oracle: extract(PAGE, fast=True, output_format="markdown")
    assert_eq!(
        text_of(&extractor),
        format!(
            "## Heading\n\n{}\n\n{SECOND}",
            FIRST.replace("with bold", "with **bold**")
        )
    );

    let document = extractor
        .bare_extraction(&Input::text(PAGE), &Call::default())
        .unwrap();
    // oracle: bare_extraction(PAGE, fast=True, output_format="markdown")
    assert_eq!(document.text, None);
    assert_eq!(document.comments, None);
    assert_eq!(
        document.raw_text.as_deref(),
        Some(
            "Heading First paragraph with  bold  words and a link inside a longer sentence that \
             reads like prose. Second paragraph says something else entirely, so the page has two \
             distinct blocks of text to keep."
        )
    );

    let html = Options::new().format(Format::Html).build();
    // oracle: extract(PAGE, fast=True, output_format="html")
    assert_eq!(
        text_of(&html),
        format!(
            "<html>\n  <body>\n    <h2>Heading</h2>\n    <p>{FIRST}</p>\n    <p>{SECOND}</p>\n  </body>\n</html>"
        )
    );

    // oracle: extract(PAGE, fast=True, output_format="python") raises ValueError
    let python = Options::new().format(Format::Python).build();
    assert_eq!(
        python.extract(&Input::text(PAGE), &Call::default()),
        rejected(Rejection::InvalidValue)
    );
    let document = python
        .bare_extraction(&Input::text(PAGE), &Call::default())
        .unwrap();
    assert!(document.text.is_some());
}

#[test]
fn markdown_implies_formatting() {
    let bold = "First paragraph with **bold** words";
    let markdown = Options::new().format(Format::Markdown).build();
    assert!(text_of(&markdown).contains(bold));

    // oracle: extract(PAGE, fast=True, output_format="markdown", include_formatting=False)
    let plain = Options::new()
        .format(Format::Markdown)
        .formatting(false)
        .build();
    assert!(!text_of(&plain).contains("**"));
    assert!(text_of(&plain).contains(&format!("{FIRST}\n")));

    // oracle: extract(PAGE, fast=True, include_formatting=True)
    let txt = Options::new().formatting(true).build();
    assert!(text_of(&txt).starts_with("## Heading\n\n"));
    assert!(text_of(&txt).contains(bold));

    // oracle: extract(PAGE, fast=True, output_format="markdown", include_links=True)
    let links = Options::new().format(Format::Markdown).links(true).build();
    assert!(text_of(&links).contains("a [link](x) inside"));
}

fn linked_page(label: &str) -> String {
    format!(
        "<html><head><title>{label}</title></head><body><article><p>Page {label} has a paragraph \
         with a <a href=\"x\">relative link</a> in the middle of a sentence long enough to be \
         kept as the main text of the page.</p><p>The second paragraph of page {label} adds more \
         words, so the page is not rejected as too short.</p></article></body></html>"
    )
}

#[test]
fn extractor_is_reusable_across_urls_and_threads() {
    fn assert_send_sync<T: Send + Sync>() {}
    assert_send_sync::<Extractor>();
    assert_send_sync::<Options>();
    assert_send_sync::<Document>();

    let build = || Options::new().links(true).format(Format::Markdown).build();
    let shared = build();
    let results: Vec<(usize, String)> = thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|index| {
                let shared = &shared;
                scope.spawn(move || {
                    let page = linked_page(&format!("p{index}"));
                    let url = format!("https://site{index}.example/d/");
                    let input = Input::text(&page).url(&url);
                    let reused = shared.extract(&input, &Call::default()).unwrap();
                    let fresh = build().extract(&input, &Call::default()).unwrap();
                    assert_eq!(reused, fresh);
                    (index, reused)
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap())
            .collect()
    });
    for (index, text) in &results {
        assert!(
            text.contains(&format!("[relative link](https://site{index}.example/x)")),
            "{index}: {text}"
        );
    }

    // oracle: extract(linked_page("p0"), url="https://a.example/d/", fast=True, output_format="markdown", include_links=True)
    let page = linked_page("p0");
    let without = shared
        .extract(&Input::text(&page), &Call::default())
        .unwrap();
    let with = shared
        .extract(
            &Input::text(&page).url("https://a.example/d/"),
            &Call::default(),
        )
        .unwrap();
    assert_ne!(with, without);
    assert!(without.contains("[relative link](x)"), "{without}");
    assert!(
        with.contains("[relative link](https://a.example/x)"),
        "{with}"
    );
    // the URL of one call leaves the shared options alone
    assert_eq!(
        shared
            .extract(&Input::text(&page), &Call::default())
            .unwrap(),
        without
    );
}

#[test]
fn html2txt_public_matches_internal() {
    for clean in [true, false] {
        let public = html2txt(&Input::text(PAGE), clean, &Call::default()).unwrap();
        let internal = baseline::html2txt(
            HtmlInput::Str(PAGE),
            clean,
            &ParseLimits::default(),
            &Call::default(),
        )
        .unwrap();
        // oracle: html2txt(PAGE), html2txt(PAGE, clean=False)
        assert_eq!(public, format!("Heading {FIRST} {SECOND}"));
        assert_eq!(public, internal);
    }
    let bytes = html2txt(&Input::bytes(PAGE.as_bytes()), true, &Call::default()).unwrap();
    assert_eq!(
        bytes,
        html2txt(&Input::text(PAGE), true, &Call::default()).unwrap()
    );
    // oracle: html2txt("")
    assert_eq!(
        html2txt(&Input::text(""), true, &Call::default()),
        Ok(String::new())
    );

    let token = CancelToken::new();
    token.cancel();
    assert_eq!(
        html2txt(&Input::text(PAGE), true, &Call::default().cancel(&token)),
        Err(ExtractError::Cancelled)
    );
}

#[test]
fn max_nodes_option_reaches_parse() {
    let page = format!(
        "<html><body>{}</body></html>",
        "<p>some text in a paragraph</p>".repeat(20)
    );
    let limited = Options::new().max_nodes(Some(5)).build();
    assert_eq!(
        limited.extract(&Input::text(&page), &Call::default()),
        rejected(Rejection::TreeTooLarge)
    );
    assert_eq!(
        limited
            .bare_extraction(&Input::text(&page), &Call::default())
            .unwrap_err(),
        ExtractError::Rejected(Rejection::TreeTooLarge)
    );
    assert!(
        Options::new()
            .max_nodes(None)
            .build()
            .extract(&Input::text(&page), &Call::default())
            .is_ok()
    );
    assert!(
        Options::new()
            .build()
            .extract(&Input::text(&page), &Call::default())
            .is_ok()
    );
}

#[test]
fn max_input_bytes_option_reaches_load() {
    let page = format!(
        "<html><body>{}</body></html>",
        "<p>some text in a paragraph</p>".repeat(20)
    );
    let limited = Options::new().max_input_bytes(Some(10)).build();
    assert_eq!(
        limited.extract(&Input::text(&page), &Call::default()),
        rejected(Rejection::InputTooLarge)
    );
    assert_eq!(
        limited
            .bare_extraction(&Input::text(&page), &Call::default())
            .unwrap_err(),
        ExtractError::Rejected(Rejection::InputTooLarge)
    );
    let unlimited = Options::new().max_input_bytes(None).build();
    assert_eq!(
        unlimited.extract(&Input::text(&page), &Call::default()),
        Options::new()
            .build()
            .extract(&Input::text(&page), &Call::default())
    );
}

#[test]
fn options_reach_the_extractor() {
    let options = Options::new()
        .focus(Focus::Precision)
        .comments(false)
        .links(true)
        .images(true)
        .tables(false)
        .target_language("de")
        .config(Config {
            min_output_size: 7,
            ..Config::default()
        })
        .build();
    for settings in [&options.text, &options.document] {
        assert_eq!(settings.focus, Focus::Precision);
        assert!(!settings.comments);
        assert!(settings.links);
        assert!(settings.images);
        assert!(!settings.tables);
        assert_eq!(settings.lang.as_deref(), Some("de"));
        assert_eq!(settings.min_output_size, 7);
        assert_eq!(settings.url, None);
    }
    assert_eq!(options.text.format, settings::Format::Txt);
    assert_eq!(options.document.format, settings::Format::Python);

    let defaults = Options::default().build();
    assert_eq!(defaults.text.focus, Focus::Balanced);
    assert!(defaults.text.comments);
    assert!(!defaults.text.links && !defaults.text.images && defaults.text.tables);
    assert_eq!(defaults.limits.max_nodes, Some(DEFAULT_MAX_NODES));
    assert_eq!(
        defaults.limits.max_input_bytes,
        Some(DEFAULT_MAX_INPUT_BYTES)
    );
    assert_eq!(defaults.limits.max_depth, ParseLimits::default().max_depth);

    let recall = Options::new().focus(Focus::Recall).build();
    assert_eq!(recall.text.focus, Focus::Recall);
}

#[test]
fn bytes_input_matches_text_and_record_id_leaves_txt_unchanged() {
    let extractor = Options::new().build();
    let from_bytes = extractor
        .extract(&Input::bytes(PAGE.as_bytes()), &Call::default())
        .unwrap();
    assert_eq!(from_bytes, text_of(&extractor));
    // the TXT output shows the record id only in its metadata header, so the text is unchanged
    let with_id = extractor
        .extract(&Input::text(PAGE).record_id("r-1"), &Call::default())
        .unwrap();
    assert_eq!(with_id, from_bytes);
}

#[test]
fn rejections_and_cancellation_propagate() {
    let extractor = Options::new().build();
    // oracle: core.bare_extraction("", fast=True) is None: load_html("") is None, which raises
    // ValueError inside the function
    assert_eq!(
        extractor.extract(&Input::text(""), &Call::default()),
        rejected(Rejection::EmptyTree)
    );
    let token = CancelToken::new();
    token.cancel();
    assert_eq!(
        extractor.extract(&Input::text(PAGE), &Call::default().cancel(&token)),
        Err(ExtractError::Cancelled)
    );
    assert_eq!(
        extractor
            .bare_extraction(&Input::text(PAGE), &Call::default().cancel(&token))
            .unwrap_err(),
        ExtractError::Cancelled
    );
}

fn paragraph_page(count: usize) -> String {
    format!("<html><body>{}</body></html>", "<p>w</p>".repeat(count))
}

#[test]
fn default_options_reject_a_page_over_the_node_limit() {
    // D-5: html, head and body count too, so 500,000 paragraphs are over the limit.
    let page = paragraph_page(500_000);
    let result = Options::new()
        .build()
        .extract(&Input::text(&page), &Call::default());
    assert_eq!(
        result.map(|text| text.len()),
        Err(ExtractError::Rejected(Rejection::TreeTooLarge))
    );
}

#[test]
fn default_options_accept_a_page_just_under_the_node_limit() {
    let page = paragraph_page(499_000);
    assert!(
        Options::new()
            .build()
            .extract(&Input::text(&page), &Call::default())
            .is_ok()
    );
}

#[test]
fn build_drops_script_text() {
    assert!(Options::new().build().limits.drop_script_text);
    assert!(
        !Options::new()
            .build()
            .with_script_text()
            .limits
            .drop_script_text
    );
}

/// The recall revert keeps `script` in the tree, and `handle_forms` compares form text with it.
fn script_after_form_page() -> String {
    format!(
        "<html><head><title>t</title></head><body><form><aside><p>Short aside paragraph here.</p></aside><div>{}</div></form><div>{}</div><script>{}</script></body></html>",
        "Form body sentence with several words in it. ".repeat(8),
        "Outside text that is part of the page as well. ".repeat(3),
        "var a = 1;".repeat(200),
    )
}

#[test]
fn recall_revert_reruns_with_script_text() {
    let page = script_after_form_page();
    // oracle: extract(page, favor_recall=True, fast=True)
    assert_eq!(
        Options::new()
            .focus(Focus::Recall)
            .build()
            .extract(&Input::text(&page), &Call::default()),
        Ok(format!(
            "{}\n{}",
            "Form body sentence with several words in it. "
                .repeat(8)
                .trim_end(),
            "Outside text that is part of the page as well. "
                .repeat(3)
                .trim_end()
        ))
    );
}

#[test]
fn forum_thread_keeps_replies_in_place() {
    let op = "The opening post explains the problem in plain sentences so that every reader can follow it. ".repeat(2);
    let cl =
        "A closing paragraph from the original poster sums up what the thread decided in the end. "
            .repeat(2);
    let r1 = "First reply agrees with the opening post and adds one more detail.";
    let r2 = "Second reply disagrees politely and suggests another approach instead.";
    let page = |head: &str| {
        format!(
            "<html><head>{head}</head><body><article><p>{op}</p><div class=\"comments\"><p>{r1}</p><p>{r2}</p></div><p>{cl}</p></article></body></html>"
        )
    };
    let extract = |page: &str| {
        Options::new()
            .comments(true)
            .build()
            .extract(&Input::text(page), &Call::default())
    };
    // oracle: extract(page, include_comments=True, fast=True)
    let forum = page(
        r#"<script type="application/ld+json">{"@context":"https://schema.org","@type":"DiscussionForumPosting","headline":"h"}</script>"#,
    );
    assert_eq!(
        extract(&forum),
        Ok(format!("{}\n{r1}\n{r2}\n{}", op.trim_end(), cl.trim_end()))
    );
    assert_eq!(
        extract(&page("")),
        Ok(format!("{}\n{}\n{r1}\n{r2}", op.trim_end(), cl.trim_end()))
    );
}
