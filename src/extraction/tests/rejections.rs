use super::*;

fn rejected(rejection: Rejection) -> ExtractError {
    ExtractError::Rejected(rejection)
}

#[test]
fn extract_python_format_rejects_after_extraction() {
    let options = bare_options(Format::Python, true);
    // oracle: core.extract(page, options=Extractor(output_format="python", fast=True, dedup=False))
    //   raises ValueError("'python' format only usable in bare_extraction() function")
    assert_eq!(
        internal_page(&main_page(""), None, &options).unwrap_err(),
        rejected(Rejection::InvalidValue)
    );
    // oracle: core.extract(x, options=<same>) is None for x = "" and x = a page with an empty body
    assert_eq!(
        internal_page("", None, &options).unwrap_err(),
        rejected(Rejection::EmptyTree)
    );
    assert_eq!(
        internal_page(&full_page("", ""), None, &options).unwrap_err(),
        rejected(Rejection::TooShort {
            text_chars: 0,
            comments_chars: 0
        })
    );
}

#[test]
fn max_tree_size_rejects() {
    let hi = "<p><hi rend=\"#i\">abc</hi></p>";
    let plain = "<p>abc</p>";
    // oracle: o = Extractor(output_format="python", fast=True, dedup=False); o.formatting = f;
    //   o.max_tree_size = limit; core.bare_extraction(page, options=o) inside html5_parsers(), page
    //   a body of `count` copies of the unit, as a string and as utils.load_html(page); None, or
    //   len(d.body)
    // (unit, count, formatting, limit, children of the body, None when rejected)
    let rows = [
        (hi, 499, true, Some(500), Some(499)),
        (hi, 501, true, Some(500), None),
        (hi, 501, true, Some(0), Some(501)),
        (hi, 501, true, None, Some(501)),
        (plain, 50, false, Some(500), Some(50)),
        (plain, 500, false, Some(500), Some(500)),
        (plain, 501, false, Some(500), None),
    ];
    for (unit, count, formatting, limit, children) in rows {
        let page = full_page(&unit.repeat(count), "");
        let mut options = bare_options(Format::Python, true);
        options.formatting = formatting;
        options.max_tree_size = limit;
        let tree = load(&page);
        let root = tree.root();
        for (kind, result) in [
            ("string", bare_page(&page, &options)),
            ("tree", bare(Source::Tree(&tree, root), &options)),
        ] {
            let label = format!("{count} x {unit} limit {limit:?} {kind}");
            match (children, result) {
                (Some(n), Ok(doc)) => assert_eq!(doc.tree.child_count(doc.body), n, "{label}"),
                // each unit is one child of the body, so the count after the strip is `count`
                (None, Err(error)) => assert_eq!(
                    error,
                    rejected(Rejection::TreeTooLarge(TreeLimit::MaxTreeSize {
                        children: count
                    })),
                    "{label}"
                ),
                (_, result) => panic!("{label}: {:?}", result.map(|doc| doc.raw_text)),
            }
        }
    }
}

/// An element with `text`, `tail` and `children`.
fn node(
    tree: &mut Tree,
    name: &str,
    text: Option<&str>,
    tail: Option<&str>,
    children: &[NodeId],
) -> NodeId {
    let id = element(tree, name, text, tail);
    for &child in children {
        tree.append(id, child);
    }
    id
}

#[test]
fn max_tree_size_strips_hi_before_counting() {
    // (limit, children as built, `Ok` or the error's count after the strip, the body afterwards)
    // oracle: core.bare_extraction(page, options=o) inside html5_parsers(), o.max_tree_size = limit,
    //   core.trafilatura_sequence stubbed to return (body, "some text", Element("body"), ""):
    //   d is not None, and etree.tostring(body, with_tail=False)
    type Build = fn(&mut Tree) -> NodeId;
    let flat: Build = |tree| {
        let hi = ["a", "b", "c"].map(|text| element(tree, "hi", Some(text), None));
        node(tree, "body", None, None, &hi)
    };
    let mixed: Build = |tree| {
        let inner = element(tree, "hi", Some("b"), None);
        let outer = node(tree, "hi", Some("a"), None, &[inner]);
        let (x, y) = (
            element(tree, "p", Some("x"), None),
            element(tree, "p", Some("y"), None),
        );
        node(tree, "body", None, None, &[x, outer, y])
    };
    let stuck: Build = |tree| {
        let mut children: Vec<NodeId> = ["1", "2", "3"]
            .map(|text| element(tree, "p", Some(text), None))
            .into();
        let posts = ["a", "b"].map(|text| element(tree, "p", Some(text), None));
        children.push(node(tree, "hi", None, None, &posts));
        node(tree, "body", None, None, &children)
    };
    let tail: Build = |tree| {
        let (x, y) = (
            element(tree, "p", Some("x"), None),
            element(tree, "p", Some("y"), None),
        );
        let hi = element(tree, "hi", Some("a"), Some("tail"));
        node(tree, "body", None, None, &[x, hi, y])
    };
    let unchanged_stuck = "<body><p>1</p><p>2</p><p>3</p><hi><p>a</p><p>b</p></hi></body>";
    let rows: [(Build, usize, Result<(), ExtractError>, &str); 7] = [
        (flat, 2, Ok(()), "<body>abc</body>"),
        (
            flat,
            3,
            Ok(()),
            "<body><hi>a</hi><hi>b</hi><hi>c</hi></body>",
        ),
        (mixed, 2, Ok(()), "<body><p>x</p>ab<p>y</p></body>"),
        (
            stuck,
            3,
            Err(rejected(Rejection::TreeTooLarge(TreeLimit::MaxTreeSize {
                children: 5,
            }))),
            "<body><p>1</p><p>2</p><p>3</p><p>a</p><p>b</p></body>",
        ),
        (stuck, 4, Ok(()), unchanged_stuck),
        (stuck, 0, Ok(()), unchanged_stuck),
        (tail, 2, Ok(()), "<body><p>x</p>atail<p>y</p></body>"),
    ];
    for (build, limit, expected, xml) in rows {
        let mut tree = Tree::new(LocalName::from("html"), Ns::Html, Vec::new());
        let body = build(&mut tree);
        let outcome = check_tree_size(&mut tree, body, Some(limit));
        assert_eq!(outcome, expected, "limit {limit}");
        assert_eq!(tree.tostring(body, false, false), xml, "limit {limit}");
    }
}

