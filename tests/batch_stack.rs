//! Stack check of the batch workers: every page of the parity corpus through `extract_batch` on one
//! worker thread, in the dev profile where stack frames are largest. A page that needs more stack
//! than `WORKER_STACK_BYTES` aborts the process, so a pass shows the stack holds the corpus.
//! `#[ignore]`d and run by hand, see `docs/batch.md`.

#[allow(dead_code)]
mod common;

use std::fs;
use std::num::NonZeroUsize;
use std::path::PathBuf;

use trafilatura_rust::{BatchEnd, BatchOptions, Call, Input, Options};

#[test]
#[ignore = "runs every corpus page on a worker thread; run with --ignored"]
fn batch_worker_stack_fits_corpus() {
    let Some((fixtures, upstream)) = common::fixture_dir().zip(common::upstream_root()) else {
        assert!(
            !common::require_fixtures(),
            "PARITY_REQUIRE_FIXTURES is set but the fixtures or the upstream checkout are missing"
        );
        eprintln!("batch_stack: fixtures or the upstream checkout are missing; skipped");
        return;
    };
    let index: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(fixtures.join("index.json")).expect("index.json is readable"),
    )
    .expect("index.json is JSON");
    let paths: Vec<PathBuf> = index["pages"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|page| page["source"].as_str())
        .map(|source| upstream.join(source))
        .collect();

    let extractor = Options::new().build();
    let mut callbacks = 0;
    let mut extracted = 0;
    let end = extractor.extract_batch(
        paths.iter(),
        BatchOptions::new(NonZeroUsize::MIN),
        &Call::default(),
        |path: &PathBuf, call: &Call<'_>| {
            let bytes = fs::read(path).expect("corpus page is readable");
            extractor.extract(&Input::bytes(&bytes), call)
        },
        |_, result| {
            callbacks += 1;
            if matches!(result, Ok(Ok(_))) {
                extracted += 1;
            }
        },
    );

    assert_eq!(end, Ok(BatchEnd::Completed));
    assert_eq!(callbacks, paths.len(), "every page gets a callback");
    assert!(extracted > 0, "no corpus page was extracted");
    eprintln!(
        "batch_stack: {extracted} of {} corpus pages extracted",
        paths.len()
    );
}
