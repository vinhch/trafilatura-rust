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

use measure::{CorpusPage, outcome, sample};
use trafilatura_rust::{Call, ExtractError, Input, InputLimit, Options, Rejection, TreeLimit};

fn rejected(rejection: Rejection) -> Result<(), ExtractError> {
    Err(ExtractError::Rejected(rejection))
}

#[test]
fn outcome_words_match_0_1_0() {
    // `MissingFields` has no public constructor, so the payload comes from a real rejection
    let no_metadata = Options::new()
        .only_with_metadata(true)
        .build()
        .bare_extraction(
            &Input::text("<html><body><p>x</p></body></html>"),
            &Call::default(),
        )
        .map(|_| ());
    assert!(matches!(
        no_metadata,
        Err(ExtractError::Rejected(Rejection::MissingMetadata(_)))
    ));
    assert_eq!(outcome(&no_metadata), "Rejected(MissingMetadata)");
    for (text_chars, comments_chars) in [(0, 0), (3, 7)] {
        let short = rejected(Rejection::TooShort {
            text_chars,
            comments_chars,
        });
        assert_eq!(outcome(&short), "Rejected(TooShort)");
    }
    for limit in [
        InputLimit::MaxInputBytes,
        InputLimit::MaxFileSize,
        InputLimit::JsonSize,
    ] {
        assert_eq!(
            outcome(&rejected(Rejection::InputTooLarge(limit))),
            "Rejected(InputTooLarge)"
        );
    }
    assert_eq!(
        outcome(&rejected(Rejection::TreeTooLarge(TreeLimit::MaxNodes))),
        "too_large"
    );
    assert_eq!(outcome(&Err::<(), _>(ExtractError::Cancelled)), "Cancelled");
    assert_eq!(
        outcome(&Err::<(), _>(ExtractError::DeadlineExceeded)),
        "DeadlineExceeded"
    );
}

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
