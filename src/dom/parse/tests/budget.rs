use std::collections::HashSet;

use html5ever::Parser;
use html5ever::tendril::TendrilSink;

use super::*;
use crate::cancel::CancelToken;
use crate::dom::{DumpMode, dump};

// `<p>` then `n` formatting elements with a long attribute, then `m` paragraphs: each `<p>`
// makes html5ever clone every open formatting element.
fn amplifying(n: usize, m: usize, value: &str) -> String {
    let mut html = String::from("<html><body><p>");
    for i in 0..n {
        html.push_str(&format!("<b id={i} t={value}>"));
    }
    html.push_str(&"<p>x".repeat(m));
    html
}

fn feed_whole<'a>(html: &str, limits: &ParseLimits, call: &'a Call<'a>) -> Parser<Sink<'a>> {
    let mut parser = html5ever::parse_document(new_sink(limits, call, false), parse_opts());
    parser.process(StrTendril::from_slice(html));
    parser
}

#[test]
fn comments_and_processing_instructions_do_not_grow_the_arena() {
    let page = |comment: &str| {
        format!(
            "<html><body><p>a{}b</p></body></html>",
            comment.repeat(100_000)
        )
    };
    let call = Call::default();
    let count = |html: &str| {
        feed_whole(html, &ParseLimits::default(), &call)
            .tokenizer
            .sink
            .sink
            .node_count()
    };
    let bare = count(&page(""));
    for comment in ["<!---->", "<!>", "<?pi>"] {
        let with_comments = count(&page(comment));
        assert!(
            with_comments <= bare + 1,
            "{comment}: {with_comments} nodes against {bare} without comments"
        );
    }
}

#[test]
fn comments_in_every_insertion_mode_parse_as_html5ever_does() {
    let pages = [
        "@<!DOCTYPE html>@<html>@<head>@<title>t@</title>@</head>@<body>@<p>x@y</p>@</body>@</html>@",
        "@<p>a@b",
        "<table>@<tr>@<td>a@b</td>@</tr>@</table>@",
        "<table>x@y</table>",
        "<table>@x@<tr>@",
        "<body>a</body>@b",
        "</body>@</html>@",
        "<template>@<tr>@<td>x@</td></tr>@</template>",
        "<svg>@<g>@<title>@t</title></g>@<![CDATA[x]]>@</svg>@",
        "<math>@<mi>@x</mi>@<annotation-xml encoding=text/html>@<p>@</annotation-xml></math>",
        "<select>@<option>a@b</option>@</select>@",
        "<frameset>@<frame>@</frameset>@",
        "<noscript>@</noscript>@",
        "<textarea>@</textarea>@",
        "<b><p>@</b>@x",
        "<a>1@<div>2@</a>3@",
        "<p><b>x@</p>@<p>y",
        "<caption>@<colgroup>@<col>@",
        "<ul><li>@<ul><li>@</ul></ul>@",
        "<script>@</script>@<style>@</style>@",
    ];
    let comments = ["<!---->", "<!>", "<?pi x>", "<!--c-->", "<!--c--!>"];
    for page in pages {
        for comment in comments {
            let html = page.replace('@', comment);
            for fragment in [false, true] {
                let tree = parse(&html, fragment).unwrap();
                let whole = whole_tree(&html, fragment);
                assert_eq!(
                    dump(&tree, DumpMode::Exact),
                    dump(&whole, DumpMode::Exact),
                    "{html:?} fragment {fragment}"
                );
            }
        }
    }
}

