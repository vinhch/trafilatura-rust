use super::*;

/// `handle_textelem(element, potential_tags, Extractor())` where `element` is the root of
/// `etree.fromstring(source)`, or its first child.
fn textelem(source: &str, on_child: bool, potential_tags: &[&str]) -> Option<String> {
    let mut tree = xml(source);
    let element = if on_child {
        tree.first_child(tree.root()).unwrap()
    } else {
        tree.root()
    };
    let options = options();
    let call = Call::default();
    let processed = handle_textelem(
        &mut tree,
        element,
        potential_tags,
        &ctx_for(&options, &call),
    )
    .unwrap();
    processed.map(|id| show(&tree, id))
}

#[test]
fn textelem_dispatches_on_the_tag() {
    // oracle: etree.tostring(handle_textelem(element, potential_tags, Extractor())), None where
    // there is no element
    let catalog = TAG_CATALOG;
    let with_table: Vec<&str> = catalog.iter().copied().chain(["table"]).collect();
    let cases: [(&str, bool, &[&str], Option<&str>); 15] = [
        (
            "<list><item>a</item></list>",
            false,
            catalog,
            Some("<list><item>a</item></list>"),
        ),
        ("<quote>q</quote>", false, catalog, Some("<quote>q</quote>")),
        ("<head>h</head>", false, catalog, Some("<head>h</head>")),
        (
            "<p>p <hi>x</hi></p>",
            false,
            catalog,
            Some("<p>p <hi>x</hi></p>"),
        ),
        (
            "<div><lb/>tail text</div>",
            true,
            catalog,
            Some("<p>tail text</p>"),
        ),
        ("<div><lb/> </div>", true, catalog, None),
        ("<div><lb/></div>", true, catalog, None),
        (
            "<div><hi>x</hi></div>",
            true,
            catalog,
            Some("<p><hi>x</hi></p>"),
        ),
        ("<item><hi>x</hi></item>", true, catalog, Some("<hi>x</hi>")),
        ("<table><tr><td>a</td></tr></table>", false, catalog, None),
        (
            "<table><tr><td>a</td></tr></table>",
            false,
            &with_table,
            Some("<table><row><cell>a</cell></row></table>"),
        ),
        (
            "<graphic src=\"x.jpg\"/>",
            false,
            &["graphic"],
            Some("<graphic src=\"x.jpg\"/>"),
        ),
        ("<graphic src=\"x.jpg\"/>", false, &[], None),
        ("<div>text</div>", false, &["div"], Some("<p>text</p>")),
        ("<div>text</div>", false, &[], None),
    ];
    for (source, on_child, tags, expected) in cases {
        assert_eq!(
            textelem(source, on_child, tags).as_deref(),
            expected,
            "{source}"
        );
    }
}

/// Fills `outer` with `levels` nested `list > item` pairs, each item holding text.
fn list_chain(tree: &mut Tree, outer: NodeId, levels: usize) {
    let mut list = outer;
    for level in 0..levels {
        let item = tree.new_element("item");
        tree.node_mut(item).text = Some("x".to_string());
        tree.append(list, item);
        if level + 1 < levels {
            let inner = tree.new_element("list");
            tree.append(item, inner);
            list = inner;
        }
    }
}

fn new_tree(tag: &str) -> (Tree, NodeId) {
    let tree = Tree::new(LocalName::from(tag), Ns::Html, Vec::new());
    let root = tree.root();
    (tree, root)
}

#[test]
fn deep_nesting_fits_512k_stack() {
    let worker = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            let options = options();
            let call = Call::default();
            let ctx = ctx_for(&options, &call);
            let tags: Vec<&str> = TAG_CATALOG.iter().copied().chain(["table"]).collect();

            // 127 list levels, the deepest a 256-level tree holds
            let (mut tree, root) = new_tree("list");
            list_chain(&mut tree, root, 127);
            assert!(handle_lists(&mut tree, root, &ctx).unwrap().is_some());

            // 255 inline levels below a quote
            let (mut tree, root) = new_tree("quote");
            let mut parent = root;
            for _ in 0..255 {
                let hi = tree.new_element("hi");
                tree.node_mut(hi).text = Some("x".to_string());
                tree.append(parent, hi);
                parent = hi;
            }
            assert!(handle_quotes(&mut tree, root, &ctx).unwrap().is_some());

            // a list chain entered from a quote
            let (mut tree, root) = new_tree("quote");
            let outer = tree.new_element("list");
            tree.append(root, outer);
            list_chain(&mut tree, outer, 100);
            assert!(
                handle_textelem(&mut tree, root, &tags, &ctx)
                    .unwrap()
                    .is_some()
            );

            // a list chain entered from a table cell; only recall mode keeps a list there
            let recall = options_with_recall(true);
            let ctx = ctx_for(&recall, &call);
            let (mut tree, root) = new_tree("table");
            let row = tree.new_element("tr");
            tree.append(root, row);
            let cell = tree.new_element("td");
            tree.append(row, cell);
            let outer = tree.new_element("list");
            tree.append(cell, outer);
            list_chain(&mut tree, outer, 100);
            assert!(
                handle_textelem(&mut tree, root, &tags, &ctx)
                    .unwrap()
                    .is_some()
            );
        })
        .unwrap();
    worker.join().unwrap();
}
