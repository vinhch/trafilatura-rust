use std::cell::Cell;

use super::*;
use crate::dom::{DumpMode, dump};

/// 1,500 attributes in 250 groups of every form of attribute.
fn mixed_forms_tag() -> String {
    let groups: String = (0..250)
        .map(|group| {
            let k = group * 6;
            format!(
                " a{k}=\"v\" a{}='v' a{}=v a{} a{}=\"\"a{}=\"\"",
                k + 1,
                k + 2,
                k + 3,
                k + 4,
                k + 5
            )
        })
        .collect();
    format!("<x{groups}>")
}

/// (page, html5ever's tokenizer reads a tag of more than MAX_TAG_ATTRIBUTES attributes)
fn cases() -> Vec<(String, bool)> {
    let t = tag(1_500);
    let mut cases = vec![
        (t.clone(), true),
        (format!("<p>{t}</p>"), true),
        (mixed_forms_tag(), true),
        (format!("<svg><![cdata[ > {t} ]]>"), true),
        (format!("<frameset><style>{t}"), true),
        (format!("<svg><title>{t}</title></svg>"), true),
        (format!("<svg><style>{t}"), true),
        (format!("<math><style>{t}"), true),
        (format!("<svg><script>{t}"), true),
        (format!("<![CDATA[ > {t}"), true),
        (format!("<!--> {t}"), true),
        (format!("<!---> {t}"), true),
        (format!("<!-- a --!> {t}"), true),
        (
            format!("<script><!--<script></script> <a b=\" </script> {t} \""),
            true,
        ),
        (format!("<title></titlex></title> {t}"), true),
        (format!("<svg><foreignObject><style></style>{t}"), true),
        // html5ever drops a U+FEFF that starts a feed call; the pieces of `drive` never start with one.
        (format!("<{BOM}x{}>", attrs(1_500)), false),
    ];
    // A character reference right before `<` is still pending when a piece ends there. Its text
    // reconstructs the formatting element `b` as an HTML element, so the section is a bogus
    // comment that ends at the first `>`, not CDATA.
    for char_ref in ["&amp", "&amp;", "&#65", "&#x41", "&"] {
        for foreign in ["<svg><foreignObject><p><b></p>", "<math><mi><p><b></p>"] {
            cases.push((format!("{foreign}{char_ref}<![CDATA[ > {t} ]]>"), true));
            cases.push((format!("{foreign}{char_ref}<!-- {t} -->"), false));
            cases.push((format!("{foreign}{char_ref}<!--> {t}"), true));
        }
    }
    for name in [
        "script", "style", "textarea", "title", "xmp", "iframe", "noembed", "noframes",
    ] {
        cases.push((format!("<{name}>\"{t}\"</{name}>"), false));
    }
    for (page, counted) in [
        (format!("<plaintext>{t}"), false),
        (format!("<!-- {t} -->"), false),
        (format!("<!-- --!- {t} -->"), false),
        (format!("<p title=\"{t}\">"), false),
        (format!("<p title='{t}'>"), false),
        (format!("<svg><![CDATA[ {t} ]]></svg>"), false),
        (format!("<?{t}?>"), false),
        (format!("<!DOCTYPE {t}>"), false),
        (format!("<title></titlex> {t} </title>"), false),
        (
            format!("<script><!--<script></script> {t} </script>"),
            false,
        ),
        (format!("<svg><foreignObject><![CDATA[ {t} ]]>"), false),
        (format!("<math><mi><![CDATA[ {t} ]]>"), false),
        (format!("<math><mi><style>\"{t}\"</style>"), false),
        (format!("<svg><desc><title>\"{t}\"</title>"), false),
        (format!("<select><title>\"{t}\"</title>"), false),
        (format!("<select><style>\"{t}\"</style>"), false),
        (tag(1_000), false),
    ] {
        cases.push((page, counted));
    }
    cases
}

/// Passes every token to `inner` and records the most attributes on one tag token: the tokenizer's
/// own count, whatever the tree builder then does with the tag.
struct TagSpy<'a, S> {
    inner: S,
    most: &'a Cell<usize>,
}

