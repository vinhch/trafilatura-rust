use super::*;

fn parse_dropping(html: &str) -> Tree {
    let limits = ParseLimits {
        drop_script_text: true,
        ..ParseLimits::default()
    };
    parse_document(html, &limits, &Call::default()).unwrap()
}

/// Elements in document order.
fn preorder(tree: &Tree) -> Vec<NodeId> {
    let mut order = Vec::new();
    let mut stack = vec![tree.root()];
    while let Some(id) = stack.pop() {
        order.push(id);
        stack.extend(std::iter::successors(tree.last_child(id), |&child| {
            tree.node(child).prev_sibling()
        }));
    }
    order
}

fn element_names(tree: &Tree) -> Vec<String> {
    preorder(tree)
        .into_iter()
        .map(|id| tree.node(id).name.to_string())
        .collect()
}

fn texts_of(tree: &Tree, name: &str) -> Vec<Option<String>> {
    preorder(tree)
        .into_iter()
        .filter(|&id| &*tree.node(id).name == name)
        .map(|id| tree.node(id).text.clone())
        .collect()
}

#[test]
fn script_and_style_text_dropped_on_request() {
    let html = concat!(
        "<html><head><style>s{}</style><script>var a=1</script>",
        r#"<script type="application/ld+json">{"a":1}</script>"#,
        r#"<script type="application/settings+json">{"b":2}</script>"#,
        "</head><body><p>t</p><script>x</script>tail</body></html>",
    );
    let some = |s: &str| Some(s.to_string());
    let dropped = parse_dropping(html);
    assert_eq!(
        texts_of(&dropped, "script"),
        [None, some(r#"{"a":1}"#), some(r#"{"b":2}"#), None]
    );
    assert_eq!(texts_of(&dropped, "style"), [None]);
    let last_script = *preorder(&dropped)
        .iter()
        .rfind(|&&id| &*dropped.node(id).name == "script")
        .unwrap();
    assert_eq!(dropped.node(last_script).tail, some("tail"));
    assert!(dropped.dropped_script_text);

    let kept = parse(html);
    assert_eq!(dropped.len(), kept.len());
    assert_eq!(element_names(&dropped), element_names(&kept));
    assert_eq!(
        texts_of(&kept, "script"),
        [
            some("var a=1"),
            some(r#"{"a":1}"#),
            some(r#"{"b":2}"#),
            some("x")
        ]
    );
    assert_eq!(texts_of(&kept, "style"), [some("s{}")]);
    assert!(!kept.dropped_script_text);

    assert!(!parse_dropping("<p>x</p>").dropped_script_text);
}

#[test]
fn script_type_match_is_exact() {
    let tree = parse_dropping(concat!(
        r#"<script type="APPLICATION/LD+JSON">a</script>"#,
        r#"<script type=" application/ld+json">b</script>"#,
        r#"<script type="text/javascript">c</script>"#,
    ));
    assert_eq!(texts_of(&tree, "script"), [None, None, None]);
}

#[test]
fn foreign_script_and_style_text_kept() {
    let tree = parse_dropping("<svg><script>a</script><style>b</style></svg>");
    assert_eq!(texts_of(&tree, "script"), [Some("a".to_string())]);
    assert_eq!(texts_of(&tree, "style"), [Some("b".to_string())]);
    assert!(!tree.dropped_script_text);
}

#[test]
fn fragment_parse_keeps_script_text() {
    let limits = ParseLimits {
        drop_script_text: true,
        ..ParseLimits::default()
    };
    let tree = parse_fragment(
        "<script>x</script><style>y</style>",
        &limits,
        &Call::default(),
    )
    .unwrap();
    assert_eq!(texts_of(&tree, "script"), [Some("x".to_string())]);
    assert_eq!(texts_of(&tree, "style"), [Some("y".to_string())]);
    assert!(!tree.dropped_script_text);
}
