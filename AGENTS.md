# Agent guide

trafilatura-rust is a Rust port of the Python library [trafilatura](https://github.com/adbar/trafilatura):
main-text, comment and metadata extraction from web pages. Behavior parity with upstream Python is the
bar. A Rust function is correct when it returns what the upstream function returns for the same input,
not when it looks similar or scores well on a benchmark.

## Documents

| Document | Read it when |
| --- | --- |
| `docs/architecture.md` | before touching module layout, the public API, cancellation, limits or performance: module map, runtime contract, planned API |
| `docs/gotchas.md` | before porting any upstream function: traps that silently change output |
| `docs/upstream-sync.md` | when syncing to a new upstream release |
| `docs/parity-harness.md` | when touching fixtures, `tools/parity/`, `tests/parity.rs`, `tests/common/` or measurement |
| `docs/dependencies.md` | before adding, changing or removing a dependency: policy and one row per dependency |
| `docs/deviations.md` | before departing from upstream, and when a `// deviation: D-n` comment appears |
| `docs/phase-plan.md` | to find what ships next, the open items (OI-n) and the followups (F-n) |
| `README.md` | for the commands (Development) |

## Upstream reference

- The pinned release is in `Cargo.toml` under `[package.metadata.upstream]` (`version`, `commit`). That
  table is the only place it is stated: never repeat the version in code, comments or docs.
- Upstream checkout: `../trafilatura` beside this repository, or the path in `TRAFILATURA_UPSTREAM`.
  Read upstream code at the pinned commit (`git -C <checkout> show <commit>:trafilatura/core.py`),
  not at its working tree, which may be ahead.
- Upstream Python is the only reference for behavior. Other ports can suggest ideas (Other ports as
  prior art), but their output is never evidence of upstream behavior.

## Porting rules

- Port behavior function by function. Keep upstream's order of steps: the extraction pipeline mutates
  one tree, so the order of operations is behavior.
- Put each port in the Rust module `docs/architecture.md` maps to the upstream file: one Rust module
  per upstream file, so a diff of one Python file points at one Rust file.
- Names follow Rust style, not upstream spelling: rename where it reads better (`xmltotxt` becomes
  `xml_to_text`), drop the leading `_` of private helpers (visibility says private), `HtmlParser`
  not `HTMLParser`, a suffix for a Rust keyword (`ref_`). The trace comment below is the link to
  upstream, so a rename never loses it.
- Give each ported function one trace comment naming its source:
  `// upstream: main_extractor.py::handle_paragraphs`. No line numbers; they drift every release.
- Record a departure from upstream, deliberate or forced, as a row in `docs/deviations.md` with an id,
  and put `// deviation: D-n` at the spot in the code. An unrecorded difference is a bug.
- Port upstream bugs as they are, unless a row in `docs/deviations.md` says otherwise. Parity comes
  first; a fix belongs upstream.
- A named upstream constant keeps its upstream name and lives in the Rust module of the upstream file
  that defines it (`ESCALATION_MAX_LENGTH` in `extraction.rs`); `settings.cfg` values go in
  `settings.rs`. Never inline one at its use.
- A call into a module a later phase ports stays in place: it calls a stub with the upstream
  signature that returns the neutral value (no duplicate, no metadata), marked
  `// todo(phase N): <module>`. Tests that depend on it are `#[ignore = "phase N"]`. Never delete the
  call site to make a test pass.
- Exceptions. An upstream `raise` that upstream itself catches becomes an `Err` that reaches the same
  handler (the `raise ValueError` rejections in `bare_extraction` become typed rejections). An
  exception that escapes upstream becomes a typed error, never a panic. Arithmetic and indexing where
  Python would raise (`ZeroDivisionError`, `IndexError`) or where Rust would overflow use checked
  forms.
- Fast mode only, for now. The port covers upstream's `fast=True` path; `fast=False` is not ported
  yet (deviation D-1, followup F-6). Until F-6 lands, this crate leaves the readability and jusText
  fallback to the caller, who gets a typed rejection and can run a fallback of their own.
  - `Options` has no `fast` field while only fast mode exists.
  - At each upstream `if not options.fast` (or `if options.fast`), port the fast branch only and mark
    the spot `// deviation: D-1`, so F-6 finds every place the slow branch goes.
  - An expression that reads `options.fast` as a value (`keep_teasers=options.fast` in
    `main_extractor.py`, the `fast` argument `core.py` passes to `extract_metadata`) takes `true`.
  - An upstream test that runs with the default `fast=False` is ported with the fast-mode result.
    When that differs from the upstream assertion, the test asserts the fast-mode output from
    upstream Python and says so in a comment citing D-1.

## Other ports as prior art

Other ports of trafilatura exist, and reading them can save time: how they model the tree, handle an
edge case, structure tests or make a hot path fast. Treat them as ideas to evaluate, not as a
reference. Many follow an older upstream release, carry changes of their own or are no longer
maintained; most translate the Go port rather than the Python library.

- Implement from upstream Python. An idea from another port is a starting point; the code that lands
  is written for this crate, from the upstream function, in this crate's style and structure.
- Verify before relying on an idea: compare it with the upstream function at the pinned commit (same
  branches, constants and order). A difference is fixed, or recorded in `docs/deviations.md`.
- Cite upstream only. Trace comments, docs, tests and commit messages point at the upstream function
  (`// upstream: file.py::fn`), the one place a reader checks behavior; other ports are not cited as
  a source, so the reference never splits.
- Agreement with another port's output is not parity; parity is upstream Python output only.

## Runtime rules

These hold for the whole crate; `docs/architecture.md` (Runtime contract) has the details.

- Synchronous and CPU-bound: no `async` in the pipeline.
- Cancellable: every extraction entry point takes a `Call`. Check it at stage boundaries and inside
  every loop over nodes, about every `CHECK_INTERVAL` nodes; a ported function that checks it returns
  `Result`; a check never changes a branch.
- Bounded: `MAX_FILE_SIZE` and `MAX_TREE_SIZE` as upstream applies them, nesting depth 256, and the
  added `max_nodes` limit (D-5). Recursion only where `MAX_DEPTH` bounds its depth, with a test that
  proves it fits a 512 KiB stack in a debug build, so a default thread stack is enough (a musl host
  that spawns its own threads sets 512 KiB).
- No hidden clock: `max_date` becomes a per-call option (planned), never read from the clock when the
  extractor is built.
- Panic-free on any input. `unwrap`, `expect` and `panic` fail clippy outside tests; `string_slice`
  fails everywhere, tests included; `indexing_slicing` warns and CI denies warnings. A shared helper
  under `tests/` starts with
  `#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]`.
  Use checked arithmetic and never let two `RefCell` borrows overlap.
- Every `pub` item has a doc comment (`missing_docs`, denied by CI).
- No process-global mutable state: caller-owned values instead. `unsafe` is forbidden.
- Dependencies: follow the policy in `docs/dependencies.md`.
- Performance: measure first, never change output, keep upstream's function boundaries
  (`docs/architecture.md`, Performance and memory).

## Tests

The upstream `tests/<name>_tests.py` of each module in the module map is its specification; tests of
out-of-scope modules are not ported. It becomes `tests/<name>_tests.rs`, case by case, keeping each
test name (`test_extract_with_metadata` becomes `fn test_extract_with_metadata`), so a changed upstream
test points at one Rust test.

## Commands

`README.md` (Development) lists the scripts; `scripts/test.sh` (or `.ps1`) runs every check CI runs.
Scripts come in `.sh` and `.ps1` pairs with the same arguments: change both together.

## Commits

Conventional commits (`feat:`, `fix:`, `docs:`, `ci:`, `chore:`). A port of an upstream change names
the upstream release or commit in the body. A performance change states its measured numbers in the
body.
