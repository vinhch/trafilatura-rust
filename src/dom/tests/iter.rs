use super::*;

fn tags(tree: &Tree, ids: impl IntoIterator<Item = NodeId>) -> Vec<String> {
    ids.into_iter()
        .map(|id| tree.node(id).name.to_string())
        .collect()
}

/// Runs a live iteration that removes every yielded node; returns the yielded ids.
fn live_remove_all(tree: &mut Tree, start: NodeId, tags: &'static [&'static str]) -> Vec<NodeId> {
    let mut live = LiveIter::new(tree, start, tags);
    let mut seen = Vec::new();
    while let Some(id) = live.next(tree) {
        seen.push(id);
        tree.remove(id);
    }
    seen
}

#[test]
fn iter_live_skips_after_deleted_nested() {
    // oracle: b = etree.fromstring("<body><aside><aside/></aside><aside/></body>"); seen = []
    //   for e in b.iter("aside"): seen.append(e.tag); e.getparent().remove(e)
    //   -> len(seen) == 2, etree.tostring(b) == b"<body><aside/></body>"
    let mut tree = fragment("<aside><aside></aside></aside><aside></aside>");
    let root = tree.root();
    let last = tree.last_child(root).unwrap();
    let seen = live_remove_all(&mut tree, root, &["aside"]);
    assert_eq!(seen.len(), 2);
    assert_eq!(tree.children(root).collect::<Vec<_>>(), [last]);
    assert_eq!(xml(&tree, root), "<div><aside/></div>");
}

#[test]
fn iter_live_accepts_a_runtime_tag_list() {
    let tree = fragment("<q></q><i></i><q></q>");
    let root = tree.root();
    let wanted: Vec<&str> = vec!["q"];
    let mut live = LiveIter::new(&tree, root, &wanted);
    let mut seen = 0;
    while live.next(&tree).is_some() {
        seen += 1;
    }
    assert_eq!(seen, 2);
}

#[test]
fn iter_live_removing_current_still_visits_later_siblings() {
    // oracle: b = etree.fromstring("<body><a/><a/><a/></body>"); n = 0
    //   for e in b.iter("a"): n += 1; b.remove(e)   -> n == 3, tostring(b) == b"<body/>"
    let mut tree = fragment("<a></a><a></a><a></a>");
    let root = tree.root();
    assert_eq!(live_remove_all(&mut tree, root, &["a"]).len(), 3);
    assert_eq!(tree.child_count(root), 0);

    // oracle: b = etree.fromstring("<body><div><aside><x><aside/></x></aside><aside/></div><aside/></body>")
    //   for e in b.iter("aside"): seen.append(e.tag); e.getparent().remove(e)
    //   -> len(seen) == 2, tostring(b) == b"<body><div><aside/></div><aside/></body>"
    let mut tree =
        fragment("<div><aside><x><aside></aside></x></aside><aside></aside></div><aside></aside>");
    let root = tree.root();
    assert_eq!(live_remove_all(&mut tree, root, &["aside"]).len(), 2);
    assert_eq!(xml(&tree, root), "<div><div><aside/></div><aside/></div>");

    // oracle: b = etree.fromstring("<body><div><q><q/></q></div><q/></body>")
    //   for e in b.iter("q"): seen.append(e.tag)
    //     if e.getparent().tag == "div": e.getparent().remove(e)
    //   -> len(seen) == 2, tostring(b) == b"<body><div/><q/></body>"
    let mut tree = fragment("<div><q><q></q></q></div><q></q>");
    let root = tree.root();
    let div = tree.first_child(root).unwrap();
    let mut live = LiveIter::new(&tree, root, &["q"]);
    let mut seen = 0;
    while let Some(id) = live.next(&tree) {
        seen += 1;
        if tree.node(id).parent() == Some(div) {
            tree.remove(id);
        }
    }
    assert_eq!(seen, 2);
    assert_eq!(xml(&tree, root), "<div><div/><q/></div>");
}

#[test]
fn iter_live_revisits_a_node_moved_ahead() {
    // oracle: b = etree.fromstring("<body><a/><b/><c/></body>"); seen = []
    //   for e in b.iter():
    //     seen.append(e.tag)
    //     if e.tag == "a": b.append(e)
    //   -> seen == ["body", "a", "b", "c", "a"], tostring(b) == b"<body><b/><c/><a/></body>"
    let mut tree = fragment("<a></a><b></b><c></c>");
    let root = tree.root();
    let mut live = LiveIter::new(&tree, root, &[]);
    let mut seen = Vec::new();
    while let Some(id) = live.next(&tree) {
        seen.push(id);
        if &*tree.node(id).name == "a" {
            tree.append(root, id);
        }
    }
    assert_eq!(tags(&tree, seen), ["div", "a", "b", "c", "a"]);
    assert_eq!(xml(&tree, root), "<div><b/><c/><a/></div>");
}

