# Phase plan

This plan is the durable record of what trafilatura-rust ships next and the items still open. It
holds current state only; history is in git. `AGENTS.md` holds the porting rules every phase works
under and `docs/architecture.md` the runtime contract; this plan does not restate them.

How the plan is kept:

- A phase runs its own brainstorm and design gate before code; nothing here pre-decides its design.
  Design specs and implementation plans are working files under `.superpowers/` (not tracked); once a
  design is approved, the decisions that outlive the phase move into `AGENTS.md` and the pages under `docs/`.
- A row moves from Remaining to Shipped when its done-when criteria hold, citing the commit or tag.
- A register row leaves the register when it closes. Ids are never reused: `OI-n` for open items,
  `F-n` for followups.
- A citation of this plan names an id or a heading, never a line.

## Roadmap

The parity bar for every phase is upstream Python output at the pinned release with `fast=True`
(D-1), compared by the parity harness Phase 2 builds.

### Remaining phases

| Order | Phase | Status | Done when | Blocked by |
| --- | --- | --- | --- | --- |
| 3 | Extraction core: `settings`, the rest of `utils`, `htmlprocessing`, `xpaths`, `main_extractor`, `baseline`, `core`, the `xml` tree helpers, and TXT, Markdown (with its escaping) and HTML output | In progress, by sub-phase below | Upstream extraction tests ported by name pass; parity fixtures for main text match or have a deviation row | none |
| 4 | Metadata: `metadata`, `json_metadata`, and the `htmldate` and `courlan` parts they call | Not started | Upstream metadata tests ported by name pass; metadata fixtures match or have a deviation row | 3 |
| 5 | Deduplication: `deduplication` (the fallbacks are followup F-6, D-1) | Not started | Upstream dedup tests ported by name pass | 3 |
| 6 | Output formats: `xml` (XML, XML-TEI, JSON, CSV) | Not started | Upstream `xml_tei_tests.py` cases ported by name pass | 3 |
| 7 | First release: lift `publish = false`, set the version, publish 0.1.0 through `release.yml`, move the .NET binding onto this crate | Not started | Crate on crates.io; the .NET binding builds and passes its tests against it | 3, 4, 5 |

Phase 3 sub-phases. Each brings its modules to parity with unit tests before the next starts; the
corpus parity check for main text turns on in 3f. Each sub-phase after 3b gets a short design addendum
before its code.

| Sub-phase | Scope | Status |
| --- | --- | --- |
| 3a Foundations | `dom/` mutation, walks and copy in lxml semantics; `pystr`; `settings.rs`; the text helpers of `utils.rs`; the `xml.py` tree helpers | done |
| 3b Selectors | `xpaths.rs` (content and comment selectors), `BASIC_CLEAN_XPATH` | next |
| 3c `html_processing.rs` | tree cleaning, pruning, link density, `convert_tags`; the `courlan` URL parts it calls | not started |
| 3d `main_extractor.rs` | the element handlers, tables, lists, `extract_content`, `extract_comments` | not started |
| 3e Output | TXT and Markdown (with its escaping) and HTML output, `merge_with_parent`, `strip_double_tags` | not started |
| 3f `baseline.rs`, `extraction.rs`, public API | `baseline`, `html2txt`, the cascade, `bare_extraction`, `extract`, `Options`, `Extractor`, `Input`, `Document`; corpus parity on `txt`, `markdown`, `html`; memory measurement of the arena copies | not started |

### Shipped phases

| Phase | Shipped in |
| --- | --- |
| 0. Scaffold: Apache-2.0 license and NOTICE, crate skeleton with panic and unsafe lints, `cargo-deny` policy, `.sh`/`.ps1` scripts, CI, release and quality workflows, Dependabot, `AGENTS.md` | `eac2d8d` |
| 1. Core design: tree model, public API, cancellation, limits, module layout, first dependencies (`AGENTS.md`, D-2..D-4) | `521051e` |
| 2. Parity harness: `tools/parity/` fixture generator, `scripts/fixtures` and `scripts/measure`, the parity test and report, `dom/` (arena tree, html5ever parse, depth and node limits), `utils::decode_file`, `error.rs`, `cancel.rs` | `35a3e8a` |

## Open items register

