//! Limits of the libxml2 builder: depth, node and attribute budgets, and the cancellation check of the token loop.

use super::*;
use crate::cancel::{CHECK_INTERVAL, CheckTrip};
use crate::dom::parse::builder::{build_document, build_with};
use crate::dom::{NodeId, Tree};

fn build(html: &str, limits: &ParseLimits) -> Result<Tree, ExtractError> {
    build_document(html, limits, &Call::default())
}

/// Depth of the deepest element under `id`, where `id` itself is at `depth`.
fn deepest(tree: &Tree, id: NodeId, depth: usize) -> usize {
    tree.children(id)
        .map(|child| deepest(tree, child, depth + 1))
        .max()
        .unwrap_or(depth)
}

fn element_names(tree: &Tree, id: NodeId, out: &mut Vec<String>) {
    out.push(tree.node(id).name.to_string());
    for child in tree.children(id) {
        element_names(tree, child, out);
    }
}

fn attribute_count(tree: &Tree, id: NodeId) -> usize {
    tree.node(id).attrs.len()
        + tree
            .children(id)
            .map(|child| attribute_count(tree, child))
            .sum::<usize>()
}

/// `count` heavy tags of `name`, each with 1,000 attributes and its own unique one.
fn heavy_tags(name: &str, count: usize) -> String {
    (0..count).map(|i| heavy_tag(name, i)).collect()
}

/// `prefix` followed by `unit` repeated up to `bytes`.
fn filled(prefix: &str, unit: &str, bytes: usize) -> String {
    let count = bytes.saturating_sub(prefix.len()) / unit.len();
    format!("{prefix}{}", unit.repeat(count))
}

/// `<name n0 .. n{count-2} unique>` with the names `{prefix}{i}` in a pseudo-random order and
/// `value` on each (none when empty): the order that makes the duplicate compare do its full work.
fn shuffled_tag(
    name: &str,
    count: usize,
    prefix: &str,
    value: &str,
    unique: &str,
    seed: &mut u64,
) -> String {
    let mut names: Vec<String> = (0..count - 1).map(|i| format!("{prefix}{i}")).collect();
    names.push(unique.to_string());
    for i in (1..names.len()).rev() {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        let j = usize::try_from(*seed % (i as u64 + 1)).unwrap();
        names.swap(i, j);
    }
    let mut tag = format!("<{name}");
    for attribute in names {
        if value.is_empty() {
            tag.push_str(&format!(" {attribute}"));
        } else {
            tag.push_str(&format!(" {attribute}=\"{value}\""));
        }
    }
    tag.push('>');
    tag
}

#[test]
fn depth_cut_keeps_partial_tree() {
    let html = format!("{}<p>x</p>", "<div>".repeat(300));
    let tree = build(&html, &ParseLimits::default()).unwrap();
    assert_eq!(deepest(&tree, tree.root(), 1), MAX_DEPTH);
    let mut names = Vec::new();
    element_names(&tree, tree.root(), &mut names);
    // `html` and the 255 divs that open below the limit; the 256th div and the `p` after it are cut.
    assert_eq!(names.len(), MAX_DEPTH);
    assert!(!names.iter().any(|name| name == "p"));
}

#[test]
fn node_limit_is_exact() {
    // `html`, `body` and `p`: two of the three are implied.
    let page = "<p>x</p>";
    assert_eq!(build(page, &limited(3)).unwrap().len(), 3);
    assert_eq!(build(page, &limited(2)).unwrap_err(), TOO_LARGE);
    // `<div` at end of input is a cut start tag: its implied `body` is the third element.
    let cut = "<html><head></head><div";
    assert_eq!(build(cut, &limited(3)).unwrap().len(), 3);
    assert_eq!(build(cut, &limited(2)).unwrap_err(), TOO_LARGE);
}

#[test]
fn huge_tag_is_rejected_within_a_second() {
    for count in [100_000, 400_000] {
        let html = tag(count);
        let start = std::time::Instant::now();
        let result = build(&html, &ParseLimits::default());
        let elapsed = start.elapsed();
        assert_eq!(
            result.unwrap_err(),
            ExtractError::Rejected(Rejection::TreeTooLarge(TreeLimit::TagAttributes))
        );
        assert!(
            elapsed < std::time::Duration::from_secs(1),
            "{count} attributes: {elapsed:?}"
        );
    }
}

