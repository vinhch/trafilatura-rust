#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Ports of upstream `tests/baseline_tests.py`, case by case with the upstream names: all 23 tests.
//!
//! The upstream tests parse with `lxml.html` (libxml2). Where the html5 tree differs, the test
//! asserts the value the oracle gives on the html5 tree, with a `deviation: D-2` comment
//! (`test_html2txt`: the feed and the `svg` text). `html2txt("123")` is not one of them: the load
//! rejects a fragment lxml returns a childless element for, which is what the plain lxml value
//! shows. Tests that build a tree from a string (`html.fromstring`) use `trees::lxml_fromstring`.
//! `test_build_body_dedupe_cap` monkeypatches `DEDUPE_SCAN_CAP` upstream; here `build_body` takes
//! the cap.

use serde_json::json;
use trafilatura_rust::__internals::{
    DEDUPE_SCAN_CAP, HtmlInput, LocalName, NodeId, Ns, ParseLimits, Tree, baseline, baseline_tree,
    build_body, html2txt, html2txt_tree,
};
use trafilatura_rust::Call;

#[allow(dead_code)]
mod trees;
use trees::lxml_fromstring;

/// `baseline(doc)` on a string.
fn baseline_str(doc: &str) -> (Tree, NodeId, String, usize) {
    baseline(
        HtmlInput::Str(doc),
        &ParseLimits::default(),
        &Call::default(),
    )
    .unwrap()
}

/// The text `baseline(doc)` returns.
fn text_of(doc: &str) -> String {
    baseline_str(doc).2
}

/// `html2txt(doc)` on a string with cleaning.
fn html2txt_str(doc: &str) -> String {
    html2txt(
        HtmlInput::Str(doc),
        true,
        &ParseLimits::default(),
        &Call::default(),
    )
    .unwrap()
}

/// `html2txt` on a tree, with cleaning.
fn html2txt_of(tree: &mut Tree, root: NodeId) -> String {
    html2txt_tree(tree, root, true, &Call::default()).unwrap()
}

/// `html.escape(s, quote=True)`.
fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

/// Wraps a JSON-LD payload (and optional body HTML) into a minimal page.
fn jsonld_doc(payload: &str, body: &str) -> String {
    format!(
        r#"<html><head><script type="application/ld+json">{payload}</script></head><body>{body}</body></html>"#
    )
}

