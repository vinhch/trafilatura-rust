#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Time of decode + parse per page: `load_html` over the parity corpus, 5-run median, with the system
//! allocator. A measurement, not a test: it runs only with `TRAFILATURA_MEASURE=1` (see
//! `scripts/measure.sh`, which also runs `measure_heap` and upstream Python).

mod measure;

use std::time::{Duration, Instant};

use serde_json::{Value, json};

/// Timed runs per page; the median is reported.
const RUNS: usize = 5;

/// Median of the samples; the mean of the two middle ones for an even count.
fn median(mut samples: Vec<Duration>) -> Duration {
    samples.sort();
    let mid = samples.len() / 2;
    match (samples.get(mid.saturating_sub(1)), samples.get(mid)) {
        (Some(low), Some(high)) if samples.len().is_multiple_of(2) => {
            low.checked_add(*high).map_or(*high, |sum| sum / 2)
        }
        (_, Some(middle)) => *middle,
        _ => Duration::ZERO,
    }
}

fn measure_page(bytes: &[u8]) -> Value {
    let mut loaded = false;
    let timings = (0..RUNS)
        .map(|_| {
            let start = Instant::now();
            let tree = measure::load(bytes);
            let elapsed = start.elapsed();
            loaded = tree.is_ok();
            drop(std::hint::black_box(tree));
            elapsed
        })
        .collect();
    json!({
        "loaded": loaded,
        "median_ns": u64::try_from(median(timings).as_nanos()).unwrap_or(u64::MAX),
    })
}

fn main() {
    if measure::requested("measure_time") {
        measure::run("time", json!({ "runs": RUNS }), measure_page);
    }
}
