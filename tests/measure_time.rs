#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Time of `load_html` (decode + parse) and of `extract` (which includes both) per corpus page,
//! 5-run median, and one run of each synthetic page, with the system allocator. A measurement, not a
//! test: it runs only with `TRAFILATURA_MEASURE=1` (see `scripts/measure.sh`, which also runs
//! `measure_heap` and upstream Python).

mod measure;

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use trafilatura_rust::ExtractError;

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

/// The median time in nanoseconds of `RUNS` calls, and whether the last one succeeded. The result
/// drops after the clock stops.
fn timed<T>(mut call: impl FnMut() -> Result<T, ExtractError>) -> (bool, u64) {
    let mut ok = false;
    let timings = (0..RUNS)
        .map(|_| {
            let start = Instant::now();
            let result = std::hint::black_box(call());
            let elapsed = start.elapsed();
            ok = result.is_ok();
            drop(result);
            elapsed
        })
        .collect();
    (
        ok,
        u64::try_from(median(timings).as_nanos()).unwrap_or(u64::MAX),
    )
}

fn measure_page(bytes: &[u8]) -> Value {
    let (loaded, load_ns) = timed(|| measure::load(bytes));
    let (extract_ok, extract_ns) = timed(|| measure::extract(bytes));
    json!({
        "loaded": loaded,
        "median_ns": load_ns,
        "extract_ok": extract_ok,
        "extract_median_ns": extract_ns,
    })
}

/// One run, no warm-up: the 200,000-item list alone takes minutes.
fn measure_synthetic(page: &measure::Synthetic) -> Value {
    let start = Instant::now();
    let result = page.extract();
    let elapsed = start.elapsed();
    json!({
        "outcome": measure::outcome(&result),
        "output_bytes": result.as_ref().ok().map(String::len),
        "elapsed_ns": u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX),
    })
}

fn main() {
    if measure::requested("measure_time") {
        measure::run_all(
            "time",
            json!({ "runs": RUNS }),
            measure_page,
            |_| true,
            measure_synthetic,
        );
    }
}
