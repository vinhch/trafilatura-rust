# Dependencies

Why each dependency in `Cargo.toml` is there. The policy that admits one is in `AGENTS.md`
(Dependency policy). A row is added with the dependency and removed with it.

| Crate | Kind | Replaces upstream | Why this crate |
| --- | --- | --- | --- |
| `html5ever` | always | `lxml.html` parsing | Servo's WHATWG parser, widely used; feeds the crate's own tree through `TreeSink` (D-2) |
| `encoding_rs` | always | Python codecs | The WHATWG encodings, maintained by Firefox's encoding owner |
| `chardetng` | always | `charset_normalizer` | Firefox's detector, same maintainer as `encoding_rs` (D-3) |
| `regex` | always, from Phase 2 | `re` | rust-lang regex; lookaround and backreferences are written by hand (Gotchas) |
| `flate2` | feature `decompress` | `gzip`, `zlib` | rust-lang; pure-Rust `miniz_oxide` backend (D-4) |
| `brotli-decompressor` | feature `decompress` | `brotli` | Decompress-only, forbids `unsafe` by default (D-4) |
| `ruzstd` | feature `decompress` | `zstd` | Pure Rust, no C build (D-4) |
| `tokio` (`rt`) | feature `tokio` | none | `spawn_blocking` for `extract_async` |
| `dhat` | dev | `tracemalloc` | Peak heap and allocation count per page for `scripts/measure`; dev-only, so stable and widely used is enough (Dependency policy) |
| `serde_json` | dev | none | Reads parity fixtures in tests; never a normal dependency |