#[test]
fn cloned_formatting_elements_share_attribute_values() {
    let value = "v".repeat(500);
    let html = amplifying(20, 50, &value);
    let call = Call::default();
    let parser = feed_whole(&html, &ParseLimits::default(), &call);
    let inner = parser.tokenizer.sink.sink.inner.borrow();
    let values: Vec<&Arc<str>> = inner
        .nodes
        .iter()
        .flat_map(|node| node.attrs.iter())
        .filter(|(name, _)| &**name == "t")
        .map(|(_, value)| value)
        .collect();
    let distinct: HashSet<*const u8> = values.iter().map(|v| v.as_ptr()).collect();
    assert!(values.len() > 20 * 50, "{} elements carry t", values.len());
    assert_eq!(distinct.len(), 20);

    let ids: Vec<&Arc<str>> = inner
        .nodes
        .iter()
        .flat_map(|node| node.attrs.iter())
        .filter(|(name, _)| &**name == "id")
        .map(|(_, value)| value)
        .collect();
    let distinct_ids: HashSet<*const u8> = ids.iter().map(|v| v.as_ptr()).collect();
    assert!(ids.len() > 20 * 50);
    assert_eq!(
        distinct_ids.len(),
        20,
        "short values are interned by content"
    );
}

#[test]
fn no_attributes_are_kept_once_the_node_limit_is_hit() {
    let html = amplifying(50, 2000, &"v".repeat(200));
    let call = Call::default();
    let limits = ParseLimits {
        max_nodes: Some(5_000),
        ..ParseLimits::default()
    };
    let parser = feed_whole(&html, &limits, &call);
    let inner = parser.tokenizer.sink.sink.inner.borrow();
    assert!(inner.stopped);
    assert_eq!(
        inner.abort,
        Some(ExtractError::Rejected(Rejection::TreeTooLarge(
            TreeLimit::MaxNodes
        )))
    );
    let with_attrs = inner.nodes.iter().filter(|n| !n.attrs.is_empty()).count();
    assert!(with_attrs <= 5_000, "{with_attrs} nodes kept attributes");
    assert!(inner.elements > 50_000, "the input must outgrow the limit");
}

