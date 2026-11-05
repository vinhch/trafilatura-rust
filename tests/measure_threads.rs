#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Thread scaling of `Extractor::extract` over the corpus sample: pages per second on 1, 2, 4 and 8
//! scoped threads sharing one extractor, and the ratio to one thread. A measurement, not a test: it
//! runs only with `TRAFILATURA_MEASURE=1`, natively (`scripts/measure.sh --threads-native`). It has no
//! dhat allocator, because dhat serializes allocations and would hide the scaling.

#[allow(dead_code)]
mod measure;

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use trafilatura_rust::{Call, Extractor, Format, Input, Options};

/// Thread counts measured, in order; the first is the one-thread baseline of the ratio.
const THREAD_COUNTS: [usize; 4] = [1, 2, 4, 8];

/// Each count keeps extracting until at least this much time has passed, so every run is long
/// enough to be steady.
const MIN_DURATION: Duration = Duration::from_secs(2);

/// Extracts pages from `pages` in turn, taking the next index from `next`, until `MIN_DURATION` has
/// passed since `start`. Returns the number of extractions it made.
fn worker(extractor: &Extractor, pages: &[Vec<u8>], next: &AtomicUsize, start: Instant) -> usize {
    let mut done = 0;
    // The check comes before the take, so a worker never starts an extraction after the deadline.
    while start.elapsed() < MIN_DURATION {
        let index = next.fetch_add(1, Ordering::Relaxed);
        let page = &pages[index % pages.len()];
        // The result is dropped: the timing is of the call, and black_box keeps it from being elided.
        let _ = std::hint::black_box(extractor.extract(&Input::bytes(page), &Call::default()));
        done += 1;
    }
    done
}

/// Runs `threads` scoped workers over `pages` and returns the total number of extractions and the
/// wall time from the start to the last join. An extraction in flight at the deadline finishes and
/// counts, so the time includes it.
fn run(extractor: &Extractor, pages: &[Vec<u8>], threads: usize) -> (usize, Duration) {
    let next = AtomicUsize::new(0);
    let start = Instant::now();
    let done: usize = thread::scope(|scope| {
        let workers: Vec<_> = (0..threads)
            .map(|_| scope.spawn(|| worker(extractor, pages, &next, start)))
            .collect();
        workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .sum()
    });
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
