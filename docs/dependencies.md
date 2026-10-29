# Dependencies

The policy that admits a dependency, and why each dependency in `Cargo.toml` is there. Read it before
adding, changing or removing one. A row is added with the dependency and removed with it.

## Policy

- Fewest dependencies that do the job. Write a small piece (a few hundred lines) before taking a crate
  for it. Two pieces are written here and have no row: JSON parsing (`pyjson`) and NFC
  normalization (`pyunicodedata`, over tables generated from the oracle's Python); the crates differ
  from CPython in output, see `gotchas.md`, "json and unicodedata".
- A Rust crate named after a Python package is not that package. Where upstream calls `htmldate` or
  `courlan`, port the logic it uses; take a crate only for infrastructure with no upstream logic in it
  (HTML parsing, regex, encoding tables and detection).
- Take a crate only when it is actively maintained (a release in the last 12 months and issues being
  answered), widely used or backed by an organization, and under a license `deny.toml` allows.
  Dev-only crates (tests, benchmarks, measurement) never reach users: stable and widely used is
  enough, without a recent release.
- One version of each crate in `Cargo.lock`: `cargo deny check` fails on duplicates, advisories,
  unmaintained crates, unknown sources and unknown licenses.
- Each dependency has a row in the table below: what it replaces upstream and why this crate.
  No exact `=` pins and no git dependencies; Dependabot moves versions weekly.
- Optional functionality goes behind a cargo feature so the default build stays small.
- The `fuzz/` crate has its own `Cargo.lock`, outside `cargo deny` and its license allow list (libFuzzer
is NCSA), moved by its own Dependabot entry, and never shipped to users.

## Table

| Crate | Kind | Replaces upstream | Why this crate |
| --- | --- | --- | --- |
| `html5ever` | always | `lxml.html` parsing | Servo's WHATWG parser, widely used; feeds the crate's own tree through `TreeSink` (D-2) |
| `encoding_rs` | always | Python codecs | The WHATWG encodings, maintained by Firefox's encoding owner |
| `chardetng` | always | `charset_normalizer` | Firefox's detector, same maintainer as `encoding_rs` (D-3) |
| `regex` | always | `re` | rust-lang regex; lookaround and backreferences are written by hand (`gotchas.md`) |
| `web_atoms` | always | none | Servo, shared with `html5ever`; supplies the entity table for `pyhtml::unescape` (`pyurl` calls it too), which `entity_table_matches_python` checks |
| `flate2` | feature `decompress` | `gzip`, `zlib` | rust-lang; pure-Rust `miniz_oxide` backend (D-4) |
| `brotli-decompressor` | feature `decompress` | `brotli` | Decompress-only, forbids `unsafe` by default (D-4) |
| `ruzstd` | feature `decompress` | `zstd` | Pure Rust, no C build (D-4) |
| `dhat` | dev | `tracemalloc` | Peak heap and allocation count per page for `scripts/measure`; dev-only, so stable and widely used is enough (Policy) |
| `serde_json` | dev | none | Reads parity fixtures in tests; never a normal dependency |
| `similar` | dev | none | Myers line diff for the parity check's per-hunk classes; dev-only, so stable and widely used is enough (Policy) |
| `libfuzzer-sys` | fuzz (`fuzz/` crate) | none | The cargo-fuzz runtime; lives in its own crate and lock, never reaches users; bundles libFuzzer (NCSA) |
