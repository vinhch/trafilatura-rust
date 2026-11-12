use super::*;

#[test]
fn strip_tags_splices_text_children_and_tail() {
    // oracle: d = etree.fromstring("<div>x<span>a<b>b</b>c</span>y</div>"); etree.strip_tags(d, "span")
    //   -> d.text == "xa", [(e.tag, e.text, e.tail) for e in d] == [("b", "b", "cy")]
    let mut tree = fragment("x<span>a<b>b</b>c</span>y");
    let root = tree.root();
    tree.strip_tags(root, &["span"]);
    let b = tree.first_child(root).unwrap();
    assert_eq!(tree.node(root).text.as_deref(), Some("xa"));
    assert_eq!(tree.child_count(root), 1);
    assert_eq!(&*tree.node(b).name, "b");
    assert_eq!(tree.node(b).text.as_deref(), Some("b"));
    assert_eq!(tree.node(b).tail.as_deref(), Some("cy"));
    assert_eq!(tree.node(b).parent(), Some(root));

    // oracle: d = etree.fromstring("<div>x<a/>y<span>s<b/>t</span>z<c/>w</div>"); strip_tags(d, "span")
    //   -> b"<div>x<a/>ys<b/>tz<c/>w</div>"
    let mut tree = fragment("x<a></a>y<span>s<b></b>t</span>z<c></c>w");
    let root = tree.root();
    tree.strip_tags(root, &["span"]);
    assert_eq!(xml(&tree, root), "<div>x<a/>ys<b/>tz<c/>w</div>");
}

#[test]
fn strip_tags_text_goes_to_previous_tail_or_parent_text() {
    // oracle: d = etree.fromstring("<div><span>s</span>z<a/></div>"); strip_tags(d, "span")
    //   -> b"<div>sz<a/></div>"
    let mut tree = fragment("<span>s</span>z<a></a>");
    let root = tree.root();
    tree.strip_tags(root, &["span"]);
    assert_eq!(xml(&tree, root), "<div>sz<a/></div>");

    // oracle: d = etree.fromstring("<div>p<span><b/>q</span>z</div>"); strip_tags(d, "span")
    //   -> b"<div>p<b/>qz</div>"
    let mut tree = fragment("p<span><b></b>q</span>z");
    let root = tree.root();
    tree.strip_tags(root, &["span"]);
    assert_eq!(xml(&tree, root), "<div>p<b/>qz</div>");

    // oracle: d = etree.fromstring("<div>p<span>s</span>z</div>"); strip_tags(d, "span")
    //   -> b"<div>psz</div>"
    let mut tree = fragment("p<span>s</span>z");
    let root = tree.root();
    tree.strip_tags(root, &["span"]);
    assert_eq!(xml(&tree, root), "<div>psz</div>");

    // oracle: d = etree.fromstring("<div><p>a</p><span><b>x</b></span></div>"); strip_tags(d, "span", "b")
    //   -> b"<div><p>a</p>x</div>"
    let mut tree = fragment("<p>a</p><span><b>x</b></span>");
    let root = tree.root();
    tree.strip_tags(root, &["span", "b"]);
    assert_eq!(xml(&tree, root), "<div><p>a</p>x</div>");
}

#[test]
fn strip_tags_nested_same_tag() {
    // oracle: d = etree.fromstring("<div><i>a<i>b</i>c</i>d</div>"); etree.strip_tags(d, "i")
    //   -> d.text == "abcd", len(d) == 0
    let mut tree = fragment("<i>a<i>b</i>c</i>d");
    let root = tree.root();
    tree.strip_tags(root, &["i"]);
    assert_eq!(tree.node(root).text.as_deref(), Some("abcd"));
    assert_eq!(tree.child_count(root), 0);
}

