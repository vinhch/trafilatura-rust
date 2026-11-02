use super::*;

#[test]
fn recover_scan_cap_counts_code_points() {
    // oracle: [len(trim("".join(el.itertext()))) for el in body] is [149999, 99] after
    // recover_wild_text(html.fromstring(page), etree.Element("body"), Extractor(recall=True)) with
    // page = <html><body><div>FILLER</div><div>CONTAINER</div><div>DUP</div></body></html>, the
    // filler 3000 times 49 "é" and a space: 150000 code points, 297000 bytes (over
    // DEDUPE_SCAN_CAP), so the duplicate is dropped only when the cap counts code points
    let container = "The quick brown fox jumps over the lazy dog while the sun was setting slowly over the meadow today.";
    let duplicate = "quick brown fox jumps over the lazy dog while the sun was setting slowly";
    let filler = format!("{} ", "\u{e9}".repeat(49)).repeat(3000);
    assert!(filler.len() > DEDUPE_SCAN_CAP && filler.chars().count() < DEDUPE_SCAN_CAP);
    let page = format!(
        "<html><body><div>{filler}</div><div>{container}</div><div>{duplicate}</div></body></html>"
    );
    let mut tree = parse_document(&page, &ParseLimits::default(), &Call::default()).unwrap();
    let root = tree.root();
    let body = tree.new_element("body");
    let options = options_with_recall(true);
    let call = Call::default();
    recover_wild_text(&mut tree, root, body, &ctx_for(&options, &call), None, None).unwrap();
    let lengths: Vec<usize> = tree
        .children(body)
        .map(|el| elem_text(&tree, el).chars().count())
        .collect();
    assert_eq!(lengths, [149_999, 99]);
}

#[test]
fn prune_checks_cancellation() {
    let mut tree = xml("<div><p>a</p></div>");
    let root = tree.root();
    let options = options();
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    assert_eq!(
        prune_unwanted_sections(
            &mut tree,
            root,
            TAG_CATALOG,
            &ctx_for(&options, &call),
            false
        ),
        Err(ExtractError::Cancelled)
    );
}

#[test]
fn prune_precision_drops_trailing_titles_with_their_tails() {
    // oracle: for each source, prune_unwanted_sections(html.fromstring(source) with h1 renamed head,
    // {"p", "head"}, Extractor(precision=True)) then etree.tostring
    let precision = Extractor::new(
        Config::default(),
        Format::Python,
        true,
        false,
        None,
        false,
        false,
        Default::default(),
        Default::default(),
    );
    let call = Call::default();
    let pruned = |source: &str| {
        let mut tree = xml(source);
        let root = tree.root();
        prune_unwanted_sections(
            &mut tree,
            root,
            &["p", "head"],
            &ctx_for(&precision, &call),
            false,
        )
        .unwrap();
        show(&tree, root)
    };
    assert_eq!(
        pruned("<div><p>text</p><head>t1</head><head>t2</head></div>"),
        "<div><p>text</p></div>"
    );
    assert_eq!(
        pruned("<div><p>text</p><head>t1</head>tail<head>t2</head></div>"),
        "<div><p>text</p></div>"
    );
    assert_eq!(
        pruned("<div><head>t0</head><p>text</p><head>t1</head><head>t2</head></div>"),
        "<div><head>t0</head><p>text</p></div>"
    );
}

#[test]
fn recover_recall_selects_lb_list_code_and_div() {
    // oracle: recover_wild_text(html.fromstring(source), etree.Element("body"), Extractor(recall=True))
    // for source = <div><list><item>entry one text</item></list><lb/>loose tail text<code>x = 1</code>
    // <div>block text</div></div> gives <body><list><item>entry one text</item></list>
    // <p>loose tail text</p><code>x = 1</code><p>block text</p></body>
    let mut tree = xml(concat!(
        "<div><list><item>entry one text</item></list><lb/>loose tail text<code>x = 1</code>",
        "<div>block text</div></div>"
    ));
    let root = tree.root();
    let body = tree.new_element("body");
    let options = options_with_recall(true);
    let call = Call::default();
    recover_wild_text(&mut tree, root, body, &ctx_for(&options, &call), None, None).unwrap();
    assert_eq!(
        show(&tree, body),
        concat!(
            "<body><list><item>entry one text</item></list><p>loose tail text</p>",
            "<code>x = 1</code><p>block text</p></body>"
        )
    );
}
