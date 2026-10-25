#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Heap of `load_html` (decode + parse) and of `extract` (which includes both) per corpus page, and
//! of each synthetic page but the ordered lists: allocation count and bytes, retained heap and peak,
//! from dhat. A measurement, not a test: it runs only with `TRAFILATURA_MEASURE=1` (see
//! `scripts/measure.sh`). The dhat allocator slows every allocation, so the timings come from
//! `measure_time`, which also times the ordered lists (dhat would multiply their minutes).

mod measure;

use serde_json::{Value, json};
use trafilatura_rust::ExtractError;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// Heap of one call, as `measure_page` and `measure_synthetic` report it.
struct Heap {
    outcome: Result<(), ExtractError>,
    alloc_bytes: u64,
    alloc_count: u64,
    retained_bytes: usize,
    retained_blocks: usize,
    peak_bytes: usize,
}

/// A fresh profiler per call (dhat allows one at a time): its peak, `max_bytes`, covers only this
/// call. The input is already read, as on the Python side. With `warm_up`, one unprofiled call
/// comes first, so lazily built statics (compiled regexes) are not charged to the first page.
fn profiled<T>(warm_up: bool, call: impl Fn() -> Result<T, ExtractError>) -> Heap {
    if warm_up {
        drop(call());
    }
    let profiler = dhat::Profiler::builder().testing().build();
    let before = dhat::HeapStats::get();
    let result = call();
    let after = dhat::HeapStats::get();
    let outcome = result.map(drop);
    drop(profiler);
    Heap {
        outcome,
        alloc_bytes: after.total_bytes.saturating_sub(before.total_bytes),
        alloc_count: after.total_blocks.saturating_sub(before.total_blocks),
        retained_bytes: after.curr_bytes.saturating_sub(before.curr_bytes),
        retained_blocks: after.curr_blocks.saturating_sub(before.curr_blocks),
        peak_bytes: after.max_bytes,
    }
}

fn measure_page(bytes: &[u8]) -> Value {
    let load = profiled(true, || measure::load(bytes));
    let extract = profiled(true, || measure::extract(bytes));
    json!({
        "loaded": load.outcome.is_ok(),
        "alloc_bytes": load.alloc_bytes,
        "alloc_count": load.alloc_count,
        "retained_bytes": load.retained_bytes,
        "retained_blocks": load.retained_blocks,
        "peak_bytes": load.peak_bytes,
        "extract_ok": extract.outcome.is_ok(),
        "extract_alloc_bytes": extract.alloc_bytes,
        "extract_alloc_count": extract.alloc_count,
        "extract_peak_bytes": extract.peak_bytes,
    })
}

/// One profiled run, no warm-up.
fn measure_synthetic(page: &measure::Synthetic) -> Value {
    let extract = profiled(false, || page.extract());
    json!({
        "outcome": measure::outcome(&extract.outcome),
        "alloc_bytes": extract.alloc_bytes,
        "alloc_count": extract.alloc_count,
        "peak_bytes": extract.peak_bytes,
    })
}

fn main() {
    if measure::requested("measure_heap") {
        measure::run_all(
            "heap",
            json!({ "peak_scope": "page" }),
            measure_page,
            |page| !page.name.starts_with("ordered_list"),
            measure_synthetic,
        );
    }
}
