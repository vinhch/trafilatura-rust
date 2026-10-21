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

/// The `commit` value inside the `[package.metadata.upstream]` table of `manifest`, if any.
fn upstream_commit(manifest: &str) -> Option<String> {
    let mut in_table = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_table = line == "[package.metadata.upstream]";
        } else if in_table && let Some(value) = line.strip_prefix("commit") {
            let value = value.trim_start().strip_prefix('=')?;
            return Some(value.trim().trim_matches('"').to_string());
        }
    }
    None
}

/// The upstream commit `Cargo.toml` pins in `[package.metadata.upstream]`.
pub fn pin() -> String {
    let manifest = std::fs::read_to_string(manifest_dir().join("Cargo.toml")).unwrap();
    upstream_commit(&manifest)
        .expect("Cargo.toml has no `commit` key under [package.metadata.upstream]")
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

#[cfg(test)]
mod tests {
    #[test]
    fn commit_is_read_from_the_upstream_table() {
        let manifest = r#"
[package]
commit = "wrong"

[package.metadata.other]
commit = "also wrong"

[package.metadata.upstream]
version = "1"
commit = "abc123"

[dependencies]
commit = "late"
"#;
        assert_eq!(super::upstream_commit(manifest).as_deref(), Some("abc123"));
        assert_eq!(
            super::upstream_commit(
                "[package]
commit = \"x\"
"
            ),
            None
        );
    }
}
