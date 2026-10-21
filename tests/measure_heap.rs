#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Heap of decode + parse per page: allocation count and bytes, retained heap and peak, from dhat.
//! A measurement, not a test: it runs only with `TRAFILATURA_MEASURE=1` (see `scripts/measure.sh`).
//! The dhat allocator slows every allocation, so the timings come from `measure_time`.

mod measure;

use serde_json::{Value, json};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn measure_page(bytes: &[u8]) -> Value {
    // One unprofiled load first, so lazily built statics (compiled regexes) are not charged to the first page.
    drop(measure::load(bytes));
    // A fresh profiler per page (dhat allows one at a time): its peak, `max_bytes`, covers only this
    // page. The input is already read, as on the Python side.
    let profiler = dhat::Profiler::builder().testing().build();
    let before = dhat::HeapStats::get();
    let tree = measure::load(bytes);
    let after = dhat::HeapStats::get();
    let loaded = tree.is_ok();
    drop(tree);
    drop(profiler);
    json!({
        "loaded": loaded,
        "alloc_bytes": after.total_bytes.saturating_sub(before.total_bytes),
        "alloc_count": after.total_blocks.saturating_sub(before.total_blocks),
        "retained_bytes": after.curr_bytes.saturating_sub(before.curr_bytes),
        "retained_blocks": after.curr_blocks.saturating_sub(before.curr_blocks),
        "peak_bytes": after.max_bytes,
    })
}

fn main() {
    if measure::requested("measure_heap") {
        measure::run("heap", json!({ "peak_scope": "page" }), measure_page);
    }
}
