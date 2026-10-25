#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Synthetic pages where every word is unique, so any word repeated in the output is a duplicate
//! (upstream's `tests/repeated_output_tests.py`). Upstream runs these with the slow default
//! (`fast=False`); the crate has fast mode only (D-1), so the sets of pages that come back empty or
//! repeat a word are the fast-mode outcome from `tools/parity/parity/gen_repeated_output_cases.py`.
//! An upstream assertion that holds in fast mode is asserted as written.

#[path = "repeated_output_tests/cases.rs"]
mod cases;

use cases::{COMMENT_LIST_REPEATED, EMPTY, KNOWN_DUPLICATES, Key};
use trafilatura_rust::{Call, ExtractError, Focus, Format, Input, Options};

// each `@` becomes a unique word
const INLINE: [(&str, &str); 10] = [
    ("a", r#"<a href="/x">@ @</a>"#),
    ("b", "<b>@ @</b>"),
    ("br", "<br/>"),
    ("code", "<code>@</code>"),
    ("em", "<em>@ @</em>"),
    ("img", r#"<img src="/x.png" alt="@ @"/>"#),
    ("span", "<span>@ @</span>"),
    ("sup", "<sup>@</sup>"),
    ("a_code", r#"<a href="/x"><code>@</code></a>"#),
    ("b_code", "<b><code>@</code></b>"),
];

// `{}` is replaced by a run of text with inline elements
const BLOCKS: [(&str, &str); 20] = [
    ("p", "<p>{}</p>"),
    ("div", "<div>{}</div>"),
    ("span_p", r#"<span data-as="p">{}</span>"#),
    (
        "span_p_holds_p",
        r#"<span data-as="p">{}<p>{}</p></span>@ @ @"#,
    ),
    (
        "span_p_then_p",
        r#"<span data-as="p">{}</span><p>{}</p>@ @ @"#,
    ),
    ("li", "<ul><li>{}</li><li>{}</li></ul>@ @ @"),
    (
        "nested_li",
        "<ul><li>{}<ul><li>{}</li><li>@ @ @</li></ul></li></ul>",
    ),
    ("p_in_li", "<ul><li><p>{}</p></li><li>@ @</li></ul>@ @ @"),
    (
        "table_in_li",
        "<ul><li>@ @<table><tr><td>{}</td></tr></table></li></ul>@ @ @",
    ),
    (
        "blockquote_in_li",
        "<ul><li><blockquote><p>{}</p></blockquote>@ @ @</li><li>@ @</li></ul>@ @ @",
    ),
    ("ol", "<ol><li>{}</li><li>@ @ @</li></ol>@ @ @"),
    (
        "table",
        "<table><tr><th>@</th><th>@</th></tr><tr><td>{}</td><td>@ @</td></tr></table>@ @ @",
    ),
    (
        "table_in_td",
        "<table><tr><td><table><tr><td>{}</td><td>@ @</td></tr></table>@ @ @</td><td>@ @</td></tr></table>@ @ @",
    ),
    (
        "caption",
        "<table><caption><p>{}</p></caption><tr><td>@ @</td><td>@ @</td></tr></table>@ @ @",
    ),
    ("blockquote", "<blockquote><p>{}</p></blockquote>@ @ @"),
    ("pre", "<p>{}</p><pre>@ @ @</pre>@ @ @"),
    (
        "figure",
        r#"<figure><img src="/f.png" alt="@ @"/><figcaption>{}</figcaption></figure>@ @ @"#,
    ),
    (
        "figure_then_p",
        r#"<figure><img src="/f.png" alt="@ @"/><figcaption>{}</figcaption></figure>@ @ @<p>{}</p>"#,
    ),
    // "hidden": `<p>{}</p><div style='display:none'>` + sixty `@ ` + `</div>`, built in `block`
    ("hidden", ""),
    ("h2", "<h2>@ @ {}</h2>@ @ @"),
];

const WRAPPERS: [(&str, &str); 3] = [
    ("bare", "{}"),
    (
        "nested",
        r#"<div class="page"><div class="col"><main><div class="content">{}</div></main></div></div>"#,
    ),
    (
        "article",
        "<main id='content'><article>{}</article><p>@ @ @</p></main>",
    ),
];

const CONFIGS: [&str; 3] = ["markdown", "recall", "precision"];

/// Three paragraphs of twenty-five words.
fn filler() -> String {
    format!("<p>{}</p>", "@ ".repeat(25)).repeat(3)
}

fn block(name: &str) -> String {
    if name == "hidden" {
        return format!(
            "<p>{{}}</p><div style='display:none'>{}</div>",
            "@ ".repeat(60)
        );
    }
    BLOCKS
        .iter()
        .find(|(key, _)| *key == name)
        .unwrap()
        .1
        .to_string()
}

fn lookup(table: &[(&str, &str)], name: &str) -> String {
    table
        .iter()
        .find(|(key, _)| *key == name)
        .unwrap()
        .1
        .to_string()
}

/// `body` in `<html><body>`, each `@` replaced by the next unique word.
// upstream: repeated_output_tests.py::_number
fn number(body: &str) -> String {
    let mut ids = 0_usize..;
    let mut out = String::new();
    for ch in format!("<html><body>{body}</body></html>").chars() {
        if ch == '@' {
            out.push_str(&format!("w{}x", ids.next().unwrap()));
        } else {
            out.push(ch);
        }
    }
    out
}

// upstream: repeated_output_tests.py::_page
fn page(block_name: &str, inline: &str, wrapper: &str, short: bool) -> String {
    let inline = lookup(&INLINE, inline);
    let run = format!("@ @ @ @ {inline} @ @ @ {inline} @ @ @ @.");
    let mut body = block(block_name).replace("{}", &run);
    if !short {
        body = format!("{}{body}{}", filler(), filler());
    }
    number(&lookup(&WRAPPERS, wrapper).replace("{}", &body))
}

/// The unique words (`w<digits>x`) of `result`, or the first five that repeat.
// upstream: repeated_output_tests.py::_assert_unique
fn assert_unique(result: &str) -> Result<Vec<String>, Vec<String>> {
    let bytes = result.as_bytes();
    let mut words: Vec<String> = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        let digits = bytes.get(at + 1..).map_or(0, |rest| {
            rest.iter().take_while(|b| b.is_ascii_digit()).count()
        });
        if bytes[at] == b'w' && digits > 0 && bytes.get(at + 1 + digits) == Some(&b'x') {
            let end = at + digits + 2;
            words.push(String::from_utf8_lossy(&bytes[at..end]).into_owned());
            at = end;
        } else {
            at += 1;
        }
    }
    let mut repeated: Vec<String> = Vec::new();
    for word in &words {
        if words.iter().filter(|other| *other == word).count() > 1 && !repeated.contains(word) {
            repeated.push(word.clone());
        }
    }
    if repeated.is_empty() {
        Ok(words)
    } else {
        repeated.truncate(5);
        Err(repeated)
    }
}

fn config(name: &str) -> Options {
    match name {
        "markdown" => Options::new()
            .format(Format::Markdown)
            .formatting(true)
            .links(true)
            .images(true),
        "recall" => Options::new().focus(Focus::Recall),
        "precision" => Options::new().focus(Focus::Precision),
        "default" => Options::new(),
        other => panic!("unknown config {other}"),
    }
}

/// What `extract` returns, with upstream's `None` as `None`; any other error is a failure.
fn extract(html: &str, options: &Options) -> Option<String> {
    match options
        .build()
        .extract(&Input::text(html), &Call::default())
    {
        Ok(text) if text.is_empty() => None,
        Ok(text) => Some(text),
        Err(ExtractError::Rejected(_)) => None,
        Err(error) => panic!("extract failed: {error:?}"),
    }
}

/// Asserts that `failures` (case, reason) is empty, listing them all.
fn assert_none_failed(name: &str, total: usize, failures: &[(String, String)]) {
    assert!(
        failures.is_empty(),
        "{name}: {} of {total} cases fail:\n{}",
        failures.len(),
        failures
            .iter()
            .map(|(case, why)| format!("{case}: {why}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
}

#[test]
fn test_no_repeated_words() {
    let mut failures = Vec::new();
    let mut total = 0;
    for (block_name, _) in BLOCKS {
        for (inline, _) in INLINE {
            for short in [false, true] {
                for (wrapper, _) in WRAPPERS {
                    for config_name in CONFIGS {
                        total += 1;
                        let key: Key = (block_name, inline, short, wrapper, config_name);
                        let html = page(block_name, inline, wrapper, short);
                        let result = extract(&html, &config(config_name));
                        let verdict = if EMPTY.contains(&key) {
                            match result {
                                None => Ok(()),
                                Some(_) => Err("expected a rejection, got text".to_string()),
                            }
                        } else {
                            match result {
                                None => Err("expected text, got a rejection".to_string()),
                                Some(text) => match (
                                    assert_unique(&text),
                                    KNOWN_DUPLICATES.iter().any(|(known, _)| *known == key),
                                ) {
                                    (Err(words), false) => Err(format!("repeated {words:?}")),
                                    (Ok(_), true) => {
                                        Err("known duplicate no longer repeats".to_string())
                                    }
                                    _ => Ok(()),
                                },
                            }
                        };
                        if let Err(why) = verdict {
                            // the page of the first failure only: a full product failing would bury the list
                            let page = if failures.is_empty() {
                                format!("\n{html}")
                            } else {
                                String::new()
                            };
                            failures.push((format!("{key:?}"), format!("{why}{page}")));
                        }
                    }
                }
            }
        }
    }
    assert_none_failed("test_no_repeated_words", total, &failures);
    assert!(!EMPTY.is_empty() && !KNOWN_DUPLICATES.is_empty());
}

/// A sidebar holding most of the text trips the over-pruning guard, recovery still skips consumed
/// elements.
#[test]
fn test_over_pruned_page_not_repeated() {
    let mut failures = Vec::new();
    for config_name in ["markdown", "precision"] {
        for wrapper in ["nested", "article"] {
            for block_name in ["p_in_li", "blockquote_in_li", "table_in_li"] {
                let mut tokens = 0_usize..;
                let sidebar = format!(
                    "<div class='sidebar'>{}</div>",
                    (0..12)
                        .map(|_| {
                            let words: Vec<String> = (0..40)
                                .map(|_| format!("s{}x", tokens.next().unwrap()))
                                .collect();
                            format!("<p>{}</p>", words.join(" "))
                        })
                        .collect::<String>()
                );
                let html = page(block_name, "br", wrapper, true)
                    .replace("</body>", &format!("{sidebar}</body>"));
                let verdict = match extract(&html, &config(config_name)) {
                    None => Err("no result".to_string()),
                    Some(text) => match assert_unique(&text) {
                        Ok(words) if words.is_empty() => Err("no words".to_string()),
                        Ok(_) => Ok(()),
                        Err(words) => Err(format!("repeated {words:?}")),
                    },
                };
                if let Err(why) = verdict {
                    failures.push((format!("{block_name} {wrapper} {config_name}"), why));
                    failures
                        .last_mut()
                        .unwrap()
                        .1
                        .push_str(&format!("\n{html}"));
                }
            }
        }
    }
    assert_none_failed("test_over_pruned_page_not_repeated", 12, &failures);
}

/// A non-div frame holding a single formatted text run must not lose it to a sidebar.
#[test]
fn test_inline_only_frame_kept() {
    let mut failures = Vec::new();
    for frame in ["article", "main"] {
        for inline in ["a", "b", "code", "em"] {
            let inline_html = lookup(&INLINE, inline);
            let run = format!("{inline_html} @ @ @ {inline_html} @ @ @ @. ").repeat(3);
            let html = number(&format!("<{frame}>{run}</{frame}><div>{}</div>", filler()));
            match extract(&html, &config("markdown")) {
                None => failures.push((format!("{frame} {inline}"), "no result".to_string())),
                Some(text) if !text.contains("w0x") => {
                    failures.push((format!("{frame} {inline}"), format!("w0x lost: {text:?}")));
                }
                Some(_) => {}
            }
        }
    }
    assert_none_failed("test_inline_only_frame_kept", 8, &failures);
}

/// The frame's own text before its first child is part of the content (Blogger post bodies).
#[test]
fn test_frame_lead_text_kept() {
    let sentence = "@ @ @ @ @ @ @ @ @ @. ".repeat(3);
    let mut failures = Vec::new();
    for config_name in CONFIGS {
        for child in ["<br/>", "<p>@ @ @ @ @ @ @ @ @ @.</p>"] {
            let body = format!(
                r#"<div class="post-body entry-content">{sentence}{child}{}</div>"#,
                filler()
            );
            let verdict = match extract(&number(&body), &config(config_name)) {
                None => Err("no result".to_string()),
                Some(text) => match assert_unique(&text) {
                    Ok(words) if words.first().map(String::as_str) == Some("w0x") => Ok(()),
                    Ok(words) => Err(format!("first word {:?}", words.first())),
                    Err(words) => Err(format!("repeated {words:?}")),
                },
            };
            if let Err(why) = verdict {
                failures.push((format!("{config_name} {child}"), why));
            }
        }
    }
    assert_none_failed("test_frame_lead_text_kept", 6, &failures);
}

/// Replies of a captured comment list appear once, even when the article is too short. Upstream
/// expects the default and recall configs to repeat; the triples that repeat in fast mode are
/// `COMMENT_LIST_REPEATED`.
#[test]
fn test_short_article_comment_list_not_repeated() {
    let mut failures = Vec::new();
    for config_name in ["default", "recall", "precision"] {
        for (tag, class) in [("ol", "commentlist"), ("ul", "comment-list")] {
            let replies = format!("<li><p>{}</p></li>", "@ ".repeat(25)).repeat(8);
            let body = format!(
                "<article><p>@ @ @ @ @ @ @ @ @ @.</p></article><{tag} class='{class}'>{replies}</{tag}>"
            );
            let expected_words = body.matches('@').count();
            let repeats = COMMENT_LIST_REPEATED.contains(&(tag, class, config_name));
            let verdict = match extract(&number(&body), &config(config_name)) {
                None => Err("no result".to_string()),
                Some(text) => match (assert_unique(&text), repeats) {
                    (Err(_), true) => Ok(()),
                    (Err(words), false) => Err(format!("repeated {words:?}")),
                    (Ok(_), true) => Err("known repeat is gone".to_string()),
                    (Ok(words), false) if words.len() == expected_words => Ok(()),
                    (Ok(words), false) => {
                        Err(format!("{} words, expected {expected_words}", words.len()))
                    }
                },
            };
            if let Err(why) = verdict {
                failures.push((format!("{tag} {class} {config_name}"), why));
            }
        }
    }
    assert_none_failed("test_short_article_comment_list_not_repeated", 6, &failures);
}
