use super::*;
use crate::dom::{DumpMode, dump};

// Without the scan html5ever compares about N^2 / 2 names, which takes minutes for these sizes.
#[test]
fn huge_tag_is_rejected_within_a_second() {
    for count in [100_000, 400_000] {
        let html = tag(count);
        let start = std::time::Instant::now();
        let result = parse(&html, false);
        let elapsed = start.elapsed();
        assert_eq!(
            result.unwrap_err(),
            ExtractError::Rejected(Rejection::TreeTooLarge)
        );
        assert!(
            elapsed < std::time::Duration::from_secs(1),
            "{count} attributes: {elapsed:?}"
        );
    }
}

// A run of BOMs fills a whole piece, which cannot start or end at one (html5ever drops a BOM at
// the start of each `feed`): the piece grows to the end of the run instead.
#[test]
fn bom_runs_do_not_stall_the_drive() {
    let boms = |count: usize| BOM.to_string().repeat(count);
    let mut pages = vec![
        format!("x{}", boms(3_000)),
        format!("<style>{}", boms(3_000)),
        format!("<script>{}</script>", boms(3_000)),
        format!("<p>x{}y</p>", boms(2 * CHUNK_BYTES)),
        format!("<style>{}</style>y", boms(2 * CHUNK_BYTES)),
        boms(CHUNK_BYTES),
        format!("{}y", boms(CHUNK_BYTES)),
    ];
    for k in 0..=8 {
        pages.push(format!("{}{}tail", "x".repeat(CHUNK_BYTES - k), boms(4)));
        pages.push(format!(
            "{}{}tail",
            "x".repeat(CHUNK_BYTES - k),
            boms(4_000)
        ));
        pages.push(format!(
            "<p>{}{}</p>",
            "x".repeat(CHUNK_BYTES - 3 - k),
            boms(3)
        ));
    }
    for html in &pages {
        for fragment in [false, true] {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
            let call = Call::default().deadline(deadline);
            let limits = ParseLimits::default();
            let result = if fragment {
                parse_fragment(html, &limits, &call)
            } else {
                parse_document(html, &limits, &call)
            };
            let label: String = html.chars().take(12).collect();
            let tree = result.unwrap_or_else(|error| {
                panic!(
                    "{label:?} ({} bytes) fragment {fragment}: {error:?}",
                    html.len()
                )
            });
            assert_eq!(
                dump(&tree, DumpMode::Exact),
                dump(&whole_tree(html, fragment), DumpMode::Exact),
                "{label:?} ({} bytes) fragment {fragment}",
                html.len()
            );
        }
    }
}

#[test]
fn a_bom_after_a_chunk_boundary_is_kept() {
    let padded = format!("<p>{}</p>", "x".repeat(CHUNK_BYTES - 7));
    let html = format!("{padded}{BOM}y");
    let tree = parse(&html, false).unwrap();
    assert_eq!(
        dump(&tree, DumpMode::Exact),
        dump(&whole_tree(&html, false), DumpMode::Exact)
    );
    assert!(dump(&tree, DumpMode::Exact).0.contains(BOM));
}

fn heavy_tags(name: &str, count: usize) -> String {
    (0..count).map(|i| heavy_tag(name, i)).collect()
}

/// `prefix` followed by `unit` repeated up to `bytes`.
fn filled(prefix: &str, unit: &str, bytes: usize) -> String {
    let count = bytes.saturating_sub(prefix.len()) / unit.len();
    format!("{prefix}{}", unit.repeat(count))
}

// The budget of 2,000 nodes is 80,000 units; the compare of one `<b></b>` against 10 entries of
// 1,000 attributes is about 20,000.
#[test]
fn compare_budget_rejects_the_heavy_entries_attack_at_a_small_limit() {
    let attack = |prefix: &str, unit: &str| format!("<html><body>{prefix}{}", unit.repeat(200));
    let heavy = heavy_tags("b", 10);
    let pages = [
        attack(&heavy, "<b></b>"),
        attack(&format!("{heavy}<table>"), "<b></b>"),
        attack(&format!("<p>{heavy}"), "</p><b></b><p>"),
        attack(&format!("{}<table>", heavy_tag("b", 0)), "</b>"),
    ];
    for (index, html) in pages.iter().enumerate() {
        let (abort, work) = compare_outcome(html, 2_000);
        assert_eq!(abort, Some(TOO_LARGE), "page {index}");
        assert!(work > 80_000, "{work}");
        assert_eq!(
            parse_document(html, &limited(2_000), &Call::default()).unwrap_err(),
            TOO_LARGE
        );
    }
}

// `max_nodes: None` lifts the compare budget as it lifts the node limit.
#[test]
fn compare_budget_is_lifted_with_the_node_limit() {
    let html = format!(
        "<html><body>{}{}",
        heavy_tags("b", 10),
        "<b></b>".repeat(200)
    );
    let unlimited = ParseLimits {
        max_nodes: None,
        ..ParseLimits::default()
    };
    assert!(parse_document(&html, &unlimited, &Call::default()).is_ok());
}

