use super::*;

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
