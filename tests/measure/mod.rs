#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Shared by `measure_time.rs` and `measure_heap.rs`: the page list, the `load_html` call and the
//! JSON file. The two targets are separate binaries because the heap one installs the dhat
//! allocator, which locks on every allocation and would inflate the timings.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};
use trafilatura_rust::__internals::{HtmlInput, ParseLimits, Tree, load_html};
use trafilatura_rust::Call;
use trafilatura_rust::ExtractError;

#[allow(dead_code)]
#[path = "../common/mod.rs"]
mod common;

/// Whether the run was asked for; otherwise the target prints a notice and exits.
pub fn requested(name: &str) -> bool {
    let on = std::env::var_os("TRAFILATURA_MEASURE").is_some_and(|value| value == "1");
    if !on {
        println!("{name}: skipped; run scripts/measure.sh (or set TRAFILATURA_MEASURE=1)");
    }
    on
}

pub fn load(bytes: &[u8]) -> Result<Tree, ExtractError> {
    load_html(
        HtmlInput::Bytes(bytes),
        &ParseLimits::default(),
        &Call::default(),
    )
}

/// Calls `measure` on each corpus page whose source contains `$TRAFILATURA_MEASURE_ONLY`, then
/// writes `.temp/measure/<version>-rust-<kind>.json`.
pub fn run(kind: &str, extra: Value, measure: impl Fn(&[u8]) -> Value) {
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
            let mut page = measure(&bytes);
            page["id"] = entry["id"].clone();
            page["source"] = entry["source"].clone();
            page["input_bytes"] = json!(bytes.len());
            println!("{source}");
            page
        })
        .collect();
    let version = common::upstream_release();
    let out: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join(".temp/measure");
    std::fs::create_dir_all(&out).unwrap();
    let path = out.join(format!("{version}-rust-{kind}.json"));
    let document = json!({
        "pin": common::pin(),
        "upstream_version": version,
        "host": { "os": std::env::consts::OS, "arch": std::env::consts::ARCH, "runtime": "native" },
        "extra": extra,
        "pages": pages,
    });
    std::fs::write(&path, serde_json::to_string_pretty(&document).unwrap()).unwrap();
    println!("wrote {} pages to {}", pages.len(), path.display());
}
