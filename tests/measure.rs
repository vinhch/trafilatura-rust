#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Time and heap of decode + parse per page, `load_html` over the parity corpus, for comparison with
//! upstream's `trafilatura.utils.load_html` (`tools/parity/parity/measure.py`). A measurement, not a
//! test: without `TRAFILATURA_MEASURE=1` it prints a notice and exits. Run it through
//! `scripts/measure.sh`; `TRAFILATURA_MEASURE_ONLY` keeps pages whose source contains the substring.

#[allow(dead_code)]
mod common;

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use trafilatura_rust::__internals::{HtmlInput, ParseLimits, load_html};
use trafilatura_rust::Call;

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// Timed runs per page; the median is reported.
const RUNS: usize = 5;

fn load(bytes: &[u8]) -> bool {
    load_html(
        HtmlInput::Bytes(bytes),
        &ParseLimits::default(),
        &Call::default(),
    )
    .is_ok()
}

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
    let timings = (0..RUNS)
        .map(|_| {
            let start = Instant::now();
            let loaded = load(bytes);
            let elapsed = start.elapsed();
            std::hint::black_box(loaded);
            elapsed
        })
        .collect();
    // A fresh profiler per page: its peak (`max_bytes`) covers only this page. Profiling slows the
    // allocator, so it never overlaps the timed runs. The input is already read, as in the Python side.
    let profiler = dhat::Profiler::builder().testing().build();
    let before = dhat::HeapStats::get();
    let tree = load_html(
        HtmlInput::Bytes(bytes),
        &ParseLimits::default(),
        &Call::default(),
    );
    let after = dhat::HeapStats::get();
    let loaded = tree.is_ok();
    drop(tree);
    drop(profiler);
    json!({
        "input_bytes": bytes.len(),
        "loaded": loaded,
        "median_ns": u64::try_from(median(timings).as_nanos()).unwrap_or(u64::MAX),
        "alloc_bytes": after.total_bytes.saturating_sub(before.total_bytes),
        "alloc_count": after.total_blocks.saturating_sub(before.total_blocks),
        "retained_bytes": after.curr_bytes.saturating_sub(before.curr_bytes),
        "retained_blocks": after.curr_blocks.saturating_sub(before.curr_blocks),
        "peak_bytes": after.max_bytes,
    })
}

fn main() {
    if std::env::var_os("TRAFILATURA_MEASURE").is_none_or(|value| value != "1") {
        println!("measure: skipped; run scripts/measure.sh (or set TRAFILATURA_MEASURE=1)");
        return;
    }
    let upstream = common::upstream_root().expect("upstream checkout not found");
    let fixtures = common::fixture_dir().expect("fixtures missing: run scripts/fixtures.sh first");
    let only = std::env::var("TRAFILATURA_MEASURE_ONLY").unwrap_or_default();
    let index: Value =
        serde_json::from_str(&std::fs::read_to_string(fixtures.join("index.json")).unwrap())
            .unwrap();
    let pages: Vec<Value> = index["pages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["source"].as_str().unwrap().contains(&only))
        .map(|entry| {
            let source = entry["source"].as_str().unwrap();
            let bytes = std::fs::read(upstream.join(source)).unwrap();
            let mut page = measure_page(&bytes);
            page["id"] = entry["id"].clone();
            page["source"] = entry["source"].clone();
            println!("{source}");
            page
        })
        .collect();
    let pin = common::pin();
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(".temp/measure");
    std::fs::create_dir_all(&out).unwrap();
    let path = out.join(format!("{pin}-rust.json"));
    let document = json!({ "pin": pin, "runs": RUNS, "peak_scope": "page", "pages": pages });
    std::fs::write(&path, serde_json::to_string_pretty(&document).unwrap()).unwrap();
    println!("wrote {} pages to {}", pages.len(), path.display());
}
