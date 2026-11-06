//! Exports the upstream version from `[package.metadata.upstream]` in `Cargo.toml` as the
//! `TRAFILATURA_UPSTREAM_VERSION` environment variable, so the TEI header states it without a
//! second copy in the source.

use std::fs;
use std::path::PathBuf;

const TABLE: &str = "[package.metadata.upstream]";

fn main() {
    let manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let manifest_path = manifest_dir.join("Cargo.toml");
    println!("cargo:rerun-if-changed=Cargo.toml");
    let manifest = match fs::read_to_string(&manifest_path) {
        Ok(text) => text,
        Err(error) => fail(&format!("cannot read {}: {error}", manifest_path.display())),
    };
    match upstream_version(&manifest) {
        Some(version) => println!("cargo:rustc-env=TRAFILATURA_UPSTREAM_VERSION={version}"),
        None => fail(&format!("no `version` in {TABLE} of Cargo.toml")),
    }
}

/// The `version` value inside the `[package.metadata.upstream]` table, without its quotes.
fn upstream_version(manifest: &str) -> Option<String> {
    let mut in_table = false;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_table = line == TABLE;
        } else if in_table
            && let Some(value) = line
                .strip_prefix("version")
                .and_then(|rest| rest.trim_start().strip_prefix('='))
        {
            return Some(value.trim().trim_matches('"').to_owned());
        }
    }
    None
}

/// Stops the build with `message`.
fn fail(message: &str) -> ! {
    eprintln!("build.rs: {message}");
    std::process::exit(1)
}
