# trafilatura-rust

A Rust port of [trafilatura](https://github.com/adbar/trafilatura), the Python library that extracts
the main text, comments and metadata of web pages. The goal is the same output as upstream Python at
the pinned release, with a synchronous, cancellable, bounded and panic-free API that is safe to call
from other languages.

Status: phases 0 to 2 are shipped: input decoding, an html5ever parse into an arena tree with depth and
node limits, and the parity harness that compares the parsed tree with upstream. There is no extraction
yet; `docs/phase-plan.md` has the roadmap.

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

Is it better? Not yet, since it extracts nothing today. The bet is that staying close to upstream
makes each Python release cheaper to follow.

## Documents

- `AGENTS.md`: the brief for agents and contributors: porting rules and the index of the pages below.
- `docs/architecture.md`: module map, runtime contract, planned public API, performance rules.
- `docs/gotchas.md`: traps found reading upstream.
- `docs/upstream-sync.md`: how to move to a new upstream release.
- `docs/parity-harness.md`: fixtures, mismatch classes, measurement.
- `docs/dependencies.md`: the dependency policy and why each dependency is there.
- `docs/deviations.md`: every departure from upstream behavior.
- `docs/phase-plan.md`: phases, open items, followups.

## Development

| Command | Does |
| --- | --- |
| `scripts/test.sh` / `scripts/test.ps1` | every check the CI test job runs: version check, fmt, clippy `-D warnings` (all features and targets, default features, no default features), tests (the parity test when fixtures exist), `cargo deny check` |
| `scripts/fixtures.sh` / `scripts/fixtures.ps1` | no flags: unpacks the tracked parity fixture archive into `.temp/fixtures/<version>`; `--generate` regenerates the fixtures from upstream, `--pack` writes the archive (`docs/parity-harness.md`) |
| `scripts/measure.sh` / `scripts/measure.ps1` | time and heap per page for crate and upstream, into `.temp/measure/<version>.json`; run by hand (`docs/parity-harness.md`) |
| `scripts/oracle.sh` / `scripts/oracle.ps1` | runs one Python snippet against upstream at the pin and prints its stdout, for expected values of ported tests (`docs/parity-harness.md`) |
| `scripts/upstream.sh` / `scripts/upstream.ps1` | upstream changes since the pin (`--to REF`, `--patch`) |
| `scripts/version.sh` / `scripts/version.ps1` | prints the crate version; `--check`, `--tag vX.Y.Z` |

CI (`.github/workflows/`): `ci.yml` calls `_ci.yml` on pushes to `main` and pull requests targeting
`main`, on Ubuntu and Windows with the toolchain pinned to `rust-version`. `release.yml` checks that
the tag names the crate version and is on `main`, runs `_ci.yml`, and publishes to crates.io through
the `crates-io` environment. `quality.yml` re-checks advisories weekly. Actions are pinned by commit
SHA and Dependabot moves them, except `dtolnay/rust-toolchain`, which moves by hand with
`rust-version` (OI-6 in `docs/phase-plan.md`). The CI test job calls `scripts/test.sh`, so CI and a
local run execute the same checks. Which job runs the parity test: `docs/parity-harness.md`.

## License

Apache-2.0, as upstream. See `LICENSE` and `NOTICE`.
