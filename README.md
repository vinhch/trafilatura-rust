# trafilatura-rust

A Rust port of [trafilatura](https://github.com/adbar/trafilatura), the Python library that extracts
the main text, comments and metadata of web pages. The goal is the same output as upstream Python at
the pinned release, with a synchronous, cancellable, bounded and panic-free API that is safe to call
from other languages.

Status: the extraction pipeline of upstream's fast mode works through the public API (`Options`,
`Extractor`, `html2txt`): main text and comments from bytes or a string, as text, Markdown or HTML, or as
the `Document` of `bare_extraction`. Not yet there: metadata extraction, deduplication, XML, XML-TEI, JSON
and CSV output, and `fast=False`. The crate is not published. Extraction is checked by upstream's unit and
filter tests ported by name, by cases generated from upstream Python and by comparing the text, Markdown
and HTML output of the corpus pages with upstream's (`docs/phase-plan.md` has the roadmap).

Scope: for now only upstream's fast mode (`fast=True`). The readability and jusText fallbacks that
upstream runs by default are not ported yet; they may come later. When extraction finds nothing usable
it returns an error that says why, and a caller that wants a fallback runs its own extractor on that
error.

## Why another Rust port

Other Rust ports exist, but most translate the Go port, which follows an older Python release. This
one ports Python directly:

- Upstream tracking: one Rust module per Python module, each function names its Python source, and
  `scripts/upstream.sh` lists upstream changes since the pinned release.
- Parity: output is compared with upstream Python output, not a benchmark score; intended differences
  are listed in `docs/deviations.md`.
- Production safety: cancellation, deadlines and input limits in the API, no panics, few dependencies.

Is it better? `scripts/measure` times `extract` per page on both sides and prints the Rust / Python
comparison (`docs/parity-harness.md`); the numbers depend on the machine, so run it on yours.
The bet is that staying close to upstream makes each Python release cheaper to follow.

## Documents

- `docs/architecture.md`: module map, runtime contract, public API, performance rules.
- `docs/gotchas.md`: traps found reading upstream.
- `docs/upstream-sync.md`: how to move to a new upstream release.
- `docs/parity-harness.md`: fixtures, mismatch classes, measurement.
- `docs/dependencies.md`: the dependency policy and why each dependency is there.
- `docs/deviations.md`: every departure from upstream behavior.

## Development

| Command | Does |
| --- | --- |
| `scripts/test.sh` / `scripts/test.ps1` | every check the CI test job runs: version check, fmt, clippy `-D warnings` (all features and targets, default features, no default features), tests (the parity test when fixtures exist), `cargo deny check` |
| `scripts/fixtures.sh` / `scripts/fixtures.ps1` | no flags: unpacks the tracked parity fixture archive into `.temp/fixtures/<version>`; `--generate` regenerates the fixtures from upstream, `--pack` writes the archive (`docs/parity-harness.md`) |
| `scripts/fuzz.sh` / `scripts/fuzz.ps1` | runs one fuzz target (`parse_document`, `extract`) for N seconds (default 60) with the libFuzzer limits, or with `--cmin <target>` minimizes that target's corpus; `.sh` needs cargo-fuzz and the `x86_64-unknown-linux-gnuasan` target (Linux x86_64), `.ps1` runs it in a container built from `scripts/fuzz.Dockerfile` (the CI toolchain, cargo-fuzz and the target baked in) and needs an x86_64 Docker host |
| `scripts/measure.sh` / `scripts/measure.ps1` | time and heap per page for crate and upstream, into `.temp/measure/<version>.json`; run by hand; off Linux both sides run in one container built from `scripts/measure.Dockerfile` (`docs/parity-harness.md`) |
| `scripts/oracle.sh` / `scripts/oracle.ps1` | runs one Python snippet against upstream at the pin and prints its stdout, for expected values of ported tests (`docs/parity-harness.md`) |
| `scripts/regression.sh` / `scripts/regression.ps1` | extraction heap and bench times of the sample, `--base REF` (default `main`) against the working tree, with the heap gate; what the `bench.yml` job runs (`docs/parity-harness.md`) |
| `scripts/upstream.sh` / `scripts/upstream.ps1` | upstream changes since the pin (`--to REF`, `--patch`) |
| `scripts/version.sh` / `scripts/version.ps1` | prints the crate version; `--check`, `--tag vX.Y.Z` |

CI (`.github/workflows/`): `ci.yml` calls `_ci.yml` on pushes to `main` and pull requests targeting
`main`, on Ubuntu and Windows with the toolchain pinned to `rust-version`. `release.yml` checks that
the tag names the crate version and is on `main`, runs `_ci.yml`, and publishes to crates.io through
the `crates-io` environment. `quality.yml` re-checks advisories and runs each fuzz target for 15
minutes with AddressSanitizer and minimizes the corpus cache, weekly and on demand. `bench.yml` runs
`scripts/regression.sh` on pull requests targeting `main` and reports the heap and time change of the
sample against the base in the job summary; it is not a required check. Actions are pinned by commit
SHA and Dependabot moves them, except `dtolnay/rust-toolchain`, which moves by hand with
`rust-version` (OI-6 in `docs/phase-plan.md`). The CI test job calls `scripts/test.sh`, so CI and a
local run execute the same checks. Which job runs the parity test: `docs/parity-harness.md`.

## License

Apache-2.0, as upstream. See `LICENSE` and `NOTICE`.
