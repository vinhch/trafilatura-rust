use super::*;

// ---------------------------------------------------------------------------- the cascade

const NEL: char = '\u{85}';

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