impl<S: TokenSink> TokenSink for TagSpy<'_, S> {
    type Handle = S::Handle;

    fn process_token(&self, token: Token, line: u64) -> TokenSinkResult<Self::Handle> {
        if let Token::TagToken(tag) = &token {
            self.most.set(self.most.get().max(tag.attrs.len()));
        }
        self.inner.process_token(token, line)
    }

    fn end(&self) {
        self.inner.end();
    }

    fn adjusted_current_node_present_but_not_in_html_namespace(&self) -> bool {
        self.inner
            .adjusted_current_node_present_but_not_in_html_namespace()
    }
}

/// The most attributes html5ever's tokenizer builds on one tag when fed `html` whole.
fn most_tag_attributes(html: &str, fragment: bool) -> usize {
    let most = Cell::new(0);
    let call = Call::default();
    let limits = ParseLimits {
        max_nodes: None,
        ..ParseLimits::default()
    };
    let sink = new_sink(&limits, &call, fragment);
    let opts = parse_opts();
    let (builder, initial_state) = if fragment {
        let context = create_element(
            &sink,
            QualName::new(None, ns!(html), local_name!("div")),
            Vec::new(),
        );
        let builder = TreeBuilder::new_for_fragment(sink, context, None, opts.tree_builder);
        let state = builder.tokenizer_state_for_context_elem(false);
        (builder, Some(state))
    } else {
        (TreeBuilder::new(sink, opts.tree_builder), None)
    };
    let tokenizer = Tokenizer::new(
        TagSpy {
            inner: builder,
            most: &most,
        },
        TokenizerOpts {
            initial_state,
            ..opts.tokenizer
        },
    );
    let queue = BufferQueue::default();
    queue.push_back(StrTendril::from_slice(html));
    while !matches!(tokenizer.feed(&queue), TokenizerResult::Done) {}
    tokenizer.end();
    most.get()
}

