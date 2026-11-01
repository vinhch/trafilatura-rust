#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]
//! Helpers shared by the integration tests: where the upstream checkout and the fixtures live.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use trafilatura_rust::__internals::{
    HtmlInput, MAX_FILE_SIZE, ParseLimits, extract_metadata, load_html,
};
use trafilatura_rust::{Call, Document};

/// `extract_metadata` as upstream's tests call it: `html` parsed with `load_html` and default
/// limits, `date_config` `None`. `None` where `load_html` gives no tree; upstream returns an empty
/// `Document()` there, which a test cannot build.
#[allow(dead_code)] // used by the metadata test binaries; the other binaries include this module too
pub fn metadata(
    html: &str,
    default_url: Option<&str>,
    extensive: bool,
    author_blacklist: &[&str],
) -> Option<Document> {
    let call = Call::default();
    let mut tree = load_html(
        HtmlInput::Str(html),
        MAX_FILE_SIZE,
        &ParseLimits::default(),
        &call,
    )
    .ok()?;
    let root = tree.root();
    let blacklist: HashSet<String> = author_blacklist
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    Some(
        extract_metadata(
            &mut tree,
            root,
            default_url,
            None,
            extensive,
            &blacklist,
            &call,
        )
        .expect("extract_metadata fails only on cancellation or a huge character reference"),
    )
}

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

/// Whether `PARITY_REQUIRE_FIXTURES` is set and non-empty. CI sets it so a missing upstream
/// checkout or fixture set fails a test instead of skipping it.
pub fn require_fixtures() -> bool {
    std::env::var_os("PARITY_REQUIRE_FIXTURES").is_some_and(|value| !value.is_empty())
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