#[test]
fn test_baseline() {
    // Empty input
    for input in [HtmlInput::Bytes(b""), HtmlInput::Str("")] {
        let (tree, body, text, len) =
            baseline(input, &ParseLimits::default(), &Call::default()).unwrap();
        assert_eq!(tree.tag(body), Some("body"));
        assert_eq!(text, "");
        assert_eq!(len, 0);
    }

    // Invalid HTML
    let (_, _, result, _) = baseline(
        HtmlInput::Bytes(b"<invalid html>"),
        &ParseLimits::default(),
        &Call::default(),
    )
    .unwrap();
    assert_eq!(result, "");

    let article = "The article consists of this text.";
    let tests = [
        (
            format!(
                "<html><body><article>{}</article></body></html>",
                article.repeat(10)
            ),
            article,
        ),
        (
            format!("<html><body><article><b>{article}</b></article></body></html>"),
            article,
        ),
        (
            "<html><body><quote>This is only a quote but it is better than nothing.</quote></body></html>".to_string(),
            "This is only a quote but it is better than nothing.",
        ),
    ];
    for (doc, expected) in &tests {
        assert!(text_of(doc).contains(expected));
    }

    // Invalid JSON
    let filecontent = br#"
        <html>
            <body>
                <script type="application/ld+json">
                    {"articleBody": "This is the article body, it has to be long enough to fool the length threshold which is set at len 100."  # invalid JSON
                </script>
            </body>
        </html>
    "#;
    let (_, _, result, _) = baseline(
        HtmlInput::Bytes(filecontent),
        &ParseLimits::default(),
        &Call::default(),
    )
    .unwrap();
    assert_eq!(result, "");

    // JSON OK
    let filecontent = br#"
        <html>
            <body>
                <script type="application/ld+json">
                    {
                        "@type": "Article",
                        "articleBody": "This is the article body, it has to be long enough to fool the length threshold which is set at len 100."
                    }
                </script>
            </body>
        </html>
    "#;
    let (_, _, result, _) = baseline(
        HtmlInput::Bytes(filecontent),
        &ParseLimits::default(),
        &Call::default(),
    )
    .unwrap();
    assert!(result.chars().count() > 100);

    // articleBody wrapped in a single paragraph: markup is parsed, not returned literally
    let filecontent = br#"
        <html>
            <body>
                <script type="application/ld+json">
                    {
                        "@type": "Article",
                        "articleBody": "<p>This is the article body, it has to be long enough to fool the length threshold which is set at len 100.<\/p>"
                    }
                </script>
            </body>
        </html>
    "#;
    let (_, _, result, _) = baseline(
        HtmlInput::Bytes(filecontent),
        &ParseLimits::default(),
        &Call::default(),
    )
    .unwrap();
    assert!(result.starts_with("This is the article body"));
    assert!(!result.contains("<p>"));

    // Real-world examples
    let my_document = r#"<html><body><script type="application/ld+json">{"description":"In letzter Zeit kam man am Begriff \"Hygge\", was so viel wie \"angenehm\" oder \"gemütlich\" bedeutet, ja nicht vorbei. Jetzt macht ihm ein neuer Glücks-Trend ...","image":[{"name":"Mit der Ikigai-Methode wirst du glücklicher","url":"https:\/\/image.brigitte.de\/10973004\/uncropped-0-0\/7d00b2658fd0a3b19e1b161f4657cc20\/Xw\/ikigai--1-.jpg","width":"2048","height":"1366","@type":"ImageObject"},{"name":"Mit der Ikigai-Methode wirst du glücklicher","url":"https:\/\/image.brigitte.de\/10973004\/16x9-1280-720\/bf947c7c24167d7c0adae0be10942d57\/Uf\/ikigai--1-.jpg","width":"1280","height":"720","@type":"ImageObject"},{"name":"Mit der Ikigai-Methode wirst du glücklicher","url":"https:\/\/image.brigitte.de\/10973004\/16x9-938-528\/bf947c7c24167d7c0adae0be10942d57\/JK\/ikigai--1-.jpg","width":"938","height":"528","@type":"ImageObject"},{"name":"Mit der Ikigai-Methode wirst du glücklicher","url":"https:\/\/image.brigitte.de\/10973004\/large1x1-622-622\/f5544b7d67e1be04f7729b130e7e0485\/KN\/ikigai--1-.jpg","width":"622","height":"622","@type":"ImageObject"}],"mainEntityOfPage":{"@id":"https:\/\/www.brigitte.de\/liebe\/persoenlichkeit\/ikigai-macht-dich-sofort-gluecklicher--10972896.html","@type":"WebPage"},"headline":"Ikigai macht dich sofort glücklicher!","datePublished":"2019-06-19T14:29:08+0000","dateModified":"2019-06-19T14:29:10+0000","author":{"name":"BRIGITTE.de","@type":"Organization"},"publisher":{"name":"BRIGITTE.de","logo":{"url":"https:\/\/image.brigitte.de\/11476842\/uncropped-0-0\/f19537e97b9189bf0f25ce924168bedb\/kK\/bri-logo-schema-org.png","width":"167","height":"60","@type":"ImageObject"},"@type":"Organization"},"articleBody":"In letzter Zeit kam man am Begriff \"Hygge\" (\"gemütlich\" oder \"angenehm\") nicht vorbei. Jetzt macht ihm ein neuer Glücks-Trend Konkurrenz: \"Ikigai\". Bist du glücklich? Schwierige Frage, nicht wahr? Viele von uns müssen da erst mal überlegen.","@type":"NewsArticle"}</script></body></html>"#;
    let result = text_of(my_document);
    assert!(result.starts_with("In letzter Zeit kam man"));
    assert!(result.ends_with("erst mal überlegen."));

    let my_document = "<html><body><div>   Document body...   </div><script> console.log('Hello world') </script></body></html>";
    assert_eq!(text_of(my_document), "Document body...");
}

#[test]
fn test_baseline_no_nested_element_duplication() {
    // regression #849/#884: a nested scraped element (inline <code>, <p> in <blockquote>) must
    // not be scraped twice.
    let filler = "Some surrounding paragraph text that is comfortably long enough to pass the one hundred character length gate.";
    let snippet = "pip install trafilatura";
    let doc = format!(
        "<html><body><p>{filler}</p><p>Use <code>{snippet}</code> for setup.</p></body></html>"
    );
    assert_eq!(text_of(&doc).matches(snippet).count(), 1);
    let doc = format!(
        "<html><body><blockquote><p>{filler}</p><p>Second paragraph inside the quote.</p></blockquote></body></html>"
    );
    assert_eq!(text_of(&doc).matches(filler).count(), 1);
}

#[test]
fn test_baseline_strategy_fallthrough() {
    // regression: a too-short JSON-LD body must not leave a stale postbody that blocks later
    // strategies.
    let para = "Real paragraph content that should be extracted by the paragraph strategy, comfortably long enough for the gate.";
    let (_, _, result, length) = baseline_str(&jsonld_doc(
        r#"{"articleBody": "Too short."}"#,
        &format!("<p>{para}</p>"),
    ));
    assert!(result.contains(para));
    assert!(length > 100);
}

#[test]
fn test_baseline_jsonld_shapes() {
    // regression: articleBody must be found in list-wrapped and @graph-nested JSON-LD, not just
    // flat dicts.
    let body_text = "Body text from structured data, made comfortably long enough to pass the one hundred character length threshold.";
    let flat = format!(r#"{{"articleBody": "{body_text}"}}"#);
    let graph = format!(
        r#"{{"@context": "https://schema.org", "@graph": [{{"@type": "Article", "articleBody": "{body_text}"}}]}}"#
    );
    let wrapped = format!(r#"[{{"@type": "Article", "articleBody": "{body_text}"}}]"#);
    // non-dict list item is skipped
    let mixed = format!(r#"[42, {{"@type": "Article", "articleBody": "{body_text}"}}]"#);
    for payload in [flat, graph, wrapped, mixed] {
        assert_eq!(
            text_of(&jsonld_doc(&payload, ""))
                .matches(body_text)
                .count(),
            1
        );
    }
}

#[test]
fn test_baseline_jsonld_content() {
    // regression: articleBody with raw control chars, p-less markup or HTML-escaped markup yields
    // clean text.
    let run = |body: &str| text_of(&jsonld_doc(&format!(r#"{{"articleBody": "{body}"}}"#), ""));

    // raw newline/tab inside the JSON string (invalid in strict mode but common in the wild)
    let ctrl = "First line of the body text\n\twith a raw newline and a tab inside the JSON string, comfortably long enough to pass the gate.";
    assert!(run(ctrl).contains("First line of the body text with a raw newline"));
    // markup without <p> must not leak literal tags
    let markup = "Opening text<br/>more after a break element<div>and inside a div</div> comfortably long enough to pass the one hundred character gate.";
    assert!(!run(markup).contains('<'));
    assert!(run(markup).contains("Opening text"));
    // HTML-escaped markup must be unescaped and parsed, not emitted as entities
    let escaped = "&lt;p&gt;Escaped paragraph body text that should come out clean, comfortably long enough to pass the one hundred character gate.&lt;/p&gt;";
    let result = run(escaped);
    assert!(!result.contains("&lt;"));
    assert!(result.contains("Escaped paragraph body text"));
    // a lone comparison operator in plain prose stays intact
    let prose = "Comparing a < b in plain prose should stay intact, with padding to exceed the one hundred character gate.";
    assert!(run(prose).contains("a < b"));
    // tag-shaped comparison expressions (a letter right after "<") must not be swallowed either
    let expr = "For x<y and z>0 the series converges absolutely, with plenty of extra padding to exceed the length gate here.";
    assert!(run(expr).contains("x<y and z>0"));
    // regression (review round 3): the letters after "<" are single-letter TAG names (p, q, u, s,
    // i, b) -- the old greedy [^>]* matched "<q ... c>" as one tag and deleted the whole span
    let tagletters = "If p<q then u<s or i<b and c>d holds, with extra padding to comfortably exceed the one hundred character gate.";
    assert!(run(tagletters).contains("p<q"));
    assert!(run(tagletters).contains("i<b and c>d holds"));
}

#[test]
fn test_baseline_teaser_does_not_shadow_longer_body() {
    // regression (review round 3): a schema.org Product/VideoObject description is a short teaser
    // and must not preempt a longer whole-body dump carrying the real content in bare divs (no
    // <article>, no <p>-family, so no earlier tier reaches it).
    let desc = "Short marketing product blurb standing in as a JSON-LD teaser, just past the one hundred character content gate here.";
    let divs: String = (0..5)
        .map(|i| {
            format!(
                "<div>Real article paragraph number {i} with genuine sentence content that only the \
                 whole-body dump captures from these bare div elements on the page.</div>"
            )
        })
        .collect();
    let jsonld = format!(r#"{{"@type": "Product", "description": "{desc}"}}"#);
    let text = text_of(&jsonld_doc(&jsonld, &divs));
    assert!(text.contains("Real article paragraph number 3"));
}

#[test]
fn test_baseline_teaser_used_when_body_shorter() {
    // the teaser still wins when the body dump is shorter than it (near-empty body, e.g. a JS
    // shell).
    let desc = "Full product description carrying the actual page content, comfortably longer than the sparse body below and past the gate.";
    let jsonld = format!(r#"{{"@type": "Product", "description": "{desc}"}}"#);
    let text = text_of(&jsonld_doc(&jsonld, "<div>x</div>"));
    assert!(text.contains("Full product description"));
}

#[test]
fn test_baseline_description_teaser_tier() {
    // Product/VideoObject descriptions are last-resort teasers: used when alone on the page,
    // outranked by any full-text source (a JSON articleBody or an HTML paragraph).
    let teaser = "Description teaser standing in for page content, comfortably longer than the one hundred character gate here.";
    let fulltext = "The full body text which must take precedence over the description teaser and clears the gate comfortably.";
    for teaser_type in ["Product", "VideoObject"] {
        let teaser_json = json!({"@type": teaser_type, "description": teaser}).to_string();
        // alone -> used
        assert!(text_of(&jsonld_doc(&teaser_json, "")).contains(teaser));
        let both = json!([
            {"@type": teaser_type, "description": teaser},
            {"@type": "Article", "articleBody": fulltext},
        ])
        .to_string();
        // JSON full-text property wins
        let outranked = text_of(&jsonld_doc(&both, ""));
        assert!(outranked.contains(fulltext));
        assert!(!outranked.contains(teaser));
        // HTML paragraph wins
        let outranked = text_of(&jsonld_doc(&teaser_json, &format!("<p>{fulltext}</p>")));
        assert!(outranked.contains(fulltext));
        assert!(!outranked.contains(teaser));
    }
}

#[test]
fn test_baseline_jsonld_double_embed() {
    // regression: the same JSON-LD block embedded twice (theme and SEO plugin both emitting it)
    // must not duplicate the article body; same for duplicated teaser descriptions.
    let body = "Article body text embedded twice via duplicated JSON-LD scripts, comfortably longer than the one hundred character strategy gate.";
    let desc = "Video description standing in for page content here, comfortably longer than the one hundred character gate.";
    for (content, payload) in [
        (body, format!(r#"{{"articleBody": "{body}"}}"#)),
        (
            desc,
            format!(r#"{{"@type": "VideoObject", "description": "{desc}"}}"#),
        ),
    ] {
        let script = format!(r#"<script type="application/ld+json">{payload}</script>"#);
        let text = text_of(&format!(
            "<html><head>{script}{script}</head><body></body></html>"
        ));
        assert_eq!(text.matches(content).count(), 1);
    }
}

#[test]
fn test_baseline_control_chars_tree_input() {
    // regression: non-whitespace control chars survive json.loads(strict=False) and caller-built
    // trees (trafilatura's parser strips them from str input, lxml.html.fromstring does not);
    // they must not crash the lxml .text assignment at any baseline sink (JSON/paragraph
    // _build_body, whole-body dump).
    let ctrl = "before\x01after, with plenty of padding words to pass the one hundred character strategy gate comfortably.";
    for doc in [
        jsonld_doc(&format!(r#"{{"articleBody": "{ctrl}"}}"#), ""),
        format!("<html><body><p>{ctrl}</p></body></html>"),
        format!("<html><body><div>{ctrl}</div></body></html>"),
    ] {
        let (mut tree, root) = lxml_fromstring(&doc);
        let (_, text, length) =
            baseline_tree(&mut tree, root, &ParseLimits::default(), &Call::default()).unwrap();
        assert!(length > 100);
        assert!(!text.contains('\x01'));
        assert!(text.contains("before"));
        assert!(text.contains("after"));
    }
}

#[test]
fn test_baseline_nested_paragraph_not_duplicated() {
    // regression: a <p> nested in a <blockquote> with surrounding text must not be emitted twice
    // by the paragraph strategy (its text is a substring of the container's, not an exact match).
    let para = "Quoted paragraph text repeated by nesting here, made comfortably longer than the one hundred character strategy gate threshold.";
    let doc = format!(
        "<html><body><blockquote>Attribution line: <p>{para}</p></blockquote></body></html>"
    );
    assert_eq!(text_of(&doc).matches(para).count(), 1);
}

#[test]
fn test_baseline_paragraph_dedup_keeps_short_repeats() {
    // regression: the paragraph strategy's substring dedup must not drop a short, distinct
    // paragraph just because its text recurs inside earlier, unrelated prose.
    let long_para = "The annual conference will be held in Berlin this coming September, organizers said today.";
    let short_para = "held in Berlin";
    let doc = format!("<html><body><p>{long_para}</p><p>{short_para}</p></body></html>");
    // once inside long_para, once standalone
    assert_eq!(text_of(&doc).matches(short_para).count(), 2);
}

#[test]
fn test_baseline_dedup_keeps_cross_boundary_paragraph() {
    // regression (review round 3): the substring dedup accumulator must not drop a distinct
    // paragraph whose text spans the join boundary of two earlier consecutive paragraphs -- a
    // space-joined accumulator saw it as a repeat (fixed by joining on a newline, which trimmed
    // paragraph text never contains). Asserts on emitted paragraphs, not the joined string,
    // which would still contain the phrase as a cross-boundary substring.
    let p1 = "The meeting concluded with remarks about the annual budget";
    let p2 = "planning process that begins next week according to officials.";
    // tail of p1 + head of p2, >50 chars
    let p3 = "annual budget planning process that begins next week";
    let mut tree = Tree::new(LocalName::from("html"), Ns::Html, Vec::new());
    let (body, _) = build_body(
        &mut tree,
        vec![p1.to_string(), p2.to_string(), p3.to_string()],
        true,
        DEDUPE_SCAN_CAP,
        &Call::default(),
    )
    .unwrap();
    assert_eq!(tree.child_count(body), 3);
    let third = tree.child_at(body, 2).unwrap();
    assert_eq!(tree.node(third).text.as_deref(), Some(p3));
}

#[test]
fn test_baseline_article_nested_not_duplicated() {
    // regression: a nested <article> (article-in-article shell) must not be counted both inside
    // its ancestor's text and again standalone by the article strategy.
    let inner = "Inner article content sentence repeated to build a properly long body text here. "
        .repeat(3);
    let doc = format!(
        "<html><body><article>Outer wrapper. <article>{inner}</article></article></body></html>"
    );
    assert_eq!(text_of(&doc).matches(inner.trim()).count(), 1);
}

#[test]
fn test_baseline_schema_properties() {
    // schema.org text properties beyond articleBody: recipe steps and FAQ answers.
    let recipe = json!({
        "@type": "Recipe",
        "recipeInstructions": [
            {
                "@type": "HowToStep",
                "text": "Mix the flour with sugar and butter until the dough is smooth and pliable in texture.",
            },
            {
                "@type": "HowToStep",
                "text": "Bake for thirty five minutes at one hundred eighty degrees until golden brown on top.",
            },
        ],
    })
    .to_string();
    let result = text_of(&jsonld_doc(&recipe, ""));
    assert!(result.contains("Mix the flour"));
    assert!(result.contains("Bake for thirty five minutes"));

    // recipeInstructions can also be plain strings rather than step objects
    let str_recipe = json!({
        "@type": "Recipe",
        "recipeInstructions": [
            "Combine all the dry ingredients thoroughly in a large mixing bowl before adding any liquids.",
            "Pour the batter into the tin and bake until a skewer inserted in the centre comes out clean.",
        ],
    })
    .to_string();
    let result = text_of(&jsonld_doc(&str_recipe, ""));
    assert!(result.contains("Combine all the dry ingredients"));
    assert!(result.contains("Pour the batter into the tin"));

    let faq = json!({
        "@type": "FAQPage",
        "mainEntity": [
            {
                "@type": "Question",
                "name": "How?",
                "acceptedAnswer": {
                    "@type": "Answer",
                    "text": "This is the accepted answer to the question, written with a comfortable amount of words to pass the one hundred character gate.",
                },
            }
        ],
    })
    .to_string();
    let result = text_of(&jsonld_doc(&faq, ""));
    assert!(result.contains("This is the accepted answer"));
}

#[test]
fn test_baseline_discourse_preload() {
    // Discourse forums: posts live as JSON in the data-preloaded attribute of an empty page.
    let topic = json!({
        "post_stream": {
            "posts": [
                {"cooked": "<p>First forum post with a good amount of substantive discussion content in it.</p>"},
                {"cooked": "<p>Second forum post continuing the thread with more useful information for everyone.</p>"},
            ]
        }
    })
    .to_string();
    // a non-"topic_" key (e.g. site settings) is skipped; only the topic_ key is parsed
    let preloaded = escape(&json!({"site_settings": "ignored", "topic_12": topic}).to_string());
    let doc = format!(
        r#"<html><body><div id="data-preloaded" data-preloaded="{preloaded}"></div></body></html>"#
    );
    let result = text_of(&doc);
    assert!(result.contains("First forum post"));
    assert!(result.contains("Second forum post"));
    assert!(!result.contains("<p>"));
}

#[test]
fn test_baseline_discourse_preload_malformed() {
    // regression: valid but non-dict JSON in data-preloaded (or a non-dict post entry) must not
    // crash baseline() -- it should just fall through to the next strategy.
    let para = "Real paragraph content that should be extracted by the paragraph strategy, comfortably long enough for the gate.";
    for preloaded in [
        "[]".to_string(),
        r#""just a string""#.to_string(),
        escape(&json!({"topic_12": r#"["not", "a", "dict"]"#}).to_string()),
    ] {
        let doc = format!(
            r#"<html><body><div id="data-preloaded" data-preloaded="{preloaded}"></div><p>{para}</p></body></html>"#
        );
        assert!(text_of(&doc).contains(para));
    }
}

#[test]
fn test_build_body_dedupe_cap() {
    // past DEDUPE_SCAN_CAP the paragraph strategy skips the substring scan and keeps potential
    // repeats (cf. recover_wild_text).
    let filler =
        "Opening paragraph content long enough to push the accumulated text past a tiny cap.";
    let dup = "This exact paragraph repeats and is well above the duplicate length gate for sure.";
    let texts = || vec![filler.to_string(), dup.to_string(), dup.to_string()];
    let mut tree = Tree::new(LocalName::from("html"), Ns::Html, Vec::new());

    let (_, text) = build_body(&mut tree, texts(), true, 50, &Call::default()).unwrap();
    // past the cap: scan skipped, repeat kept
    assert_eq!(text.matches(dup).count(), 2);

    let (_, text) = build_body(&mut tree, texts(), true, 200_000, &Call::default()).unwrap();
    // under the cap: substring dedup fires
    assert_eq!(text.matches(dup).count(), 1);
}

#[test]
fn test_baseline_howto() {
    // HowTo step/itemListElement text is content (own text and nested itemListElement both kept).
    let step_text = "Measure the table and add the overhang you want on every side before cutting any fabric at all.";
    let howto = json!({
        "@type": "HowTo",
        "step": [
            {
                "@type": "HowToSection",
                "itemListElement": {
                    "@type": "HowToDirection",
                    "text": format!("{step_text} Then hem the edges neatly all around the cloth."),
                },
            }
        ],
    })
    .to_string();
    assert!(text_of(&jsonld_doc(&howto, "")).contains(step_text));

    // a step carrying BOTH its own text and an itemListElement keeps both
    let both = json!({
        "@type": "HowTo",
        "step": [
            {
                "@type": "HowToStep",
                "text": "Own step text: preheat the oven to a moderate temperature first.",
                "itemListElement": [
                    {"@type": "HowToDirection", "text": "Set the dial to one hundred and eighty degrees."}
                ],
            }
        ],
    })
    .to_string();
    let result = text_of(&jsonld_doc(&both, ""));
    assert!(result.contains("preheat the oven"));
    assert!(result.contains("one hundred and eighty"));
}

#[test]
fn test_baseline_element_input_not_mutated() {
    // the public baseline() must not modify a caller-supplied tree (mirrors html2txt's guard)
    let doc = format!(
        "<html><body><aside>side text</aside><p>{}</p></body></html>",
        "Real paragraph content on the page. ".repeat(4)
    );
    let (mut tree, root) = lxml_fromstring(&doc);
    let before = tree.tostring(root, false, true);
    let (_, text, _) =
        baseline_tree(&mut tree, root, &ParseLimits::default(), &Call::default()).unwrap();
    assert!(text.contains("Real paragraph content"));
    assert!(!text.contains("side text"));
    // basic_cleaning ran on a copy
    assert!(tree.find(root, "aside").is_some());
    assert_eq!(tree.tostring(root, false, true), before);
}

#[test]
fn test_baseline_article_dominance() {
    // article strategy: a dominant article drops much smaller siblings (teasers), similar-sized
    // ones all stay.
    // ~600 chars
    let main =
        "Main article content sentence repeated to build a properly dominant body. ".repeat(8);
    // >100, < max/5
    let teaser = "Related article teaser text that comfortably exceeds the length gate....";
    let teaser_head: String = teaser.chars().take(40).collect();
    let doc = format!(
        "<html><body><article>{main}</article><article>{teaser}{teaser_head}</article></body></html>"
    );
    let result = text_of(&doc);
    assert!(result.contains("Main article content"));
    assert!(!result.contains("Related article teaser"));

    let post = |i: usize| {
        format!(
            "Forum post number {i} with a comparable amount of discussion text in every single post here."
        )
    };
    let doc2 = format!(
        "<html><body>{}</body></html>",
        (0..3)
            .map(|i| format!("<article>{}</article>", post(i)))
            .collect::<String>()
    );
    let result2 = text_of(&doc2);
    assert!((0..3).all(|i| result2.contains(&post(i))));
}

#[test]
fn test_baseline_minified_block_boundaries() {
    // no whitespace at block boundaries must not fuse words (GH #896), in any strategy
    let sentence = |i: usize| {
        format!("Sentence number {i} of the notice, long enough to clear the content length gate.")
    };
    let blocks: String = (0..3).map(|i| format!("<p>{}</p>", sentence(i))).collect();
    // article strategy, then paragraph strategy (a <br> inside the paragraph)
    for doc in [
        format!("<html><body><article><h1>Notice Title</h1>{blocks}</article></body></html>"),
        format!(
            "<html><body><div><p>{}<br/>{}</p>{blocks}</div></body></html>",
            sentence(0),
            sentence(1)
        ),
    ] {
        let result = text_of(&doc);
        assert!(!result.contains("notice.Sentence"));
        assert!(!result.contains("TitleSentence"));
        assert!(result.contains(&sentence(1)));
    }

    // markup arriving escaped inside embedded JSON
    let body = escape(&blocks);
    let doc = format!(
        r#"<html><body><script type="application/ld+json">{{"articleBody": "{body}"}}</script></body></html>"#
    );
    let result = text_of(&doc);
    assert!(!result.contains("notice.Sentence"));
    assert!(result.contains(&sentence(1)));
}

#[test]
fn test_html2txt() {
    let mydoc = "<html><body>Here is the body text</body></html>";
    assert_eq!(html2txt_str(mydoc), "Here is the body text");
    let (mut tree, root) = lxml_fromstring(mydoc);
    assert_eq!(html2txt_of(&mut tree, root), "Here is the body text");
    assert_eq!(html2txt_str(""), "");
    // oracle: html2txt("123") on plain lxml, which rejects the fragment (`html5_parsers` would
    // keep it and give "123")
    assert_eq!(html2txt_str("123"), "");
    assert_eq!(html2txt_str("<html></html>"), "");
    assert_eq!(html2txt_str("<html><body/></html>"), "");
    assert_eq!(
        html2txt_str("<html><body><style>font-size: 8pt</style><p>ABC</p></body></html>"),
        "ABC"
    );
    // block boundaries stay separated on minified pages, inline runs stay joined
    let minified = "<html><body><p>First block.</p><p>Second block.</p><h2>Heading</h2>line<br>break</body></html>";
    assert_eq!(
        html2txt_str(minified),
        "First block. Second block. Heading line break"
    );
    assert_eq!(
        html2txt_str("<html><body><div>a<p>b</p></div></body></html>"),
        "a b"
    );
    assert_eq!(
        html2txt_str("<html><body><p>Hyper<b>link</b></p></body></html>"),
        "Hyperlink"
    );
    // an element input is left untouched by the cleaning
    let (mut tree, root) =
        lxml_fromstring("<html><body><aside>side</aside><p>content</p></body></html>");
    assert_eq!(html2txt_of(&mut tree, root), "content");
    assert!(tree.text_content(root).contains("side"));
    // an element without a body still yields its text
    let (mut tree, root) = lxml_fromstring("<div><p>bare element text</p></div>");
    assert_eq!(html2txt_of(&mut tree, root), "bare element text");
    // ... but a parsed document without a body (e.g. a feed) is not HTML text:
    // cli --probe relies on "" here to skip non-HTML URLs
    let feed = concat!(
        r#"<?xml version="1.0"?><feed xmlns="http://www.w3.org/2005/Atom"><title>Feed title</title>"#,
        "<entry><title>Entry one</title><summary>Summary one</summary></entry>",
        "<entry><title>Entry two</title><summary>Summary two</summary></entry></feed>"
    );
    // deviation: D-2
    // oracle: html2txt(feed) under html5_parsers: the html5 tree gives the page a body, where
    // libxml2 gives none and upstream asserts ""
    assert_eq!(
        html2txt_str(feed),
        "Feed titleEntry one Summary one Entry two Summary two"
    );
    // spec-invisible content is not text: template is never rendered, svg text is graphical
    let invisible = "<html><body><svg><title>Icon name</title></svg><template><p>markup</p></template><p>visible</p></body></html>";
    // deviation: D-2
    // oracle: html2txt(invisible) under html5_parsers: the html5 svg carries a namespace, so the
    // cleaning's `svg` name test skips it and upstream asserts "visible"
    assert_eq!(html2txt_str(invisible), "Icon name visible");
    // regression (review round 3): a caller-supplied tree can hold control chars lxml rejects on
    // write; html2txt must not raise (it would null the whole extract via the escalation gate).
    // the control char is dropped (as load_html does for string input), joining the two words
    let (mut ctrl_tree, root) =
        lxml_fromstring("<html><body><p>before\x01after visible text</p></body></html>");
    assert_eq!(
        html2txt_of(&mut ctrl_tree, root),
        "beforeafter visible text"
    );
}