fn attr_pairs(tree: &Tree, id: crate::dom::NodeId) -> Vec<(String, String)> {
    tree.node(id)
        .attrs
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

fn pair(name: &str, value: &str) -> (String, String) {
    (name.to_string(), value.to_string())
}

#[test]
fn repeated_html_and_body_tags_add_missing_attributes() {
    let html = "<html x=1><body a=1><html x=2 y=3><body a=2 b=3>";
    let tree = parse_document(html, &ParseLimits::default(), &Call::default()).unwrap();
    let root = tree.root();
    let body = tree
        .children(root)
        .find(|&id| &*tree.node(id).name == "body")
        .unwrap();
    assert_eq!(attr_pairs(&tree, root), [pair("x", "1"), pair("y", "3")]);
    assert_eq!(attr_pairs(&tree, body), [pair("a", "1"), pair("b", "3")]);
}

#[test]
fn merged_attributes_are_capped() {
    let tags = |n: usize| (0..n).map(|i| format!("<body a{i}=1>")).collect::<String>();
    let parse = |n: usize| parse_document(&tags(n), &ParseLimits::default(), &Call::default());
    let tree = parse(MAX_TAG_ATTRIBUTES).unwrap();
    let body = tree
        .children(tree.root())
        .find(|&id| &*tree.node(id).name == "body")
        .unwrap();
    assert_eq!(tree.node(body).attrs.len(), MAX_TAG_ATTRIBUTES);
    assert_eq!(
        parse(MAX_TAG_ATTRIBUTES + 1).unwrap_err(),
        ExtractError::Rejected(Rejection::TreeTooLarge(TreeLimit::TagAttributes))
    );
}

// Repeated names are skipped, so the body stays small: parse time stays linear in the input size.
#[test]
#[ignore = "slow"]
fn body_attribute_merge_is_linear() {
    let time = |count: usize| {
        let html: String = (0..count)
            .map(|i| format!("<body a{}=1>", i % 1_000))
            .collect();
        let start = std::time::Instant::now();
        let limits = ParseLimits {
            max_input_bytes: None,
            ..ParseLimits::default()
        };
        parse_document(&html, &limits, &Call::default()).unwrap();
        start.elapsed()
    };
    let base = time(1_000_000);
    let scaled = time(4_000_000);
    assert!(scaled < base * 8, "{base:?} for N, {scaled:?} for 4N");
}

// html5ever holds a name from `elem_name` across later sink calls that mutate the arena.
#[test]
fn elem_name_held_across_create_element() {
    let call = Call::default();
    let sink = new_sink(&ParseLimits::default(), &call, false);
    let first = sink.create_element(
        QualName::new(None, ns!(html), local_name!("div")),
        Vec::new(),
        ElementFlags::default(),
    );
    let name = sink.elem_name(&first);
    let second = sink.create_element(
        QualName::new(None, ns!(html), local_name!("p")),
        Vec::new(),
        ElementFlags::default(),
    );
    sink.append(&0, NodeOrText::AppendNode(first));
    sink.append(&first, NodeOrText::AppendNode(second));
    assert_eq!(*name.local_name(), local_name!("div"));
    assert_eq!(*name.ns(), ns!(html));
}

// `entries` distinct `<b>` of 1,000 attributes (999 shared, one unique so Noah's Ark keeps them
// all), then `<p>` makes html5ever clone every one: `entries` x 1,000 attributes per paragraph.
fn reconstruction_page(entries: usize, paragraphs: usize) -> String {
    let mut html = String::from("<html><body><p>");
    for i in 0..entries {
        html.push_str(&format!("<b{} u{i}>", attrs(MAX_TAG_ATTRIBUTES - 1)));
    }
    html.push_str(&"<p>x".repeat(paragraphs));
    html
}

// Fed whole, html5ever keeps working after the abort, so `paragraphs` is chosen to leave the
// element count below the node limit while the attributes pass the budget.
fn assert_clones_trip_the_attribute_budget(entries: usize, paragraphs: usize, max: usize) {
    let call = Call::default();
    let html = reconstruction_page(entries, paragraphs);
    let parser = feed_whole(&html, &limited(max), &call);
    let inner = parser.tokenizer.sink.sink.inner.borrow();
    assert_eq!(
        inner.abort,
        Some(ExtractError::Rejected(Rejection::TreeTooLarge(
            TreeLimit::MaxNodes
        )))
    );
    let kept: usize = inner.nodes.iter().map(|node| node.attrs.len()).sum();
    assert!(kept <= max * ATTRIBUTES_PER_NODE, "{kept} attributes kept");
    assert!(
        inner.elements < max,
        "the attributes, not the node limit, must trip: {} elements",
        inner.elements
    );
}

#[test]
fn attribute_budget_counts_clones_at_a_small_limit() {
    assert_clones_trip_the_attribute_budget(5, 20, 2_000);
}

// 250 entries x 2,000 paragraphs is the reconstruction attack at the default limit.
#[test]
#[ignore = "slow"]
fn attribute_budget_counts_clones() {
    assert_clones_trip_the_attribute_budget(250, 40, DEFAULT_MAX_NODES);
    assert_eq!(
        parse(&reconstruction_page(250, 2_000), false).unwrap_err(),
        ExtractError::Rejected(Rejection::TreeTooLarge(TreeLimit::MaxNodes))
    );
}

#[test]
fn a_page_within_the_attribute_budget_parses() {
    let page = |count: usize, attributes: usize| {
        format!(
            "<html><body>{}",
            format!("<p{}>", attrs(attributes)).repeat(count)
        )
    };
    let html = page(DEFAULT_MAX_NODES / 2, 10);
    assert!(parse(&html, false).is_ok());
}

fn html_files(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries.flatten().map(|entry| entry.path()) {
        if path.is_dir() {
            html_files(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "html") {
            out.push(path);
        }
    }
}

// The corpus of `docs/parity-harness.md` (the upstream checkout, as `tests/common` finds it): the
// attributes created and the work charged per page, through the gate, stay a hundredth of their
// budgets. Skips without it.
#[test]
#[ignore = "slow"]
fn corpus_work_is_far_below_the_budgets() {
    let root = std::env::var_os("TRAFILATURA_UPSTREAM")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../trafilatura"));
    let mut files = Vec::new();
    for dir in ["eval", "cache", "resources"] {
        html_files(&root.join("tests").join(dir), &mut files);
    }
    files.sort();
    let mut most = (0, std::path::PathBuf::new());
    let mut most_work = (0, std::path::PathBuf::new());
    for file in &files {
        let html = String::from_utf8_lossy(&std::fs::read(file).unwrap()).into_owned();
        let call = Call::default();
        let driver = Driver::new(
            new_sink(&limited(usize::MAX / ATTRIBUTES_PER_NODE), &call, false),
            false,
        );
        driver.process(&html);
        let inner = driver.sink().inner.borrow();
        if inner.attributes > most.0 {
            most = (inner.attributes, file.clone());
        }
        if inner.compare_work > most_work.0 {
            most_work = (inner.compare_work, file.clone());
        }
    }
    println!(
        "{} pages, most attributes {} in {:?}, most compare work {} in {:?}",
        files.len(),
        most.0,
        most.1,
        most_work.0,
        most_work.1
    );
    let budget = DEFAULT_MAX_NODES * ATTRIBUTES_PER_NODE;
    assert!(most.0 * 100 <= budget, "{} of {budget}", most.0);
    let work_budget = DEFAULT_MAX_NODES * COMPARE_WORK_PER_NODE;
    assert!(
        most_work.0 * 100 <= work_budget,
        "{} of {work_budget}",
        most_work.0
    );
}

// Budget of 2,000 nodes is 20,000 attributes: the total passes exactly at the budget.
#[test]
fn attribute_budget_is_exact() {
    let limits = limited(2_000);
    let page = |extra: usize| {
        let mut html = String::from("<html><body>");
        html.push_str(&format!("<p{}>", attrs(10)).repeat(1_000));
        html.push_str(&format!("<p{}>", attrs(MAX_TAG_ATTRIBUTES)).repeat(10));
        html.push_str(&format!("<p{}>", attrs(extra)));
        html
    };
    let parse_with = |html: &str| parse_document(html, &limits, &Call::default());
    assert!(parse_with(&page(0)).is_ok());
    assert_eq!(
        parse_with(&page(1)).unwrap_err(),
        ExtractError::Rejected(Rejection::TreeTooLarge(TreeLimit::MaxNodes))
    );
    let unlimited = ParseLimits {
        max_nodes: None,
        ..limits
    };
    assert!(parse_document(&page(1), &unlimited, &Call::default()).is_ok());
}

// Elements created while input ends (`<html>` at end of input makes `body`; the empty string
// makes all three) must count against the limit: html5ever builds them inside `finish`.
#[test]
fn node_limit_is_exact_for_elements_created_at_end_of_input() {
    for html in ["", "<html>", "<html><body><di"] {
        let call = Call::default();
        let count = parse_document(html, &ParseLimits::default(), &call)
            .unwrap()
            .len();
        assert!(
            parse_document(html, &limited(count), &call).is_ok(),
            "{html:?}: limit == count"
        );
        assert_eq!(
            parse_document(html, &limited(count - 1), &call).unwrap_err(),
            ExtractError::Rejected(Rejection::TreeTooLarge(TreeLimit::MaxNodes)),
            "{html:?}: limit == count - 1"
        );
    }
}

// `drive` checks the token before `finish`, so an abort raised inside `finish` is set directly.
#[test]
fn abort_raised_while_finishing_is_reported() {
    for error in [ExtractError::Cancelled, ExtractError::DeadlineExceeded] {
        let call = Call::default();
        let limits = ParseLimits::default();
        let parser = feed_whole("<html>", &limits, &call);
        parser.tokenizer.sink.sink.inner.borrow_mut().abort = Some(error.clone());
        assert_eq!(parser.finish().unwrap_err(), error);
    }
    let token = CancelToken::new();
    token.cancel();
    let call = Call::default().cancel(&token);
    assert_eq!(
        parse_document("", &ParseLimits::default(), &call).unwrap_err(),
        ExtractError::Cancelled
    );
}
