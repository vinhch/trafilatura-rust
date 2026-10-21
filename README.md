# trafilatura-rust

A Rust port of [trafilatura](https://github.com/adbar/trafilatura), the Python library that extracts
the main text, comments and metadata of web pages. The goal is the same output as upstream Python at
the pinned release, with a synchronous, cancellable, bounded and panic-free API that is safe to call
from other languages.

Status: scaffold. No extraction code yet; `docs/phase-plan.md` has the roadmap.

Scope: for now only upstream's fast mode (`fast=True`). The readability and jusText fallbacks that
upstream runs by default are not ported yet; they may come later. When extraction finds nothing usable
it returns an error that says why, and a caller that wants a fallback runs its own extractor on that
error.

## Why another Rust port

Other Rust ports exist, but they translate the Go port, which follows an older Python release. This
one ports Python directly:

- Upstream tracking: one Rust module per Python module, each function names its Python source, and
  `scripts/upstream.sh` lists upstream changes since the pinned release.
- Parity: output is compared with upstream Python output, not a benchmark score; intended differences
  are listed in `docs/deviations.md`.
- Production safety: cancellation, deadlines and input limits in the API, no panics, few dependencies.

Is it better? Not yet, since it extracts nothing today. The bet is that staying close to upstream
makes each Python release cheaper to follow.

## Documents

- `AGENTS.md`: porting rules, how to follow upstream releases, the traps.
- `docs/phase-plan.md`: phases, open items, followups.
- `docs/deviations.md`: every departure from upstream behavior.
- `docs/dependencies.md`: why each dependency is there.

Run every check CI runs: `scripts/test.sh` (or `scripts/test.ps1`).

## License

Apache-2.0, as upstream. See `LICENSE` and `NOTICE`.
