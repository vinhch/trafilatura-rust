#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! The corpus sample of `tests/measure`: which pages the bench and the heap sample mode measure.

#[allow(dead_code)]
mod measure;

use std::path::PathBuf;

use measure::{CorpusPage, sample};

fn page(id: &str, size_bytes: u64) -> CorpusPage {
    CorpusPage {
        id: id.to_owned(),
        source: format!("tests/{id}.html"),
        path: PathBuf::from(id),
        size_bytes,
    }
}

fn ids<'a>(pages: &[&'a CorpusPage]) -> Vec<&'a str> {
    pages.iter().map(|page| page.id.as_str()).collect()
}

#[test]
fn sample_picks_smallest_median_and_largest() {
    // Listed out of order: the sample sorts by size, not by index order.
    let pages: Vec<CorpusPage> = [30, 90, 10, 60, 20, 80, 50, 70, 40]
        .into_iter()
        .map(|size| page(&format!("p{size}"), size))
        .collect();
    assert_eq!(ids(&sample(&pages)), ["p10", "p50", "p70", "p80", "p90"]);
}

#[test]
fn sample_of_few_pages_has_no_duplicates() {
    let pages = [page("b", 2), page("a", 1)];
    assert_eq!(ids(&sample(&pages)), ["a", "b"]);
    assert!(sample(&[]).is_empty());
}

#[test]
fn sample_breaks_size_ties_by_id() {
    let pages = [page("z", 5), page("a", 5)];
    assert_eq!(ids(&sample(&pages)), ["a", "z"]);
}