#[test]
fn iter_includes_self_and_filters_tags() {
    // oracle: b = etree.fromstring("<body><q><i/><q/></q><i/></body>")
    //   [e.tag for e in b.iter("i")] == ["i", "i"]; [e.tag for e in b.iter("q", "i")] == ["q", "i", "q", "i"]
    let tree = fragment("<q><i></i><q></q></q><i></i>");
    let root = tree.root();
    assert_eq!(tags(&tree, tree.iter(root, &["i"])), ["i", "i"]);
    assert_eq!(
        tags(&tree, tree.iter(root, &["q", "i"])),
        ["q", "i", "q", "i"]
    );
    assert_eq!(
        tags(&tree, tree.iter(root, &[])),
        ["div", "q", "i", "q", "i"]
    );
    // The start matches the filter: it is yielded first.
    assert_eq!(tree.iter(root, &["div"]).collect::<Vec<_>>(), [root]);
    assert_eq!(tree.iter(root, &["nope"]).count(), 0);
}

#[test]
fn iter_on_a_detached_subtree_stays_inside_it() {
    // oracle: b = etree.fromstring("<body><div><p/></div></body>"); d = b[0]; b.remove(d)
    //   [e.tag for e in d.iter()] == ["div", "p"]
    let mut tree = fragment("<div><p></p></div>");
    let div = tree.first_child(tree.root()).unwrap();
    tree.remove(div);
    assert_eq!(tags(&tree, tree.iter(div, &[])), ["div", "p"]);
}

#[test]
fn iterdescendants_excludes_self() {
    // oracle: b = etree.fromstring("<body><div><p/><p/></div><p/></body>"); d = b[0]
    //   [e.tag for e in d.iter()] == ["div", "p", "p"]; [e.tag for e in d.iterdescendants()] == ["p", "p"]
    let tree = fragment("<div><p></p><p></p></div><p></p>");
    let div = tree.first_child(tree.root()).unwrap();
    assert_eq!(tags(&tree, tree.iter(div, &[])), ["div", "p", "p"]);
    assert_eq!(tags(&tree, tree.iterdescendants(div, &[])), ["p", "p"]);
    assert_eq!(tree.iterdescendants(div, &["div"]).count(), 0);
    assert_eq!(tree.iterdescendants(div, &["p"]).count(), 2);
    assert_eq!(tree.find(div, "p"), tree.child_at(div, 0));
    assert_eq!(tree.find(div, "div"), None);
}

#[test]
fn iterancestors_order_is_parent_first() {
    // oracle: b = etree.fromstring("<body><div><p/><p/></div><p/></body>"); d = b[0]
    //   [e.tag for e in d[1].iterancestors()] == ["div", "body"]
    let tree = fragment("<div><p></p><p></p></div><p></p>");
    let root = tree.root();
    let div = tree.first_child(root).unwrap();
    let p = tree.child_at(div, 1).unwrap();
    assert_eq!(tree.iterancestors(p, &[]).collect::<Vec<_>>(), [div, root]);
    assert_eq!(tree.iterancestors(p, &["div"]).count(), 2);
    assert_eq!(tree.iterancestors(p, &["p"]).count(), 0);
    assert_eq!(tree.iterancestors(root, &[]).count(), 0);
}

#[test]
fn itertext_excludes_own_tail() {
    // oracle: w = etree.fromstring("<w><p>a<b>b</b>c</p>tail</w>"); p = w[0]
    //   list(p.itertext()) == ["a", "b", "c"]; p.tail == "tail"
    let tree = fragment("<p>a<b>b</b>c</p>tail");
    let p = tree.first_child(tree.root()).unwrap();
    assert_eq!(tree.itertext(p).collect::<Vec<_>>(), ["a", "b", "c"]);
    assert_eq!(tree.text_content(p), "abc");
    assert_eq!(tree.node(p).tail.as_deref(), Some("tail"));
    // Every descendant's tail counts.
    assert_eq!(
        tree.itertext(tree.root()).collect::<Vec<_>>(),
        ["a", "b", "c", "tail"]
    );
}

#[test]
fn deep_chain_walks_iteratively() {
    // Recursion that follows the depth would overflow 64 KiB long before 256 levels.
    std::thread::Builder::new()
        .stack_size(64 * 1024)
        .spawn(|| {
            let mut tree = Tree::new(LocalName::from("div"), Ns::Html, vec![]);
            let mut parent = tree.root();
            tree.node_mut(parent).text = Some("x".into());
            for _ in 1..256 {
                let child = tree.new_element("div");
                tree.node_mut(child).text = Some("x".into());
                tree.append(parent, child);
                parent = child;
            }
            assert_eq!(tree.len(), 256);

            let copy = tree.copy(tree.root(), &Call::default()).unwrap();
            assert_eq!(tree.len(), 512);
            assert_eq!(tree.iter(copy, &[]).count(), 256);
            assert_eq!(tree.iterdescendants(copy, &["div"]).count(), 255);
            assert_eq!(tree.iterancestors(parent, &[]).count(), 255);
            assert_eq!(tree.itertext(copy).count(), 256);
            assert_eq!(tree.text_content(tree.root()).len(), 256);

            let root = tree.root();
            tree.strip_tags(root, &["div"]);
            assert_eq!(tree.child_count(root), 0);
            assert_eq!(tree.node(root).text.as_deref().map(str::len), Some(256));
            tree.strip_elements(copy, &["div"], true);
            assert_eq!(tree.child_count(copy), 0);
        })
        .unwrap()
        .join()
        .unwrap();
}