// Attack pages at the size they were measured (release: 11 s each without the budget) and at the
// input cap: each ends rejected in under a second.
#[test]
#[ignore = "slow"]
fn attack_pages_are_rejected_in_under_a_second() {
    let cap = DEFAULT_MAX_INPUT_BYTES;
    let nested = |entries: usize| format!("<html><body>{}", heavy_tags("b", entries));
    let light: String = (0..250).map(|i| format!("<b id=e{i}>")).collect();
    // (label, prefix, repeated unit, count of units; 0 = up to the input cap)
    let mut pages: Vec<(String, String, &str, usize)> = vec![
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
    // End tags of an out-of-scope formatting element: a clone of its attributes each.
    for (name, end) in [("b", "</b>"), ("a", "</a>"), ("nobr", "</nobr>")] {
        for scope in ["<table>", "<svg><desc>"] {
            pages.push((
                format!("{end} under {scope}, to the cap"),
                format!("<html><body>{}{scope}", heavy_tag(name, 0)),
                end,
                0,
            ));
        }
    }
    let limits = ParseLimits::default();
    for (label, prefix, unit, count) in &pages {
        let html = if *count == 0 {
            filled(prefix, unit, cap)
        } else {
            format!("{prefix}{}", unit.repeat(*count))
        };
        let start = std::time::Instant::now();
        let result = parse_document(&html, &limits, &Call::default());
        let elapsed = start.elapsed();
        println!("{label}: {} bytes, {elapsed:?}", html.len());
        assert_eq!(result.unwrap_err(), TOO_LARGE, "{label}");
        assert!(
            elapsed < std::time::Duration::from_secs(1),
            "{label} ({} bytes): {elapsed:?}",
            html.len()
        );
    }
}

/// `<name n0 .. n{count-2} unique>` with the names `{prefix}{i}` in a pseudo-random order and
/// `value` on each (none when empty): the order that makes `Tag::equiv_modulo_attr_order`'s sorts
/// do their full work, where an in-order tag costs one pass.
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

fn assert_rejected_in_under_a_second(label: &str, html: &str) {
    let start = std::time::Instant::now();
    let result = parse_document(html, &ParseLimits::default(), &Call::default());
    let elapsed = start.elapsed();
    println!("{label}: {} bytes, {elapsed:?}", html.len());
    assert_eq!(result.unwrap_err(), TOO_LARGE, "{label}");
    assert!(
        elapsed < std::time::Duration::from_secs(1),
        "{label} ({} bytes): {elapsed:?}",
        html.len()
    );
}

// The compare sorts both attribute lists, so shuffled names cost about ten times an in-order
// list (116 ns against 13 ns per attribute at 1,000) and names with a long common prefix more:
// 50 heavy `b` then probes of 1,000 attributes each followed by `</b>`.
#[test]
#[ignore = "slow"]
fn shuffled_name_attack_pages_are_rejected_in_under_a_second() {
    let long_prefix = "averyveryverylongcommonprefixofnamesxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";
    let kilobyte_prefix = "q".repeat(1_000);
    // (prefix, heavy entries, probes), each page up to about 63 MB
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
        let label = format!("shuffled names, prefix of {} bytes", prefix.len());
        assert_rejected_in_under_a_second(&label, &html);
    }
}

// Reconstruction clones cost html5ever a clone of the attributes each (50 to 120 ns per attribute,
// more with long values): 14 formatting names of 14 entries of 1,000 attributes, then `<p>x`.
#[test]
#[ignore = "slow"]
fn reconstruction_attack_pages_are_rejected_in_under_a_second() {
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
        let label = format!("14 x 14 entries, values of {} bytes", value.len());
        assert_rejected_in_under_a_second(&label, &html);
    }
}