#[test]
fn too_short_rejected() {
    let tiny = "<article><p>Short text only.</p></article>";
    let reply = "<div id=\"comments\"><p>One comment here</p></div>";
    // oracle: o = Extractor(output_format="python", fast=True, dedup=False) with the two minimum
    //   sizes set; core.bare_extraction(page, options=o) inside html5_parsers(): None or a Document
    // (page, min_output_size, min_output_comm_size, the text kept, None when rejected)
    let rows = [
        // an empty body under the default minimums of 1
        (full_page("", ""), 1, 1, None),
        (full_page(tiny, ""), 1, 1, Some("Short text only.")),
        (full_page(tiny, ""), 200, 1, None),
        // the text is short but the comments are long enough for their minimum
        (
            full_page(&format!("{tiny}{reply}"), ""),
            200,
            1,
            Some("Short text only.\nOne comment here"),
        ),
        (full_page(&format!("{tiny}{reply}"), ""), 200, 100, None),
        // the minimum is exclusive: a text of 16 characters passes 16 and fails 17
        (full_page(tiny, ""), 16, 1, Some("Short text only.")),
        (full_page(tiny, ""), 17, 1, None),
    ];
    for (page, min_output, min_comments, kept) in rows {
        let mut options = bare_options(Format::Python, true);
        options.min_output_size = min_output;
        options.min_output_comm_size = min_comments;
        let result = bare_page(&page, &options);
        let label = format!("min {min_output} {min_comments} {kept:?}");
        match kept {
            Some(text) => assert_eq!(result.unwrap().text.as_deref(), Some(text), "{label}"),
            None => assert!(
                matches!(
                    result,
                    Err(ExtractError::Rejected(Rejection::TooShort { .. }))
                ),
                "{label}"
            ),
        }
    }
}

#[test]
fn too_short_reports_char_counts() {
    // The article text has 27 chars and 33 UTF-8 bytes; the comment has 11 chars and 13 bytes.
    // The baseline rescue (extraction.rs, baseline.rs) wins with a text that joins the article and
    // the reply with a newline: 27 + 1 newline + 11 = 39.
    let text = "Café déjà vu, naïve résumé.";
    let reply = "<div id=\"comments\"><p>Réponse ici</p></div>";
    let mut options = bare_options(Format::Python, true);
    options.min_output_size = 100;
    options.min_output_comm_size = 100;
    let page = full_page(&format!("<article><p>{text}</p></article>{reply}"), "");
    assert_eq!(
        bare_page(&page, &options).unwrap_err(),
        rejected(Rejection::TooShort {
            text_chars: 39,
            comments_chars: 11
        })
    );
}

#[test]
fn wrong_html_lang_rejected() {
    let meta = |name: &str, content: &str| {
        let key = if name == "og:locale" {
            "property"
        } else {
            "http-equiv"
        };
        format!("<meta {key}=\"{name}\" content=\"{content}\">")
    };
    let page = |head: &str| {
        full_page(
            &format!("<article><p>{}</p></article>", prose("Main", 5)),
            head,
        )
    };
    let html_lang = format!(
        "<html lang=\"de\"><head><title>T</title></head><body><article><p>{}</p></article></body></html>",
        prose("Main", 5)
    );
    let de = meta("content-language", "de");
    // oracle: core.bare_extraction(page, options=Extractor(output_format="python", fast=True,
    //   dedup=False, lang=target)) inside html5_parsers(): None, or a Document; check_html_lang is
    //   called with strict=False
    // (page, target language, accepted)
    let rows = [
        (page(&de), Some("en"), false),
        (page(&de), Some("de"), true),
        (page(&meta("content-language", "en-US")), Some("en"), true),
        (page(""), Some("en"), true),
        // the `html` lang attribute is read only when strict
        (html_lang, Some("en"), true),
        (page(&meta("og:locale", "fr_FR")), Some("en"), false),
        // the first attribute with a match decides: http-equiv before og:locale
        (
            page(&format!(
                "{}{}",
                meta("content-language", "en"),
                meta("og:locale", "de_DE")
            )),
            Some("en"),
            true,
        ),
        (page(&de), None, true),
    ];
    for (html, target, accepted) in rows {
        let mut options = bare_options(Format::Python, true);
        options.lang = target.map(str::to_owned);
        let result = bare_page(&html, &options);
        let label = format!("{target:?}");
        if accepted {
            assert!(result.is_ok(), "{label}");
        } else {
            assert_eq!(
                result.unwrap_err(),
                rejected(Rejection::WrongHtmlLang),
                "{label}"
            );
        }
    }
}