#[test]
fn formatting_attack_pages_finish_in_under_a_second() {
    let cap = DEFAULT_MAX_INPUT_BYTES;
    let limits = ParseLimits::default();
    let check = |label: &str, html: &str| {
        let start = std::time::Instant::now();
        let result = build(html, &limits);
        let elapsed = start.elapsed();
        println!("{label}: {} bytes, {elapsed:?}", html.len());
        assert!(
            matches!(result, Ok(_) | Err(ExtractError::Rejected(_))),
            "{label}: {result:?}"
        );
        assert!(
            elapsed < std::time::Duration::from_secs(1),
            "{label} ({} bytes): {elapsed:?}",
            html.len()
        );
    };

    // Formatting-clone attack pages: the builder keeps one element per tag, so each finishes in time.
    let nested = |entries: usize| format!("<html><body>{}", heavy_tags("b", entries));
    let light: String = (0..250).map(|i| format!("<b id=e{i}>")).collect();
    // (label, prefix, repeated unit, count of units; 0 = up to the input cap)
    let pages: Vec<(String, String, &str, usize)> = vec![
        (
            "50 heavy b, <b></b> x 2,000".into(),
            nested(50),
            "<b></b>",
            2_000,
        ),
        (
            "50 heavy b, <b></b> x 8,000".into(),
            nested(50),
            "<b></b>",
            8_000,
        ),
        (
            "200 heavy b, <b></b> x 2,000".into(),
            nested(200),
            "<b></b>",
            2_000,
        ),
        (
            "50 heavy b, <b></b> to the cap".into(),
            nested(50),
            "<b></b>",
            0,
        ),
        (
            "200 heavy b, <b></b> to the cap".into(),
            nested(200),
            "<b></b>",
            0,
        ),
        (
            "foster, 50 heavy b, to the cap".into(),
            format!("{}<table>", nested(50)),
            "<b></b>",
            0,
        ),
        (
            "misnested </p>, 50 heavy b, to the cap".into(),
            format!("<html><body><p>{}", heavy_tags("b", 50)),
            "</p><b></b><p>",
            0,
        ),
        (
            "250 light entries, <b id></b> to the cap".into(),
            format!("<html><body>{light}"),
            "<b id=x></b>",
            0,
        ),
    ];
    for (label, prefix, unit, count) in &pages {
        let html = if *count == 0 {
            filled(prefix, unit, cap)
        } else {
            format!("{prefix}{}", unit.repeat(*count))
        };
        check(label, &html);
    }

    // Shuffled names: the sorts of a duplicate compare do their full work on each probe.
    let long_prefix = "averyveryverylongcommonprefixofnamesxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
    let kilobyte_prefix = "q".repeat(1_000);
    for (prefix, entries, probes) in [
        ("a", 50, 2_000),
        (long_prefix, 50, 700),
        (kilobyte_prefix.as_str(), 30, 20),
    ] {
        let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
        let mut html = String::from("<html><body>");
        for entry in 0..entries {
            html.push_str(&shuffled_tag(
                "b",
                1_000,
                prefix,
                "",
                &format!("u{entry}"),
                &mut seed,
            ));
        }
        for probe in 0..probes {
            html.push_str(&shuffled_tag(
                "b",
                1_000,
                prefix,
                "",
                &format!("p{probe}"),
                &mut seed,
            ));
            html.push_str("</b>");
        }
        check(
            &format!("shuffled names, prefix of {} bytes", prefix.len()),
            &html,
        );
    }

    // 14 formatting names of 14 entries of 1,000 attributes, then `<p>x`.
    let names = [
        "a", "b", "big", "code", "em", "font", "i", "nobr", "s", "small", "strike", "strong", "tt",
        "u",
    ];
    for value in [String::new(), "v".repeat(20), "v".repeat(200)] {
        let mut seed = 0x2545_F491_4F6C_DD1D_u64;
        let mut html = String::from("<html><body><p>");
        for name in names {
            for entry in 0..14 {
                html.push_str(&shuffled_tag(
                    name,
                    1_000,
                    "a",
                    &value,
                    &format!("u{entry}"),
                    &mut seed,
                ));
            }
        }
        html.push_str(&"<p>x".repeat(2_000));
        check(
            &format!("14 x 14 entries, values of {} bytes", value.len()),
            &html,
        );
    }
}