// Tags that cost html5ever nothing after a deep stack of heavy formatting elements: the budget
// may be nearly spent (N = 40 to 42 heavy entries come close) yet every later tag is free, so
// nothing may make each of them expensive. Each page takes at most 1.5 times as long as the same
// tail behind N entries without attributes, whether it ends rejected or parsed. A `</x>` walks
// the open elements to find its match, so its cost grows with the depth (html5ever's own cost,
// attributes or not; the figures are in D-5); a comment does not, and takes what it takes alone.
#[test]
#[ignore = "slow"]
fn free_tags_behind_heavy_entries_cost_what_they_cost_alone() {
    let cap = DEFAULT_MAX_INPUT_BYTES;
    let time = |html: &str, limits: &ParseLimits| {
        let start = std::time::Instant::now();
        let result = parse_document(html, limits, &Call::default());
        (start.elapsed(), result.is_ok())
    };
    let default = ParseLimits::default();
    let mut failures = Vec::new();
    for tail in ["</x>", "<!---->"] {
        let tail_only = tail.repeat(cap / tail.len());
        let alone = time(&format!("<html><body>{tail_only}"), &default).0;
        for entries in [1, 20, 40, 42, 43, 50] {
            let mut seed = 0x9E37_79B9_7F4A_7C15_u64;
            let heavy: String = (0..entries)
                .map(|entry| {
                    let unique = format!("u{entry}");
                    shuffled_tag("b", MAX_TAG_ATTRIBUTES, "a", "", &unique, &mut seed)
                })
                .collect();
            let light: String = (0..entries).map(|entry| format!("<b u{entry}>")).collect();
            let (light_time, _) = time(&format!("<html><body>{light}{tail_only}"), &default);
            let (elapsed, parsed) = time(&format!("<html><body>{heavy}{tail_only}"), &default);
            println!(
                "{entries} heavy b, {tail} to the cap: {elapsed:?} (light entries {light_time:?}, alone {alone:?}), parsed {parsed}"
            );
            if elapsed >= light_time * 3 / 2 {
                failures.push(format!(
                    "{entries} heavy b, {tail}: {elapsed:?} against {light_time:?} with light entries"
                ));
            }
            if tail == "<!---->" && elapsed >= alone * 3 / 2 {
                failures.push(format!(
                    "{entries} heavy b, {tail}: {elapsed:?} against {alone:?} alone"
                ));
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

// The same deep stacks with the budgets lifted: what a heavy entry adds is nothing beyond what
// the depth costs html5ever.
#[test]
#[ignore = "slow"]
fn deep_stacks_cost_the_same_with_attributes_and_the_budgets_lifted() {
    let tail = "</x>".repeat(DEFAULT_MAX_INPUT_BYTES / 4);
    let unlimited = ParseLimits {
        max_nodes: None,
        max_input_bytes: None,
        ..ParseLimits::default()
    };
    let time = |html: &str| {
        let start = std::time::Instant::now();
        parse_document(html, &unlimited, &Call::default()).unwrap();
        start.elapsed()
    };
    for entries in [43, 50, 250] {
        let mut seed = 0x2545_F491_4F6C_DD1D_u64;
        let heavy: String = (0..entries)
            .map(|entry| {
                let unique = format!("u{entry}");
                shuffled_tag("b", MAX_TAG_ATTRIBUTES, "a", "", &unique, &mut seed)
            })
            .collect();
        let light: String = (0..entries).map(|entry| format!("<b u{entry}>")).collect();
        let light_time = time(&format!("<html><body>{light}{tail}"));
        let heavy_time = time(&format!("<html><body>{heavy}{tail}"));
        println!("{entries} entries, </x> to the cap: heavy {heavy_time:?}, light {light_time:?}");
        assert!(heavy_time < light_time * 3 / 2, "{entries} entries");
    }
}

// The other exits of the adoption agency either remove the entry or are the plain cost of a tag:
// with a heavy open formatting element a page of end tags takes as long as without attributes.
#[test]
#[ignore = "slow"]
fn end_tags_that_drop_the_entry_cost_the_same_with_a_heavy_element() {
    let bytes = 16 * 1024 * 1024;
    let time = |html: &str| {
        let start = std::time::Instant::now();
        let limits = ParseLimits {
            max_input_bytes: None,
            ..ParseLimits::default()
        };
        parse_document(html, &limits, &Call::default()).unwrap();
        start.elapsed()
    };
    for open in ["<div>", "<div><div>", "<p>x", "<button>", "<table><tr><td>"] {
        let heavy = time(&filled(
            &format!("<html><body>{}{open}", heavy_tag("b", 0)),
            "</b>",
            bytes,
        ));
        let light = time(&filled(&format!("<html><body><b>{open}"), "</b>", bytes));
        assert!(
            heavy < light * 2,
            "{open}: heavy {heavy:?}, light {light:?}"
        );
    }
}

// The not-in-scope end tag is charged through html5ever's parse error: this fails if the message
// changes, which would silently uncharge the clone each such end tag makes.
#[test]
fn not_in_scope_parse_error_message_is_charged() {
    let open = format!("<html><body>{}<table>", heavy_tag("b", 0));
    let (_, without) = compare_outcome(&open, 500_000);
    let (_, with) = compare_outcome(&format!("{open}</b>"), 500_000);
    assert_eq!(with - without, MAX_TAG_ATTRIBUTES + PAIR_UNITS);
    // A scoped end tag is an ordinary close.
    let (_, closed) = compare_outcome(&format!("<html><body>{}</b>", heavy_tag("b", 0)), 500_000);
    let (_, open_only) = compare_outcome(&format!("<html><body>{}", heavy_tag("b", 0)), 500_000);
    assert_eq!(closed, open_only);
}