| Id | Item | Needed by |
| --- | --- | --- |
| OI-4 | Language check. Upstream's `target_language` uses the optional `py3langid`. Decide whether the port includes it, behind a feature, and with which model. | Phase 4 |
| OI-5 | Open upstream changes to watch on each sync: the proposed switch from `lxml.html` to another parser, the Markdown and XML output fixes, and the concurrent `extract()` crash report. A merged one changes Phases 3 and 6. | Each sync |
| OI-6 | The CI toolchain pin (`dtolnay/rust-toolchain`, 1.98.1) and `rust-version` must move together; Dependabot cannot move the action because its ref is a branch tip. | Each toolchain bump |

## Followups

Work worth doing after the phases it depends on, not scheduled yet.

| Id | Followup | After |
| --- | --- | --- |
| F-1 | Generate the selectors from upstream `xpaths.py`: parse its token tuples and expressions at build time (or in a checked-in generator) so a changed upstream selector shows as a diff, not a manual translation. | Phase 3 |
| F-2 | Fuzzing: `cargo-fuzz` targets for parse plus extract, asserting no panic, bounded time and bounded memory on arbitrary bytes. | Phase 3 |
| F-3 | Benchmarks in CI: a criterion suite over the parity corpus covering time, peak heap and allocations, compared with the Phase 2 baseline, and a job that reports regressions. | Phase 3 |
| F-4 | Mutation testing with `cargo-mutants` in the weekly quality workflow. | Phase 3 |
| F-5 | A scheduled upstream watch: a workflow that runs `scripts/upstream.sh` weekly and opens an issue when a new upstream tag appears. | Phase 7 |
| F-6 | `fast=False` (closes D-1): port `external.py` and `readability_lxml.py`, add the `fast` option, fill every `// deviation: D-1` branch, regenerate parity fixtures in both modes. Decide between porting the used parts of `justext` and a maintained jusText crate, judged on parity fixtures and the dependency policy. Likely behind a cargo feature. | Phase 7 |
| F-7 | libxml2-style tree construction: a tree builder that follows libxml2's `HTMLparser.c` rules (`htmlStartClose`, `htmlEndPriority`, no implied `tbody`, no adoption agency) instead of HTML5 tree construction. Only if the Phase 2 harness shows parser-caused mismatches worth closing; port just the rules behind them. `xmloxide` (MIT, pure Rust, HTML parser modelled on `HTMLparser.c`) is a candidate, as a dependency or as reference; check it against those tables first. | Phase 2 |
| F-8 | charset_normalizer parity for non-UTF-8 input: replace the `chardetng` guess with a detector that follows upstream's charset_normalizer, if the Phase 2 harness shows encoding-caused mismatches worth closing. Candidates: `charset-norm` and `charset-normalizer-core` (both MIT, young, single author; the second claims behavior-identical to charset_normalizer 3.5.x). Check each against charset_normalizer's mess and coherence logic and on non-UTF-8 fixtures first. | Phase 2 |
| F-9 | Enforce the no-exact-pin rule mechanically: `scripts/test.sh` and `.ps1` fail when `Cargo.toml` declares a `=` version requirement, which `cargo deny` does not check (`docs/dependencies.md`, Policy). | Phase 2 |
| F-10 | Minimal-versions check: a CI job runs `cargo +nightly update -Z direct-minimal-versions` and the tests, so each declared minimum really builds; add the rule to the Policy in `docs/dependencies.md`. Matters once the crate is published and resolved by ranges instead of this repo's `Cargo.lock`. | Phase 2, before Phase 7 |
| F-12 | Tighten parity classification: a page in class `encoding`, `normalized` or `python-parser` is excluded whole today; classify each differing line and fail on any line no class explains. | Phase 3 |
| F-13 | Harness hardening: restore logging and warnings state after the generator tests; write fixture pages and index atomically (temp directory, then rename); parametrize the identity test over pin, upstream version, `max_date` and lock hash. | Phase 3 |
| F-14 | `dom/`: replace the walk to the root per placement and the subtree walk on reparent with stored depth if measurement shows parse time above twice upstream; narrow the `allow(dead_code)` on `context`, `dom`, `pystr`, `settings`, `utils` and `xml` as callers land. | Phase 3 |
| F-15 | XML output backend for Phase 6: upstream `xml.py` serializes with `lxml.etree` (`control_xml_output` round-trips through `XMLParser(remove_blank_text=True)` and `tostring(pretty_print=True)`; `validate_tei` checks a DTD). Choose between a hand-written serializer over the arena tree, `quick-xml`'s writer, and `xmloxide` (serializer and DTD validation, already a candidate for F-7). Not checked yet: whether `quick-xml` or `xmloxide` indents exactly as lxml's `pretty_print` (whitespace around text and tails). Judge on byte-exact match with the parity fixtures and on the dependency policy. | Phase 6 |
