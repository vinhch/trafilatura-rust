#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Time of `load_html` (decode + parse) and of `extract` (which includes both) per corpus page, and of
//! the rich option set (metadata, images, links) in Markdown and HTML, 5-run median, with the output
//! digests `parity.measure --check-digests` compares; one run of each synthetic page, with the system
//! allocator. A measurement, not a test: it runs only with `TRAFILATURA_MEASURE=1` (see
//! `scripts/measure.sh`, which also runs `measure_heap` and upstream Python).

#[allow(dead_code)]
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

/// The median time in nanoseconds of `RUNS` calls, and the last call's result. The caller reads the
/// result after the clock has stopped.
fn timed<T>(mut call: impl FnMut() -> Result<T, ExtractError>) -> (u64, Result<T, ExtractError>) {
    let mut last = None;
    let timings = (0..RUNS)
        .map(|_| {
            let start = Instant::now();
            let result = std::hint::black_box(call());
            let elapsed = start.elapsed();
            last = Some(result);
            elapsed
        })
        .collect();
    let result = last.expect("RUNS is at least one");
    (
        u64::try_from(median(timings).as_nanos()).unwrap_or(u64::MAX),
        result,
    )
}

/// Times `load_html`, `extract` and the two rich runs, and digests the last output of each.
fn measure_page(bytes: &[u8]) -> Value {
    let (load_ns, load) = timed(|| measure::load(bytes));
    let loaded = load.is_ok();
    drop(load);
    let (extract_ns, extract) = timed(|| measure::extract(bytes));
    let (md_ns, md) = timed(|| measure::extract_rich_md(bytes));
    let (html_ns, html) = timed(|| measure::extract_rich_html(bytes));
    json!({
        "loaded": loaded,
        "median_ns": load_ns,
        "extract_ok": extract.is_ok(),
        "extract_median_ns": extract_ns,
        "output_digest": measure::digest_of(&extract),
        "rich_md_median_ns": md_ns,
        "rich_md_digest": measure::digest_of(&md),
        "rich_html_median_ns": html_ns,
        "rich_html_digest": measure::digest_of(&html),
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
        "output_digest": measure::digest_of(&result),
    })
}

fn main() {
    if measure::requested("measure_time") {
        measure::run_all(
            "time",
            json!({ "runs": RUNS }),
            |_, bytes| measure_page(bytes),
            |_| true,
            measure_synthetic,
        );
    }
}
