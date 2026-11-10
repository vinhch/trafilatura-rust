# Development

The contributor reference: what to install, the scripts that run the checks, what CI runs and how a
release is cut. Porting rules are in `AGENTS.md`.

## Prerequisites

| Tool | Needed by |
| --- | --- |
| Rust toolchain at the `rust-version` in `Cargo.toml` | everything |
| cargo-nextest, cargo-deny | `scripts/test.sh` (`--no-deny` skips cargo-deny) |
| PowerShell 7.2 or later | every `.ps1` script; Windows PowerShell 5.1 refuses them |
| uv | `oracle`, `fixtures --generate` and `measure` on Linux; `regression` everywhere |
| Docker | `oracle`, `fixtures --generate` and `measure` off Linux (or with `--docker`), and `fuzz.ps1` on an x86_64 host |
| cargo-fuzz and the `x86_64-unknown-linux-gnuasan` target | `fuzz.sh` (Linux x86_64) |
| `cargo install --locked cargo-mutants@27.1.0` | mutation testing |
| an upstream checkout at the pinned commit | `fixtures --generate`, `oracle`, `measure`, `regression`, `upstream`: `../trafilatura` or `TRAFILATURA_UPSTREAM` |

The test scripts need network access: the package step, `cargo publish --dry-run`, reads the crates.io
index.

## Scripts

Scripts come in `.sh` and `.ps1` pairs with the same arguments; `--help` on each lists its flags.

| Command | Does |
| --- | --- |
| `scripts/test.sh` | every check the CI test job runs: version check, the pin check (`scripts/pin-check.sh`), the size check (`scripts/size-check.sh`), fmt, clippy `-D warnings` (all features and targets, default features, no default features), tests under `cargo nextest` (the parity test when fixtures exist) plus the doc tests (the README examples included), the batch example (`examples/batch`), the stack tests again in the dev profile (a debug build), the package verify step (`cargo publish --dry-run`, which builds the library from the files `include` in `Cargo.toml` selects, without uploading), `cargo deny check`. `--report` runs nextest with its `ci` profile, which writes `target/nextest/ci/junit.xml`; `--no-deny` skips `cargo deny` |
| `cargo mutants -j 4 --output .work/mutants` | mutation testing of the modules in `examine_globs` with the settings in `.cargo/mutants.toml`; needs the environment in its header; rerun each timeout alone with `-j 1`, since under load it can hide a survivor; CI runs it sharded |
| `scripts/fixtures.sh` | no flags: unpacks the tracked parity fixture archive into `.temp/fixtures/<version>`; `--generate` regenerates the fixtures from upstream, `--pack` writes the archive, `--only SUBSTRING` keeps matching pages, `--docker` runs in Docker even on Linux, `--upstream PATH` names the checkout (`docs/parity-harness.md`) |
| `scripts/fuzz.sh` | runs one fuzz target (`parse_document`, `extract`) for N seconds (default 60) with the libFuzzer limits; `--tmin <target> <artifact>` minimizes a crash file, `--cmin <target>` minimizes the corpus. `.ps1` runs it in a container built from `scripts/fuzz.Dockerfile` |
| `scripts/measure.sh` | time and heap per page for crate and upstream, into `.temp/measure/<version>.json`; run by hand; off Linux both sides run in one container built from `scripts/measure.Dockerfile`; `--docker`, `--only SUBSTRING`, `--upstream PATH`; `--threads-native` runs only the thread-scaling target, natively (`docs/parity-harness.md`) |
| `scripts/oracle.sh` | runs one Python snippet against upstream at the pin and prints its stdout, for expected values of ported tests (`docs/parity-harness.md`) |
| `scripts/pin-check.sh` | fails on a dependency requirement in `Cargo.toml` (or the manifest given) that starts with `=`; run by `scripts/test.sh` after its case test `scripts/pin-check-test.sh` |
| `scripts/regression.sh` | extraction heap and bench times of the sample, `--base REF` (default `main`) against the working tree, with the heap gate; `--upstream PATH`; what the `bench.yml` job runs (`docs/parity-harness.md`) |
| `scripts/size-check.sh` | fails on a hand-written `.rs` file over the size guide in `AGENTS.md` without a `size:` line, or over 1.5 times it; skips generated files; run by `scripts/test.sh` after its fixture test `scripts/size-check-test.sh` |
| `scripts/upstream.sh` | upstream changes since the pin (`--to REF`, `--patch`) |
| `scripts/version.sh` | prints the crate version; `--check`, `--tag vX.Y.Z` |

## CI

The workflows live in `.github/workflows/`. The CI test job calls `scripts/test.sh --report`, so CI and
a local run execute the same checks. Which job runs the parity test: `docs/parity-harness.md`.

- `ci.yml` calls `_ci.yml` on pushes to `main` and pull requests targeting `main`. The test job runs on
  Ubuntu and Windows with the toolchain pinned to `rust-version`; it publishes the nextest results with
  `dorny/test-reporter` and uploads the JUnit file as the artifact `test-report-<os>`. The
  `minimal-versions` job runs on Ubuntu with a nightly pinned by date and tests each dependency at its
  lowest allowed version (`docs/dependencies.md`).
- `release.yml` runs on version tags; see Releasing.
- `quality.yml` runs weekly and on demand: it re-checks advisories, runs each fuzz target for 15
  minutes with AddressSanitizer and minimizes the corpus cache, and runs `cargo-mutants` on the modules
  in `examine_globs` in 17 shards that fail on a surviving mutant not listed in `.cargo/mutants.toml`.
- `bench.yml` runs `scripts/regression.sh` on pull requests targeting `main` and reports the heap and
  time change of the sample against the base in the job summary; it is not a required check.
- `upstream-watch.yml` runs weekly and opens an issue when upstream tags a release newer than the pin
  (`docs/upstream-sync.md`).

Actions are pinned by commit SHA and Dependabot moves them, except `dtolnay/rust-toolchain`, which
moves by hand with `rust-version` and the nightly date (OI-6 in `docs/phase-plan.md`).

## Releasing

1. Set `version` in `Cargo.toml` and run `scripts/version.sh --check`.
2. Commit `chore: release X.Y.Z` on `main` and push `main`.
3. Push the tag `vX.Y.Z` on that commit.

`release.yml` runs on tags matching `v[0-9]*.[0-9]*.[0-9]*`. Its guard checks that the tagged commit is
on `origin/main` and that the tag names the version in `Cargo.toml`; then it runs `_ci.yml` and
publishes to crates.io through trusted publishing in the `crates-io` environment, which accepts
deployments only from `v*` tags and waits for a required reviewer.

After the publish, the `github-release` job creates the GitHub Release of the tag with generated
notes, marked a pre-release when the tag has a `-` suffix. If it fails after the publish succeeded,
use "Re-run failed jobs": re-running all jobs asks for the environment approval again, and
`cargo publish` then fails because the version already exists.