#[test]
fn scan_agrees_with_html5ever() {
    // the case starts k bytes before the first chunk boundary, so its markup straddles it
    let padded = |k: usize| format!("<p>{}</p>", "x".repeat(CHUNK_BYTES - 7 - k));
    for k in 0..=16 {
        assert_eq!(padded(k).len() + k, CHUNK_BYTES);
    }
    let mut failures = Vec::new();
    for (page, counted) in cases() {
        let htmls =
            std::iter::once(page.clone()).chain((0..=16).map(|k| format!("{}{page}", padded(k))));
        for (variant, html) in htmls.enumerate() {
            for fragment in [false, true] {
                let label: String = page.chars().take(60).collect();
                let ctx = format!("{label:?} variant {variant} fragment {fragment}");
                // The document tree builder accepts a `frameset` only before any content, so the
                // `style` that follows is ignored there alone.
                let counted = if page.starts_with("<frameset>") {
                    variant == 0 && !fragment
                } else {
                    counted
                };
                if (most_tag_attributes(&html, fragment) > MAX_TAG_ATTRIBUTES) != counted {
                    failures.push(format!("html5ever does not agree with the case: {ctx}"));
                    continue;
                }
                match (parse(&html, fragment), counted) {
                    (
                        Err(ExtractError::Rejected(Rejection::TreeTooLarge(
                            TreeLimit::TagAttributes,
                        ))),
                        true,
                    ) => {}
                    (Ok(tree), false) => {
                        let whole = whole_tree(&html, fragment);
                        if dump(&tree, DumpMode::Exact) != dump(&whole, DumpMode::Exact) {
                            failures.push(format!("tree differs from the unscanned parse: {ctx}"));
                        }
                    }
                    (result, _) => failures.push(format!(
                        "counted {counted}, got {:?}: {ctx}",
                        result.map(|_| ())
                    )),
                }
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures, first: {:#?}",
        failures.len(),
        &failures[..failures.len().min(8)]
    );
}

// Random markup made of the pieces that change a tokenizer state, with one tag of
// MAX_TAG_ATTRIBUTES + 1 attributes at a random place: the scan rejects it exactly when
// html5ever's tokenizer reads that many attributes on one tag.
#[test]
fn scan_agrees_with_html5ever_on_random_markup() {
    let bom = BOM.to_string();
    let pieces = [
        "<",
        ">",
        "</",
        "<!",
        "<!-",
        "<!--",
        "-->",
        "--!>",
        "--",
        "-",
        "<![CDATA[",
        "]]>",
        "]]",
        "]",
        "<?",
        "<!DOCTYPE",
        "<!doctype html",
        " PUBLIC \"x\"",
        "<script>",
        "</script>",
        "<style>",
        "</style>",
        "<title>",
        "</title>",
        "<textarea>",
        "</textarea>",
        "<xmp>",
        "<iframe>",
        "<noembed>",
        "<noframes>",
        "<plaintext>",
        "<svg>",
        "</svg>",
        "<math>",
        "<mi>",
        "<foreignObject>",
        "<desc>",
        "<select>",
        "<table>",
        "<frameset>",
        "<head>",
        "<body>",
        "<p>",
        "</p>",
        "<b>",
        "</b>",
        "<i>",
        "<meta charset=x>",
        "<html>",
        "<template>",
        "<noscript>",
        "<svg><foreignObject><p><b></p>",
        "<scr",
        "</scr",
        "ipt>",
        "<!--<script>",
        "'",
        "\"",
        "=",
        "/",
        " ",
        "\n",
        "\r\n",
        "x",
        "a",
        "&amp;",
        "&amp",
        "&#65",
        "&#x41",
        "&",
        "&#",
        "<a ",
        "<a b=",
        "</a ",
        "<script ",
        "</SCRIPT ",
        "</title ",
        "é",
        bom.as_str(),
    ];
    let big = tag(MAX_TAG_ATTRIBUTES + 1);
    let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
    let mut next = |bound: usize| {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        usize::try_from(seed % bound as u64).unwrap()
    };
    let mut failures = Vec::new();
    for round in 0..2_000 {
        let mut parts: Vec<&str> = (0..1 + next(30))
            .map(|_| pieces[next(pieces.len())])
            .collect();
        parts.insert(next(parts.len() + 1), &big);
        let padding = match next(3) {
            0 => String::new(),
            _ => format!("<p>{}</p>", "x".repeat(CHUNK_BYTES - 7 - next(40))),
        };
        let html = format!("{padding}{}", parts.concat());
        for fragment in [false, true] {
            let counted = most_tag_attributes(&html, fragment) > MAX_TAG_ATTRIBUTES;
            let rejected = parse(&html, fragment).is_err_and(|error| {
                error == ExtractError::Rejected(Rejection::TreeTooLarge(TreeLimit::TagAttributes))
            });
            if counted != rejected {
                failures.push(format!(
                    "round {round} fragment {fragment}: html5ever {counted}, scan {rejected}: {:?}",
                    parts.concat()
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures, first: {:#?}",
        failures.len(),
        &failures[..failures.len().min(5)]
    );
}

// The pauses a scan of `html` in steps of `step` bytes reports, as (position, pause), and whether it
// rejected. `resume_*` answers alternate, the same way for every step size.
fn scan_trace(html: &str, step: usize) -> (Vec<(usize, String)>, bool) {
    let mut scanner = TagScanner::new();
    let mut pauses = Vec::new();
    let mut start = 0;
    while start < html.len() {
        let mut end = start.saturating_add(step).min(html.len());
        while !html.is_char_boundary(end) {
            end += 1;
        }
        let Ok((cut, pause)) = scanner.scan(html, start, end) else {
            return (pauses, true);
        };
        let yes = pauses.len() % 2 == 0;
        match &pause {
            Pause::End => {}
            Pause::RawTextTag(_) => scanner.resume_after_tag(yes),
            Pause::Cdata => scanner.resume_at_cdata(yes),
        }
        if pause != Pause::End {
            pauses.push((cut, format!("{pause:?}")));
        }
        start = cut;
    }
    (pauses, false)
}

#[test]
fn state_carries_across_scan_calls() {
    for (page, _) in cases() {
        let label: String = page.chars().take(60).collect();
        assert_eq!(
            scan_trace(&page, 1),
            scan_trace(&page, page.len()),
            "{label:?}"
        );
    }
}

#[test]
fn end_tag_attributes_are_counted() {
    let end = |count: usize| format!("</p{}>", attrs(count));
    assert_eq!(
        parse(&end(MAX_TAG_ATTRIBUTES + 1), false).unwrap_err(),
        ExtractError::Rejected(Rejection::TreeTooLarge(TreeLimit::TagAttributes))
    );
    assert!(parse(&end(MAX_TAG_ATTRIBUTES), false).is_ok());
}

#[test]
fn duplicate_names_are_counted() {
    let html = format!("<x{}>", " a".repeat(MAX_TAG_ATTRIBUTES + 1));
    assert_eq!(
        parse(&html, false).unwrap_err(),
        ExtractError::Rejected(Rejection::TreeTooLarge(TreeLimit::TagAttributes))
    );
}
