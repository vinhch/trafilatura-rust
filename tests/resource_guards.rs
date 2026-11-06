#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Guards for the resource changes that swap an expression for a cheaper one with the same value.
//! The corpus half runs on the fixtures and returns early without them; the synthetic half is
//! `#[ignore]`d and run by hand, with `cargo test --test resource_guards -- --ignored`.

#[allow(dead_code)]
mod measure;

use trafilatura_rust::__internals::{HtmlInput, ParseLimits, Tree, char_len, load_html};
use trafilatura_rust::Call;

/// Whether `PARITY_REQUIRE_FIXTURES` is set and non-empty, as `common::require_fixtures` reads it.
/// Not imported from `common`: `measure` already loads that module, and a second `mod` is refused.
fn require_fixtures() -> bool {
    std::env::var_os("PARITY_REQUIRE_FIXTURES").is_some_and(|value| !value.is_empty())
}

/// Asserts that `char_len` of every node of `tree` equals the character count of its
/// `text_content`, the expression `html_processing.rs` used before `char_len`. Returns the number
/// of nodes checked, so a caller can tell a vacuous run from a passing one.
fn assert_char_len_matches_text_content(tree: &Tree, page: &str) -> usize {
    let mut checked = 0;
    for node in tree.iter(tree.root(), &[]) {
        assert_eq!(
            char_len(tree, node),
            tree.text_content(node).chars().count(),
            "{page}: node {node:?}"
        );
        checked += 1;
    }
    checked
}

#[test]
fn char_len_equals_text_content_chars_on_corpus_pages() {
    let Some(pages) = measure::corpus() else {
        assert!(
            !require_fixtures(),
            "PARITY_REQUIRE_FIXTURES is set but the fixtures or the upstream checkout are missing"
        );
        eprintln!("resource_guards: fixtures or the upstream checkout are missing; skipped");
        return;
    };
    let mut pages_checked = 0;
    let mut nodes_checked = 0;
    for page in &pages {
        let bytes = std::fs::read(&page.path).unwrap();
        // A page that fails to load gives no tree and no count; the corpus run reports those.
        if let Ok(tree) = measure::load(&bytes) {
            nodes_checked += assert_char_len_matches_text_content(&tree, &page.source);
            pages_checked += 1;
        }
    }
    assert!(pages_checked > 0, "no corpus page loaded");
    eprintln!("resource_guards: {pages_checked} corpus pages, {nodes_checked} nodes checked");
}

#[test]
#[ignore = "builds the synthetic pages, the largest over 10 MiB; run with --ignored"]
fn char_len_equals_text_content_chars_on_synthetic_pages() {
    // The synthetic pages probe the limits, so the parse limits are lifted here: the guard checks
    // the counts on the tree a page builds, not whether the page is accepted.
    let limits = ParseLimits {
        max_nodes: None,
        max_input_bytes: None,
        ..ParseLimits::default()
    };
    let mut nodes_checked = 0;
    for page in measure::synthetic() {
        let tree = load_html(
            HtmlInput::Bytes(page.html.as_bytes()),
            usize::MAX,
            &limits,
            &Call::default(),
        )
        .unwrap();
        nodes_checked += assert_char_len_matches_text_content(&tree, page.name);
    }
    assert!(nodes_checked > 0, "no synthetic node was checked");
    eprintln!("resource_guards: synthetic pages, {nodes_checked} nodes checked");
}