// Comments and processing instructions are dropped tokens: they add no element and no text.
#[test]
fn comments_and_processing_instructions_do_not_grow_the_arena() {
    let page = |comment: &str| {
        format!(
            "<html><body><p>a{}b</p></body></html>",
            comment.repeat(100_000)
        )
    };
    let limits = ParseLimits::default();
    let bare = build(&page(""), &limits).unwrap().len();
    for comment in ["<!---->", "<!>", "<?pi>"] {
        let tree = build(&page(comment), &limits).unwrap();
        assert_eq!(tree.len(), bare, "{comment}");
    }
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
    assert!(build(&page(0), &limits).is_ok());
    assert_eq!(build(&page(1), &limits).unwrap_err(), TOO_LARGE);
    let unlimited = ParseLimits {
        max_nodes: None,
        ..limits
    };
    assert!(build(&page(1), &unlimited).is_ok());
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
// attributes the built trees keep, per page, stay a hundredth of the attribute budget.
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
    let limits = ParseLimits {
        max_nodes: None,
        ..ParseLimits::default()
    };
    let mut most = (0, std::path::PathBuf::new());
    for file in &files {
        let html = String::from_utf8_lossy(&std::fs::read(file).unwrap()).into_owned();
        let Ok(tree) = build_document(&html, &limits, &Call::default()) else {
            continue;
        };
        let attributes: usize = tree
            .document_roots
            .iter()
            .map(|&id| attribute_count(&tree, id))
            .sum();
        if attributes > most.0 {
            most = (attributes, file.clone());
        }
    }
    println!(
        "{} pages, most attributes {} in {:?}",
        files.len(),
        most.0,
        most.1
    );
    let budget = DEFAULT_MAX_NODES * ATTRIBUTES_PER_NODE;
    assert!(most.0 * 100 <= budget, "{} of {budget}", most.0);
}

#[test]
fn nothing_after_the_stop() {
    // The third tag carries one attribute past `MAX_TAG_ATTRIBUTES`; the 10,000 `<p>` after it are never read.
    let html = format!(
        "<html><body><p{}>{}",
        attrs(MAX_TAG_ATTRIBUTES + 1),
        "<p>".repeat(10_000)
    );
    let (result, stats) = build_with(&html, &ParseLimits::default(), &Call::default());
    assert_eq!(
        result.unwrap_err(),
        ExtractError::Rejected(Rejection::TreeTooLarge(TreeLimit::TagAttributes))
    );
    // `html`, `body`, then the abort.
    assert_eq!(stats.tokens, 3);
    assert_eq!(stats.tokens_after_abort, 0);
}

#[test]
fn deadline_checked_in_the_token_loop() {
    // Each `<!x>` is a bogus comment: one `Skipped` token, no element.
    let bogus = "<!x>".repeat(256 * 1024);
    let trip = CheckTrip::counting();
    let (result, stats) = build_with(
        &bogus,
        &ParseLimits::default(),
        &Call::default().trip(&trip),
    );
    assert_eq!(
        result.unwrap_err(),
        ExtractError::Rejected(Rejection::EmptyTree)
    );
    assert_eq!(stats.tokens, 256 * 1024);
    // One check before the loop, then one per `CHECK_INTERVAL` tokens.
    assert_eq!(trip.checks(), 1 + stats.tokens.div_ceil(CHECK_INTERVAL));

    // Pairs, not a nest: `<b>` alone nests past `MAX_DEPTH` and the parse ends before the third check.
    let html = "<b></b>".repeat(100_000);
    let trip = CheckTrip::at(3);
    let (result, stats) = build_with(&html, &ParseLimits::default(), &Call::default().trip(&trip));
    assert_eq!(result.unwrap_err(), ExtractError::Cancelled);
    assert_eq!(stats.tokens, CHECK_INTERVAL);
}