#[test]
fn strip_tags_keeps_the_node_itself_and_does_nothing_without_tags() {
    // oracle: d = etree.fromstring("<w><div>x<div>y</div>t</div>u</w>")[0]; strip_tags(d, "div")
    //   -> b"<div>xyt</div>", d.tail == "u"
    let mut tree = fragment("<div>x<div>y</div>t</div>u");
    let div = tree.first_child(tree.root()).unwrap();
    tree.strip_tags(div, &["div"]);
    assert_eq!(xml(&tree, div), "<div>xyt</div>");
    assert_eq!(tree.node(div).tail.as_deref(), Some("u"));
    assert_eq!(tree.node(div).parent(), Some(tree.root()));

    // oracle: d = etree.fromstring("<div>x<a>1</a>y<s/>z</div>"); strip_tags(d); strip_elements(d)
    //   -> both leave b"<div>x<a>1</a>y<s/>z</div>"
    let mut tree = fragment("x<a>1</a>y<s></s>z");
    let root = tree.root();
    tree.strip_tags(root, &[]);
    tree.strip_elements(root, &[], true);
    assert_eq!(xml(&tree, root), "<div>x<a>1</a>y<s/>z</div>");
}

#[test]
fn strip_elements_takes_tail() {
    // oracle: d = etree.fromstring("<div>a<s>1</s>t1<p>b<s>2</s>t2</p><s/>t3</div>")
    //   strip_elements(d, "s", with_tail=True)  -> b"<div>a<p>b</p></div>"
    //   strip_elements(d, "s", with_tail=False) -> b"<div>at1<p>bt2</p>t3</div>"
    let html = "a<s>1</s>t1<p>b<s>2</s>t2</p><s></s>t3";
    let mut tree = fragment(html);
    let root = tree.root();
    tree.strip_elements(root, &["s"], true);
    assert_eq!(xml(&tree, root), "<div>a<p>b</p></div>");
    let mut tree = fragment(html);
    let root = tree.root();
    tree.strip_elements(root, &["s"], false);
    assert_eq!(xml(&tree, root), "<div>at1<p>bt2</p>t3</div>");

    // oracle: d = etree.fromstring("<div><s>1</s>t1<p/></div>")
    //   with_tail=True -> b"<div><p/></div>"; with_tail=False -> b"<div>t1<p/></div>"
    let html = "<s>1</s>t1<p></p>";
    let mut tree = fragment(html);
    let root = tree.root();
    tree.strip_elements(root, &["s"], true);
    assert_eq!(xml(&tree, root), "<div><p/></div>");
    let mut tree = fragment(html);
    let root = tree.root();
    tree.strip_elements(root, &["s"], false);
    assert_eq!(xml(&tree, root), "<div>t1<p/></div>");
}

#[test]
fn strip_elements_drops_nested_matches_with_their_parent() {
    // oracle: d = etree.fromstring("<div><s>1<s>x</s></s>t1<p/></div>"); strip_elements(d, "s")
    //   -> b"<div><p/></div>"
    let mut tree = fragment("<s>1<s>x</s></s>t1<p></p>");
    let root = tree.root();
    tree.strip_elements(root, &["s"], true);
    assert_eq!(xml(&tree, root), "<div><p/></div>");

    // oracle: d = etree.fromstring("<w><s>1</s>t</w>")[0]; strip_elements(d, "s")
    //   -> b"<s>1</s>" unchanged, d.tail == "t"
    let mut tree = fragment("<s>1</s>t");
    let s = tree.first_child(tree.root()).unwrap();
    tree.strip_elements(s, &["s"], true);
    assert_eq!(xml(&tree, s), "<s>1</s>");
    assert_eq!(tree.node(s).tail.as_deref(), Some("t"));
    assert_eq!(tree.node(s).parent(), Some(tree.root()));
}

#[test]
fn strip_tags_where_strips_svg_elements() {
    // oracle: p = etree.fromstring("<p>a<svg><title>t</title></svg>b</p>")
    //   etree.strip_tags(p, "svg", "title") -> p.text == "atb", len(p) == 0
    let mut tree = fragment("<p>a<svg><title>t</title></svg>b</p>");
    let p = tree.first_child(tree.root()).unwrap();
    assert_eq!(tree.iter(p, &[]).count(), 3);
    tree.strip_tags_where(p, |tree, node| tree.tag(node) != Some("p"));
    assert_eq!(xml(&tree, p), "<p>atb</p>");
}
