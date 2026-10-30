#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Timing of `load_html` and `extract` over the corpus sample, and of one ordered-list page, with
//! criterion. Run with `cargo bench --features __internals --bench extract`; without the corpus it
//! prints a notice and exits.

#[allow(dead_code)]
#[path = "../tests/measure/mod.rs"]
mod measure;

use std::hint::black_box;

use criterion::Criterion;

fn main() {
    let Some(pages) = measure::corpus() else {
        println!("extract bench: skipped; run scripts/fixtures.sh");
        return;
    };
    // Read before the groups so a group times the call, not the disk.
    let inputs: Vec<(&str, Vec<u8>)> = measure::sample(&pages)
        .into_iter()
        .map(|page| (page.id.as_str(), std::fs::read(&page.path).unwrap()))
        .collect();
    let mut criterion = Criterion::default().configure_from_args();

    let mut group = criterion.benchmark_group("load_html");
    for (id, bytes) in &inputs {
        group.bench_function(*id, |b| b.iter(|| black_box(measure::load(bytes))));
    }
    group.finish();

    let mut group = criterion.benchmark_group("extract");
    for (id, bytes) in &inputs {
        group.bench_function(*id, |b| b.iter(|| black_box(measure::extract(bytes))));
    }
    group.finish();

    // The list is large, so each iteration takes long; ten samples keep the group in seconds.
    let list = measure::ordered_list_page("ordered_list_50k", 50_000);
    let mut group = criterion.benchmark_group("synthetic");
    group.sample_size(10);
    group.bench_function(list.name, |b| b.iter(|| black_box(list.extract())));
    group.finish();

    criterion.final_summary();
}
