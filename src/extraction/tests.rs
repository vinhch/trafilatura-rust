use std::collections::HashSet;

use super::*;
use crate::cancel::CancelToken;
use crate::dom::{LocalName, Ns};
use crate::error::Rejection;
use crate::settings::{Config, MAX_FILE_SIZE};
use crate::utils::{HtmlInput, load_html};

const BS: char = '\u{5c}';

fn options(format: Format, with_metadata: bool) -> Extractor {
    let mut options = Extractor::new(
        Config::default(),
        format,
        false,
        false,
        None,
        false,
        false,
        HashSet::new(),
        HashSet::new(),
    );
    options.with_metadata = with_metadata;
    options
}

fn render(doc: &mut Document, format: Format, with_metadata: bool) -> String {
    determine_returnstring(doc, &options(format, with_metadata), &Call::default()).unwrap()
}

fn strings(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

/// A body with one paragraph holding `text`.
fn paragraph(doc: &mut Document, text: &str) {
    let p = doc.tree.new_element("p");
    doc.tree.append(doc.body, p);
    doc.tree.node_mut(p).text = Some(text.to_owned());
}

#[test]
fn yaml_scalar_matches_python() {
    let quoted = |text: &str| format!("\"{text}\"");
    let rows: Vec<(String, String)> = vec![
        // oracle: _yaml_scalar("\u216b" + "x")
        ("\u{216b}x".into(), quoted("\u{216b}x")),
        // oracle: _yaml_scalar("\u01c5" + "x")
        ("\u{1c5}x".into(), "\u{1c5}x".into()),
        // oracle: _yaml_scalar("x" + chr(0x1c))
        ("x\u{1c}".into(), quoted(&format!("x{BS}u001c"))),
        // oracle: _yaml_scalar("")
        (String::new(), quoted("")),
        // oracle: _yaml_scalar(" a")
        (" a".into(), quoted(" a")),
        // oracle: _yaml_scalar("a ")
        ("a ".into(), quoted("a ")),
        // oracle: _yaml_scalar("a #b")
        ("a #b".into(), quoted("a #b")),
        // oracle: _yaml_scalar("a:")
        ("a:".into(), quoted("a:")),
        // oracle: _yaml_scalar("a: b")
        ("a: b".into(), quoted("a: b")),
        // oracle: _yaml_scalar("Yes")
        ("Yes".into(), quoted("Yes")),
        // oracle: _yaml_scalar("None")
        ("None".into(), quoted("None")),
        // oracle: _yaml_scalar("NULL")
        ("NULL".into(), quoted("NULL")),
        // oracle: _yaml_scalar("~")
        ("~".into(), quoted("~")),
        // oracle: _yaml_scalar("1a")
        ("1a".into(), quoted("1a")),
        // oracle: _yaml_scalar("\u00e9t\u00e9")
        ("\u{e9}t\u{e9}".into(), "\u{e9}t\u{e9}".into()),
        // oracle: _yaml_scalar("a" + chr(0x2028) + "b")
        ("a\u{2028}b".into(), "a\u{2028}b".into()),
        // oracle: _yaml_scalar(chr(0x345) + "x")
        ("\u{345}x".into(), quoted("\u{345}x")),
        // oracle: _yaml_scalar("a" + chr(0x7f) + "b")
        ("a\u{7f}b".into(), quoted("a\u{7f}b")),
        // oracle: _yaml_scalar("a" + chr(0x85) + "b")
        ("a\u{85}b".into(), "a\u{85}b".into()),
        // oracle: _yaml_scalar("a\nb")
        ("a\nb".into(), quoted(&format!("a{BS}nb"))),
        // oracle: _yaml_scalar('say "hi"')
        ("say \"hi\"".into(), "say \"hi\"".into()),
        // oracle: _yaml_scalar("Title")
        ("Title".into(), "Title".into()),
    ];
    for (input, expected) in rows {
        assert_eq!(yaml_scalar(&input), expected, "input {input:?}");
    }
}

#[test]
fn txt_header_matches_python() {
    let mut doc = Document::new();
    doc.title = Some("T".into());
    doc.categories = Some(strings(&["it's", "say \"hi\"", "a\nb", "\u{85}", "\u{e9}"]));
    doc.id = Some("7".into());
    doc.fingerprint = Some(String::new());
    // oracle: determine_returnstring(Document(title="T", categories=["it's", 'say "hi"', "a\nb",
    //   chr(0x85), chr(0xe9)], idval="7", fingerprint=""), Extractor(output_format=fmt,
    //   with_metadata=True)) for fmt in ("txt", "markdown")
    let expected = format!(
        "---\ntitle: T\ncategories: [\"it's\", 'say \"hi\"', 'a{BS}nb', '{BS}x85', '\u{e9}']\nid: \"7\"\n---"
    );
    for format in [Format::Txt, Format::Markdown] {
        assert_eq!(render(&mut doc, format, true), expected, "{format:?}");
    }
    // oracle: determine_returnstring(Document(), Extractor(output_format="txt", with_metadata=True))
    assert_eq!(render(&mut Document::new(), Format::Txt, true), "---\n---");
}

#[test]
fn html_meta_matches_python() {
    let mut doc = Document::new();
    doc.title = Some("T".into());
    doc.categories = Some(strings(&["a", "b"]));
    doc.id = Some("7".into());
    // oracle: determine_returnstring(Document(title="T", categories=["a", "b"], idval="7"),
    //   Extractor(output_format="html", with_metadata=True))
    assert_eq!(
        render(&mut doc, Format::Html, true),
        "<html>\n  <head>\n    <meta name=\"title\" content=\"T\"/>\n    \
         <meta name=\"categories\" content=\"a;b\"/>\n    <meta name=\"id\" content=\"7\"/>\n  \
         </head>\n  <body/>\n</html>"
    );
    // oracle: determine_returnstring(Document(), Extractor(output_format="html", with_metadata=True))
    assert_eq!(
        render(&mut Document::new(), Format::Html, true),
        "<html>\n  <head/>\n  <body/>\n</html>"
    );
}

#[test]
fn empty_document_in_each_format() {
    // oracle: determine_returnstring(Document(), Extractor(output_format=fmt)) for fmt in
    //   ("txt", "markdown", "html")
    for (format, expected) in [
        (Format::Txt, ""),
        (Format::Markdown, ""),
        (Format::Html, "<html>\n  <body/>\n</html>"),
    ] {
        assert_eq!(
            render(&mut Document::new(), format, false),
            expected,
            "{format:?}"
        );
    }
}

#[test]
fn returnstring_is_nfc() {
    // oracle: body = Element("body"); SubElement(body, "p").text = "e" + chr(0x301);
    //   determine_returnstring(Document(body=body), Extractor(output_format=fmt))
    //   for fmt in ("txt", "markdown", "html")
    for (format, expected) in [
        (Format::Txt, "\u{e9}"),
        (Format::Markdown, "\u{e9}"),
        (
            Format::Html,
            "<html>\n  <body>\n    <p>\u{e9}</p>\n  </body>\n</html>",
        ),
    ] {
        let mut doc = Document::new();
        paragraph(&mut doc, "e\u{301}");
        assert_eq!(render(&mut doc, format, false), expected, "{format:?}");
    }
}

// ---------------------------------------------------------------------------- the cascade

const NEL: char = '\u{85}';

/// The options of a cascade run: `Format::Txt`, balanced unless a flag says otherwise.
fn cascade_options(precision: bool, recall: bool, comments: bool) -> Extractor {
    let mut options = Extractor::new(
        Config::default(),
        Format::Txt,
        precision,
        recall,
        None,
        false,
        false,
        HashSet::new(),
        HashSet::new(),
    );
    options.comments = comments;
    options
}

/// A tree of one `html` element holding a `script` with the given `type` and text.
fn script_tree(kind: &str, text: Option<&str>) -> (Tree, NodeId) {
    let mut tree = Tree::new(LocalName::from("html"), Ns::Html, Vec::new());
    let root = tree.root();
    let script = tree.new_element("script");
    tree.set(script, "type", kind);
    tree.node_mut(script).text = text.map(str::to_owned);
    tree.append(root, script);
    (tree, root)
}

fn is_forum(kind: &str, text: Option<&str>) -> bool {
    let (tree, root) = script_tree(kind, text);
    forum_thread_page(&tree, root, &Call::default()).unwrap()
}

fn load(html: &str) -> Tree {
    load_html(
        HtmlInput::Str(html),
        MAX_FILE_SIZE,
        &ParseLimits::default(),
        &Call::default(),
    )
    .unwrap()
}

#[test]
fn forum_pattern_compiles() {
    assert!(DISCUSSION_FORUM_POSTING_RE.is_some());
}

#[test]
fn forum_regex_uses_python_whitespace() {
    let (t, d) = ("\"@type\"", "\"DiscussionForumPosting\"");
    let ld = "application/ld+json";
    // a control character never reaches a loaded page (`repair_faulty_html` strips it), so these go
    // through a hand-built tree
    let rows = [
        // oracle: bool(core._DISCUSSION_FORUM_POSTING_RE.search(s)) for s = '{' + T + ':' + D + '}'
        (format!("{{{t}:{d}}}"), true),
        // ... s = T + "\x1c:\x1c" + D
        (format!("{t}\x1c:\x1c{d}"), true),
        // ... s = T + "\x1d:" + D
        (format!("{t}\x1d:{d}"), true),
        // ... s = T + ":\x1e" + D
        (format!("{t}:\x1e{d}"), true),
        // ... s = T + "\x1f:\x1f" + D
        (format!("{t}\x1f:\x1f{d}"), true),
        // ... s = T + "\x85:" + D
        (format!("{t}{NEL}:{d}"), true),
        // ... s = T + "\x1b:" + D
        (format!("{t}\x1b:{d}"), false),
        // ... s = T + " :" + D
        (format!("{t} :{d}"), true),
        // ... s = T + "\x0b:" + D
        (format!("{t}\x0b:{d}"), true),
        // ... s = T + "\x1c:\x1c" + '["Thing",' + D + "]"
        (format!("{t}\x1c:\x1c[\"Thing\",{d}]"), true),
        // ... s = T + ':\x85["Thing",\x85' + D + "]"
        (format!("{t}:{NEL}[\"Thing\",{NEL}{d}]"), true),
        // ... s = T + ':["Thing"\x1c,' + D + "]"
        (format!("{t}:[\"Thing\"\x1c,{d}]"), true),
    ];
    for (text, expected) in rows {
        assert_eq!(is_forum(ld, Some(&text)), expected, "{text:?}");
    }
    // a script of another type, or with no text, is no forum page
    assert!(!is_forum("application/json", Some(&format!("{t}:{d}"))));
    assert!(!is_forum(ld, None));
    // the root is not a candidate: `.//script` reads descendants
    let (mut tree, root) = script_tree(ld, Some(&format!("{t}:{d}")));
    tree.set(root, "type", ld);
    tree.node_mut(root).text = Some(format!("{t}:{d}"));
    let script = tree.first_child(root).unwrap();
    tree.remove(script);
    assert!(!forum_thread_page(&tree, root, &Call::default()).unwrap());
}

#[test]
fn forum_thread_page_on_loaded_pages() {
    let is_forum_page = |attr: &str, text: &str| {
        let page = format!(
            "<html><head><title>T</title><script {attr}>{text}</script></head><body></body></html>"
        );
        let tree = load(&page);
        forum_thread_page(&tree, tree.root(), &Call::default()).unwrap()
    };
    let ld = "type=\"application/ld+json\"";
    let (t, d) = ("\"@type\"", "\"DiscussionForumPosting\"");
    // oracle: core._forum_thread_page(utils.load_html(page)) inside html5_parsers(), with
    //   page = '<html><head><title>T</title><script ' + attr + '>' + s + '</script></head><body></body></html>'
    let rows = [
        // s = '{' + T + ' : ' + D + '}'
        (ld, format!("{{{t} : {d}}}"), true),
        // s = '{' + T + '\xa0:' + D + '}'
        (ld, format!("{{{t}\u{a0}:{d}}}"), true),
        // s = '{' + T + '\u2003:' + D + '}'
        (ld, format!("{{{t}\u{2003}:{d}}}"), true),
        // s = '{' + T + '\u200b:' + D + '}'
        (ld, format!("{{{t}\u{200b}:{d}}}"), false),
        // s = '{' + T + ': ["Thing", ' + D + ']}'
        (ld, format!("{{{t}: [\"Thing\", {d}]}}"), true),
        // s = '{' + T + ':\n["Thing",\n' + D + ']}'
        (ld, format!("{{{t}:\n[\"Thing\",\n{d}]}}"), true),
        // s = '{' + T + ': ["Thing"], "x": ' + D + '}'
        (ld, format!("{{{t}: [\"Thing\"], \"x\": {d}}}"), false),
        // s = '{' + T + ': "QAPage"}'
        (ld, format!("{{{t}: \"QAPage\"}}"), false),
        // s = '{"description": "a DiscussionForumPosting here"}'
        (
            ld,
            "{\"description\": \"a DiscussionForumPosting here\"}".to_owned(),
            false,
        ),
        // attr = 'type="application/json"'
        ("type=\"application/json\"", format!("{{{t}:{d}}}"), false),
        // attr = 'TYPE="application/ld+json"' (html5 lowercases attribute names)
        ("TYPE=\"application/ld+json\"", format!("{{{t}:{d}}}"), true),
        // s = ''
        (ld, String::new(), false),
    ];
    for (attr, text, expected) in rows {
        assert_eq!(is_forum_page(attr, &text), expected, "{attr} {text:?}");
    }
}

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

fn element(tree: &mut Tree, name: &str, text: Option<&str>, tail: Option<&str>) -> NodeId {
    let id = tree.new_element(name);
    tree.node_mut(id).text = text.map(str::to_owned);
    tree.node_mut(id).tail = tail.map(str::to_owned);
    id
}

/// A `body` of `p` children given as (text, tail).
fn body_of(tree: &mut Tree, children: &[(Option<&str>, Option<&str>)]) -> NodeId {
    let body = tree.new_element("body");
    for &(text, tail) in children {
        let child = element(tree, "p", text, tail);
        tree.append(body, child);
    }
    body
}

/// `salvage` of `posts` into `body`: the new text and the body as XML.
fn salvage_case(
    body: &[(Option<&str>, Option<&str>)],
    posts: &[(Option<&str>, Option<&str>)],
) -> (Option<String>, String) {
    let mut tree = Tree::new(LocalName::from("html"), Ns::Html, Vec::new());
    let postbody = body_of(&mut tree, body);
    let forum_posts = body_of(&mut tree, posts);
    let text = salvage(&mut tree, postbody, forum_posts, &Call::default()).unwrap();
    (text, tree.tostring(postbody, false, false))
}

#[test]
fn salvage_joins_with_empty_text_pieces() {
    let some = Some;
    // oracle: core.trafilatura_sequence on a forum page with extract_content stubbed to return
    //   fromstring("<body><p>Main body text</p></body>") and extract_comments stubbed to return the
    //   posts (s1, with `posts[1].tail = ""`): the text and etree.tostring(body, with_tail=False)
    let (text, body) = salvage_case(
        &[(some("Main body text"), None)],
        &[
            (some("One comment here"), None),
            (some("Second line"), some("")),
            (some("third"), None),
        ],
    );
    assert_eq!(
        text.as_deref(),
        Some("Main body text One comment here Second line  third")
    );
    assert_eq!(
        body,
        "<body><p>Main body text</p><p>One comment here</p><p>Second line</p><p>third</p></body>"
    );
    // s6: the empty piece after the last post is stripped
    let (text, _) = salvage_case(
        &[(some("Main body text"), None)],
        &[(some("Fresh post"), some(""))],
    );
    assert_eq!(text.as_deref(), Some("Main body text Fresh post"));
    // s5: the body's own tail counts in the text but not in the existing posts
    let (text, body) = salvage_case(
        &[(some("Main"), some("stray text"))],
        &[(some("stray text"), None), (some("Fresh"), None)],
    );
    assert_eq!(text.as_deref(), Some("Main stray text stray text Fresh"));
    assert_eq!(
        body,
        "<body><p>Main</p>stray text<p>stray text</p><p>Fresh</p></body>"
    );
}

#[test]
fn salvage_appends_only_missing_posts() {
    let some = Some;
    // oracle: s2, as in salvage_joins_with_empty_text_pieces: a post inside the body's text and an
    //   empty one stay out, a nested post comes whole
    let mut tree = Tree::new(LocalName::from("html"), Ns::Html, Vec::new());
    let postbody = body_of(&mut tree, &[(some("Main body text"), None)]);
    let forum_posts = body_of(&mut tree, &[(some("Main body"), None), (None, None)]);
    let nested = element(&mut tree, "div", some("A"), None);
    let bold = element(&mut tree, "b", some("bold"), some("tail"));
    tree.append(nested, bold);
    tree.append(forum_posts, nested);
    let fresh = element(&mut tree, "p", some("Fresh post"), None);
    tree.append(forum_posts, fresh);
    let text = salvage(&mut tree, postbody, forum_posts, &Call::default()).unwrap();
    assert_eq!(
        text.as_deref(),
        Some("Main body text A bold tail Fresh post")
    );
    assert_eq!(
        tree.tostring(postbody, false, false),
        "<body><p>Main body text</p><div>A<b>bold</b>tail</div><p>Fresh post</p></body>"
    );
    // s3: every post is inside the body's text: nothing is appended and the text is not recomputed
    let (text, body) = salvage_case(
        &[(some("Main body text"), None)],
        &[(some("Main"), None), (some("body text"), None)],
    );
    assert_eq!(text, None);
    assert_eq!(body, "<body><p>Main body text</p></body>");
    // s4: the body's texts are joined with a newline, so a post that spans two of them is missing
    let (text, body) = salvage_case(
        &[
            (some("Main one"), None),
            (None, None),
            (some("Main two"), None),
        ],
        &[(some("one Main"), None), (some("Main two"), None)],
    );
    assert_eq!(text.as_deref(), Some("Main one Main two one Main"));
    assert_eq!(
        body,
        "<body><p>Main one</p><p/><p>Main two</p><p>one Main</p></body>"
    );
}

#[test]
fn salvage_checks_call_per_post() {
    let token = CancelToken::new();
    token.cancel();
    let cancelled = Call::default().cancel(&token);
    let mut tree = Tree::new(LocalName::from("html"), Ns::Html, Vec::new());
    // an empty postbody in both legs: only the post loop can check
    let postbody = body_of(&mut tree, &[]);
    let one_post = body_of(&mut tree, &[(Some("Fresh post"), None)]);
    assert_eq!(
        salvage(&mut tree, postbody, one_post, &cancelled),
        Err(ExtractError::Cancelled)
    );
    let no_posts = body_of(&mut tree, &[]);
    assert_eq!(salvage(&mut tree, postbody, no_posts, &cancelled), Ok(None));
    // the scan of the body's children checks too
    let one_child = body_of(&mut tree, &[(Some("Main"), None)]);
    assert_eq!(
        salvage(&mut tree, one_child, no_posts, &cancelled),
        Err(ExtractError::Cancelled)
    );
}

/// `n` sentences about `topic`, each ending in a space.
fn prose(topic: &str, n: usize) -> String {
    (0..n)
        .map(|i| {
            format!(
                "{topic} sentence number {i} says something quite different about its subject. "
            )
        })
        .collect()
}

fn full_page(body: &str, head: &str) -> String {
    format!("<html><head><title>T</title>{head}</head><body>{body}</body></html>")
}

struct Run {
    tree: Tree,
    body: NodeId,
    text: String,
    comments_body: NodeId,
    comments: String,
}

fn sequence(html: &str, options: &Extractor) -> Run {
    let call = Call::default();
    let mut tree = load(html);
    let root = tree.root();
    let ctx = Ctx {
        options: Cow::Borrowed(options),
        call: &call,
    };
    let (body, text, comments_body, comments) =
        trafilatura_sequence(&mut tree, root, &ctx, None, &ParseLimits::default()).unwrap();
    Run {
        tree,
        body,
        text,
        comments_body,
        comments,
    }
}

#[test]
fn sequence_matches_python() {
    let forum = "<script type=\"application/ld+json\">{\"@context\":\"https://schema.org\",\
        \"@type\":\"DiscussionForumPosting\",\"headline\":\"T\"}</script>";
    let thread = "<div id=\"comments\"><p>One comment here</p><br>Second line<p>third</p></div>";
    let thread_text = "One comment here Second line  third";
    let article = format!("<article><p>{}</p></article>", prose("Main", 5));
    let short = format!("<article><p>{}</p></article>", prose("Main", 1));
    let article_ld = format!(
        "<script type=\"application/ld+json\">{{\"@type\": \"Article\", \"articleBody\": \"{}\"}}</script>",
        prose("Json", 6)
    );
    let teaser = |n: usize| format!("<div class=\"teaser\"><p>{}</p></div>", prose("Teaser", n));
    let esc_accept = format!(
        "<article><p>{}</p>{}</article>",
        prose("Main", 4),
        teaser(40)
    );
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
    // oracle: core.trafilatura_sequence(utils.load_html(html), Extractor(output_format="txt",
    //   fast=True, precision=p, recall=r, comments=c)) inside html5_parsers(): `temp_text` and
    //   `temp_comments` of each page, built from prose() and page() as here
    // (label, html, precision, comments, text, comments text)
    let rows: Vec<(&str, String, bool, bool, String, &str)> = vec![
        // forum, posts missing from the body: the salvage appends them
        (
            "f1",
            full_page(&with(thread, &article), forum),
            false,
            true,
            format!("{main5} {thread_text}"),
            "",
        ),
        (
            "f2",
            full_page(&with(thread, &article), forum),
            true,
            true,
            format!("{main5} {thread_text}"),
            "",
        ),
        (
            "f3",
            full_page(&with(thread, &article), ""),
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
            main5.clone(),
            "",
        ),
        (
            "f5",
            full_page(&with(thread, &article), ""),
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
            true,
            format!("{main1}\nOne comment here\nSecond line\nthird"),
            "",
        ),
        (
            "f7",
            full_page(&short, &article_ld),
            false,
            true,
            json_text,
            "",
        ),
        (
            "f8",
            full_page("<article><p>Short text only.</p></article>", ""),
            false,
            true,
            "Short text only.".to_owned(),
            "",
        ),
        (
            "f9",
            full_page(&with(thread, &article), ""),
            true,
            true,
            main5,
            thread_text,
        ),
        // escalation accepted and refused by the ratio, with a forum and with comments
        (
            "e1",
            full_page(&esc_accept, ""),
            false,
            true,
            esc_text.clone(),
            "",
        ),
        (
            "e2",
            full_page(&esc_ratio, ""),
            false,
            true,
            main4.clone(),
            "",
        ),
        (
            "e3",
            full_page(&with(thread, &esc_accept), forum),
            false,
            true,
            esc_text.clone(),
            "",
        ),
        (
            "e4",
            full_page(&with(thread, &esc_ratio), forum),
            false,
            true,
            format!("{main4} {thread_text}"),
            "",
        ),
        (
            "e5",
            full_page(&with(thread, &esc_accept), ""),
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
            esc_text,
            "",
        ),
        (
            "e7",
            full_page(&with(thread, &esc_ratio), ""),
            false,
            true,
            main4,
            thread_text,
        ),
    ];
    for (label, html, precision, comments, text, comments_text) in rows {
        let run = sequence(&html, &cascade_options(precision, false, comments));
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

// ---------------------------------------------------------------------------- bare_extraction

const COMMENT_DIV: &str =
    "<div id=\"comments\"><p>One comment here</p><p>Second reply here</p></div>";
const COMMENT_BODY: &str = "<body><p>One comment here</p><p>Second reply here</p></body>";

fn main_page(extra: &str) -> String {
    full_page(
        &format!("<article><p>{}</p></article>{extra}", prose("Main", 5)),
        "",
    )
}

fn main_text() -> String {
    prose("Main", 5).trim().to_owned()
}

fn bare_options(format: Format, comments: bool) -> Extractor {
    let mut options = options(format, false);
    options.comments = comments;
    options
}

fn bare(source: Source<'_>, options: &Extractor) -> Result<Document, ExtractError> {
    let call = Call::default();
    let ctx = Ctx {
        options: Cow::Borrowed(options),
        call: &call,
    };
    bare_extraction(source, &ctx, &ParseLimits::default())
}

fn bare_page(html: &str, options: &Extractor) -> Result<Document, ExtractError> {
    bare(Source::Input(HtmlInput::Str(html)), options)
}

fn internal_page(
    html: &str,
    record_id: Option<&str>,
    options: &Extractor,
) -> Result<Document, ExtractError> {
    let call = Call::default();
    let ctx = Ctx {
        options: Cow::Borrowed(options),
        call: &call,
    };
    internal_extraction(
        Source::Input(HtmlInput::Str(html)),
        record_id,
        &ctx,
        &ParseLimits::default(),
    )
}

fn rejected(rejection: Rejection) -> ExtractError {
    ExtractError::Rejected(rejection)
}

fn body_xml(doc: &Document) -> String {
    doc.tree.tostring(doc.body, false, false)
}

fn comments_xml(doc: &Document) -> String {
    doc.tree.tostring(doc.comments_body, false, false)
}

#[test]
fn python_format_document() {
    let main = main_text();
    // oracle: d = core.bare_extraction(page, options=Extractor(output_format="python", fast=True,
    //   dedup=False, comments=c)) inside html5_parsers(), page = an article of prose("Main", 5) with
    //   or without '<div id="comments"><p>One comment here</p><p>Second reply here</p></div>';
    //   d.text, d.comments, d.raw_text, etree.tostring(d.body), etree.tostring(d.commentsbody)
    // (page, comments on, comments, comments body)
    let rows = [
        (
            main_page(COMMENT_DIV),
            true,
            Some("One comment here\nSecond reply here"),
            COMMENT_BODY,
        ),
        // comments off: `comments` stays None and the comments body is the empty default
        (main_page(COMMENT_DIV), false, None, "<body/>"),
        // no comment section: an empty string, not None
        (main_page(""), true, Some(""), "<body/>"),
    ];
    for (page, comments, comments_text, comments_body) in rows {
        let doc = bare_page(&page, &bare_options(Format::Python, comments)).unwrap();
        let label = format!("comments {comments}, {comments_text:?}");
        assert_eq!(doc.text.as_deref(), Some(main.as_str()), "{label}");
        assert_eq!(doc.raw_text, doc.text, "{label}");
        assert_eq!(doc.comments.as_deref(), comments_text, "{label}");
        assert_eq!(
            body_xml(&doc),
            format!("<body><p>{main}</p></body>"),
            "{label}"
        );
        assert_eq!(comments_xml(&doc), comments_body, "{label}");
    }
}

#[test]
fn txt_format_document() {
    let main = main_text();
    // oracle: d = core.bare_extraction(page, options=Extractor(output_format="txt", fast=True,
    //   dedup=False, comments=c)) inside html5_parsers(), pages as in python_format_document;
    //   d.text, d.comments, d.raw_text, etree.tostring(d.body), etree.tostring(d.commentsbody)
    for (page, comments, comments_body) in [
        (main_page(COMMENT_DIV), true, COMMENT_BODY),
        (main_page(COMMENT_DIV), false, "<body/>"),
        (main_page(""), true, "<body/>"),
    ] {
        let doc = bare_page(&page, &bare_options(Format::Txt, comments)).unwrap();
        assert_eq!(doc.text, None);
        assert_eq!(doc.comments, None);
        assert_eq!(doc.raw_text.as_deref(), Some(main.as_str()));
        assert_eq!(body_xml(&doc), format!("<body><p>{main}</p></body>"));
        assert_eq!(comments_xml(&doc), comments_body);
    }
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
        rejected(Rejection::TooShort)
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
                (None, Err(error)) => {
                    assert_eq!(error, rejected(Rejection::TreeTooLarge), "{label}")
                }
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
    // (limit, children as built, `Ok` or the error, the body afterwards)
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
            Err(rejected(Rejection::TreeTooLarge)),
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
            None => assert_eq!(
                result.unwrap_err(),
                rejected(Rejection::TooShort),
                "{label}"
            ),
        }
    }
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

#[test]
fn id_and_fingerprint_only_outside_txt() {
    let main = main_text();
    let page = main_page(COMMENT_DIV);
    // oracle: d = core._internal_extraction(page, record_id="7", options=Extractor(output_format=f,
    //   fast=True, dedup=False)) inside html5_parsers(); d.text, d.id, d.fingerprint is None
    // (format, text, id, fingerprint set)
    let rows = [
        (
            Format::Txt,
            format!("{main}\nOne comment here\nSecond reply here"),
            None,
            false,
        ),
        (
            Format::Markdown,
            format!("{main}\n\nOne comment here\n\nSecond reply here"),
            None,
            false,
        ),
        (
            Format::Html,
            format!("<html>\n  <body>\n    <p>{main}</p>\n  </body>\n</html>"),
            Some("7"),
            true,
        ),
    ];
    for (format, text, id, fingerprint) in rows {
        let doc = internal_page(&page, Some("7"), &bare_options(format, true)).unwrap();
        assert_eq!(doc.text.as_deref(), Some(text.as_str()), "{format:?}");
        assert_eq!(doc.id.as_deref(), id, "{format:?}");
        assert_eq!(doc.fingerprint.is_some(), fingerprint, "{format:?}");
        assert_eq!(doc.raw_text.as_deref(), Some(main.as_str()), "{format:?}");
    }
    // oracle: core._internal_extraction(page, options=Extractor(output_format="html", fast=True,
    //   dedup=False)): d.id is None and d.fingerprint is not None
    let doc = internal_page(&page, None, &bare_options(Format::Html, true)).unwrap();
    assert_eq!(doc.id, None);
    assert!(doc.fingerprint.is_some());
}

#[test]
fn caller_tree_untouched() {
    let page = main_page(COMMENT_DIV);
    let tree = load(&page);
    let root = tree.root();
    let before = tree.tostring(root, false, true);
    let options = bare_options(Format::Python, true);
    let from_tree = bare(Source::Tree(&tree, root), &options).unwrap();
    // oracle: before = etree.tostring(tree); core.bare_extraction(tree, options=o) inside
    //   html5_parsers(); etree.tostring(tree) == before is True
    assert_eq!(tree.tostring(root, false, true), before);
    // oracle: the Document of a tree source equals that of the string source: text, comments
    let from_input = bare_page(&page, &options).unwrap();
    assert_eq!(from_tree.text, from_input.text);
    assert_eq!(
        from_tree.comments.as_deref(),
        Some("One comment here\nSecond reply here")
    );
    assert_eq!(body_xml(&from_tree), body_xml(&from_input));
}

#[test]
fn deep_caller_tree_on_small_stack() {
    // oracle: tree = html.fromstring("<html><body><p>root text</p></body></html>"); node = the
    //   body; 3000 times node = SubElement(node, "div"); node.text = DEEP; core.bare_extraction(tree,
    //   options=Extractor(output_format=f, fast=True, dedup=False) with min_output_size and
    //   min_extracted_size 0): d.text, etree.tostring(d.body) (depth 2: the body and its paragraphs)
    const DEEP: &str =
        "Deep text long enough to be kept by the extractor, appearing at the very bottom.";
    let outcome = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            let mut tree = Tree::new(LocalName::from("html"), Ns::Html, Vec::new());
            let root = tree.root();
            let body = tree.new_element("body");
            tree.append(root, body);
            let p = element(&mut tree, "p", Some("root text"), None);
            tree.append(body, p);
            let mut node = body;
            for _ in 0..3000 {
                let div = tree.new_element("div");
                tree.append(node, div);
                node = div;
            }
            tree.node_mut(node).text = Some(DEEP.to_owned());
            let mut options = bare_options(Format::Python, true);
            options.min_output_size = 0;
            options.min_extracted_size = 0;
            let doc = bare(Source::Tree(&tree, root), &options).unwrap();
            let mut txt = bare_options(Format::Txt, true);
            txt.min_output_size = 0;
            txt.min_extracted_size = 0;
            let call = Call::default();
            let ctx = Ctx {
                options: Cow::Borrowed(&txt),
                call: &call,
            };
            let rendered = internal_extraction(
                Source::Tree(&tree, root),
                None,
                &ctx,
                &ParseLimits::default(),
            )
            .unwrap();
            (body_xml(&doc), doc.text, rendered.text)
        })
        .unwrap()
        .join()
        .unwrap();
    let expected_text = format!("root text\n{DEEP}");
    assert_eq!(
        outcome.0,
        format!("<body><p>root text</p><p>{DEEP}</p></body>")
    );
    assert_eq!(outcome.1.as_deref(), Some(expected_text.as_str()));
    // oracle: core.extract(tree, options=Extractor(output_format="txt", ...)) == d.text
    assert_eq!(outcome.2.as_deref(), Some(expected_text.as_str()));
}
