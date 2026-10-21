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

/// The value of `key` inside the `[package.metadata.upstream]` table of `manifest`, if any. The
/// package's own `version` comes earlier in the file, so the table scope matters.
fn upstream_value(manifest: &str, key: &str) -> Option<String> {
    let mut in_table = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_table = line == "[package.metadata.upstream]";
        } else if in_table
            && let Some(value) = line
                .strip_prefix(key)
                .and_then(|rest| rest.trim_start().strip_prefix('='))
        {
            return Some(value.trim().trim_matches('"').to_string());
        }
    }
    None
}

fn upstream_commit(manifest: &str) -> Option<String> {
    upstream_value(manifest, "commit")
}

fn upstream_version(manifest: &str) -> Option<String> {
    upstream_value(manifest, "version")
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

/// The upstream release `Cargo.toml` follows in `[package.metadata.upstream]`; it names the fixture
/// directory.
pub fn upstream_release() -> String {
    let manifest = std::fs::read_to_string(manifest_dir().join("Cargo.toml")).unwrap();
    upstream_version(&manifest)
        .expect("Cargo.toml has no `version` key under [package.metadata.upstream]")
}

/// The fixture set for the pinned release: `$TRAFILATURA_FIXTURES`, else `.temp/fixtures/<version>`;
/// `None` when it has no `index.json`.
pub fn fixture_dir() -> Option<PathBuf> {
    let dir = match std::env::var_os("TRAFILATURA_FIXTURES") {
        Some(path) => manifest_dir().join(path),
        None => manifest_dir()
            .join(".temp/fixtures")
            .join(upstream_release()),
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
version = "0.0.0"
commit = "wrong"

[package.metadata.other]
commit = "also wrong"

[package.metadata.upstream]
version = "1"
commits = "not the key"
commit = "abc123"

[dependencies]
commit = "late"
"#;
        assert_eq!(super::upstream_commit(manifest).as_deref(), Some("abc123"));
        assert_eq!(super::upstream_version(manifest).as_deref(), Some("1"));
        let no_table = "[package]
version = \"0.0.0\"
commit = \"x\"
";
        assert_eq!(super::upstream_commit(no_table), None);
        assert_eq!(super::upstream_version(no_table), None);
    }
}
