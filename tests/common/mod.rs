#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Helpers shared by the integration tests: where the upstream checkout and the fixtures live.

use std::path::{Path, PathBuf};

fn manifest_dir() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// The upstream commit `Cargo.toml` pins in `[package.metadata.upstream]`.
pub fn pin() -> String {
    let manifest = std::fs::read_to_string(manifest_dir().join("Cargo.toml")).unwrap();
    manifest
        .lines()
        .find_map(|line| line.strip_prefix("commit = "))
        .map(|value| value.trim().trim_matches('"').to_string())
        .expect("Cargo.toml has no `commit = ` line under [package.metadata.upstream]")
}

/// The upstream checkout: `$TRAFILATURA_UPSTREAM`, else `../trafilatura`; `None` when it is absent.
pub fn upstream_root() -> Option<PathBuf> {
    let root = match std::env::var_os("TRAFILATURA_UPSTREAM") {
        Some(path) => manifest_dir().join(path),
        None => manifest_dir().join("../trafilatura"),
    };
    root.join("tests").is_dir().then_some(root)
}

/// The fixture set for the pin: `$TRAFILATURA_FIXTURES`, else `.temp/fixtures/<pin>`; `None` when it
/// has no `index.json`.
pub fn fixture_dir() -> Option<PathBuf> {
    let dir = match std::env::var_os("TRAFILATURA_FIXTURES") {
        Some(path) => manifest_dir().join(path),
        None => manifest_dir().join(".temp/fixtures").join(pin()),
    };
    dir.join("index.json").is_file().then_some(dir)
}

/// The directory the tests write their reports to.
pub fn report_dir() -> PathBuf {
    manifest_dir().join(".temp/parity")
}
