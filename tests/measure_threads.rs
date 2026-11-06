#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Thread scaling of `Extractor::extract_batch` over the corpus sample: pages per second on 1, 2, 4
//! and 8 batch workers sharing one extractor, and the ratio to one thread. A measurement, not a test:
//! it runs only with `TRAFILATURA_MEASURE=1`, natively (`scripts/measure.sh --threads-native`). It has
//! no dhat allocator, because dhat serializes allocations and would hide the scaling.

#[allow(dead_code)]
mod measure;

use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use trafilatura_rust::{BatchOptions, Call, Extractor, Format, Input, Options};

/// Thread counts measured, in order; the first is the one-thread baseline of the ratio.
const THREAD_COUNTS: [usize; 4] = [1, 2, 4, 8];

/// Each count keeps extracting until at least this much time has passed, so every run is long
/// enough to be steady.
const MIN_DURATION: Duration = Duration::from_secs(2);

/// Extracts the pages of `pages` in turn on `threads` batch workers until `MIN_DURATION` has passed
/// since `start`. Returns the number of extractions and the wall time to the last result. The
/// iterator checks the time before each take, so no extraction starts after the deadline; an
/// extraction in flight finishes and counts, so the time includes it.
fn run(extractor: &Extractor, pages: &[Vec<u8>], threads: usize) -> (usize, Duration) {
    let start = Instant::now();
    let items = pages
        .iter()
        .cycle()
        .take_while(|_| start.elapsed() < MIN_DURATION);
    let options = BatchOptions::new(NonZeroUsize::new(threads).expect("thread counts are nonzero"));
    let mut done = 0;
    // No token and no batch deadline, so no item is skipped and every result counts.
    extractor
        .extract_batch(
            items,
            options,
            &Call::default(),
            |page: &Vec<u8>, call: &Call<'_>| extractor.extract(&Input::bytes(page), call),
            |_, _| done += 1,
        )
        .expect("the batch has no token or deadline and its workers do not panic");
    (done, start.elapsed())
}

fn main() {
    if !measure::requested("measure_threads") {
        return;
    }
    let corpus = measure::corpus()
        .expect("the upstream checkout and the fixtures are needed: run scripts/fixtures.sh first");
    let pages: Vec<Vec<u8>> = measure::sample(&corpus)
        .into_iter()
        .map(|page| std::fs::read(&page.path).unwrap())
        .collect();
    assert!(!pages.is_empty(), "the corpus sample is empty");

    let extractor = Options::new().format(Format::Txt).build();
    let threads_available = std::thread::available_parallelism().map_or(1, NonZeroUsize::get);

    let rates: Vec<f64> = THREAD_COUNTS
        .into_iter()
        .map(|threads| {
            let (done, elapsed) = run(&extractor, &pages, threads);
            let rate = done as f64 / elapsed.as_secs_f64();
            println!("threads {threads}: {done} pages in {elapsed:?}, {rate:.2} pages/s");
            rate
        })
        .collect();
    let single = rates.first().copied().unwrap_or_default();
    let runs: Vec<Value> = THREAD_COUNTS
        .into_iter()
        .zip(&rates)
        .map(|(count, rate)| {
            json!({ "count": count, "pages_per_sec": rate, "ratio": rate / single })
        })
        .collect();

    let path = measure::output_path("threads");
    let document = json!({ "threads_available": threads_available, "runs": runs });
    std::fs::write(&path, serde_json::to_string_pretty(&document).unwrap()).unwrap();
    println!("wrote {}", path.display());
}
