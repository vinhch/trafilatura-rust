# Phase plan

This plan is the durable record of what trafilatura-rust ships next and the items still open. It
holds current state only; history is in git. `AGENTS.md` holds the porting rules and the runtime
contract every phase works under; this plan does not restate them.

How the plan is kept:

- A phase runs its own brainstorm and design gate before code; nothing here pre-decides its design.
  A design spec, once approved, goes under `docs/specs/` and the phase row links it.
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
| 1 | Core design: tree model with text and tail, public API (options, results, errors), cancellation and deadline handle, resource limits, module layout, first dependency choices | Next | Design spec approved in `docs/specs/`; `docs/dependencies.md` lists each chosen crate with its reason | OI-1, OI-2 |
| 2 | Parity harness: a script that runs upstream Python at the pinned commit over a page corpus and writes fixtures; a Rust test that compares crate output with them | Not started | `scripts/test.sh` runs the comparison; fixture generation is reproducible from the pin | 1, OI-3 |
| 3 | Extraction core: `settings`, `utils`, `htmlprocessing`, `xpaths`, `main_extractor`, `baseline`, `core` with TXT, Markdown and HTML output | Not started | Upstream extraction tests ported by name pass; parity fixtures for main text match or have a deviation row | 2 |
| 4 | Metadata: `metadata`, `json_metadata`, and the `htmldate` and `courlan` parts they call | Not started | Upstream metadata tests ported by name pass; metadata fixtures match or have a deviation row | 3 |
| 5 | Deduplication: `deduplication` (the fallbacks are followup F-6, D-1) | Not started | Upstream dedup tests ported by name pass | 3 |
| 6 | Output formats: `xml` (XML, XML-TEI, JSON, CSV), Markdown escaping parity | Not started | Upstream `xml_tei_tests.py` cases ported by name pass | 3 |
| 7 | First release: lift `publish = false`, set the version, publish 0.1.0 through `release.yml`, move the .NET binding onto this crate | Not started | Crate on crates.io; the .NET binding builds and passes its tests against it | 3, 4, 5 |

### Shipped phases

| Phase | Shipped in |
| --- | --- |
| 0. Scaffold: Apache-2.0 license and NOTICE, crate skeleton with panic and unsafe lints, `cargo-deny` policy, `.sh`/`.ps1` scripts, CI, release and quality workflows, Dependabot, `AGENTS.md` | the first commit after `c2404f3` |

## Open items register

| Id | Item | Needed by |
| --- | --- | --- |
| OI-1 | Tree and parser choice. Upstream parses with libxml2 (`lxml.html`); an html5 parser builds a different tree for malformed markup. Decide the parser crate and the tree type, and measure how many parity fixtures differ from parser behavior alone. | Phase 1 |
| OI-2 | Encoding detection. Upstream decodes bytes with `charset_normalizer`. Decide between porting its used parts and a maintained crate, judged on parity fixtures. | Phase 1 |
| OI-3 | Parity corpus. Choose pages that can be committed (license) or fetched from a pinned source, covering upstream's `tests/resources` and `tests/eval` cases. | Phase 2 |
| OI-4 | Language check. Upstream's `target_language` uses the optional `py3langid`. Decide whether the port includes it, behind a feature, and with which model. | Phase 4 |
| OI-5 | Open upstream changes to watch on each sync: the proposed switch from `lxml.html` to another parser, the Markdown and XML output fixes, and the concurrent `extract()` crash report. A merged one changes Phases 3 and 6. | Each sync |
| OI-6 | The CI toolchain pin (`dtolnay/rust-toolchain`, 1.98.1) and `rust-version` must move together; Dependabot cannot move the action because its ref is a branch tip. | Each toolchain bump |

## Followups

Work worth doing after the phases it depends on, not scheduled yet.

| Id | Followup | After |
| --- | --- | --- |
| F-1 | Generate the selectors from upstream `xpaths.py`: parse its token tuples and expressions at build time (or in a checked-in generator) so a changed upstream selector shows as a diff, not a manual translation. | Phase 3 |
| F-2 | Fuzzing: `cargo-fuzz` targets for parse plus extract, asserting no panic, bounded time and bounded memory on arbitrary bytes. | Phase 3 |
| F-3 | Benchmarks: a criterion suite over the parity corpus, with a recorded baseline and a CI job that reports regressions. | Phase 3 |
| F-4 | Mutation testing with `cargo-mutants` in the weekly quality workflow. | Phase 3 |
| F-5 | A scheduled upstream watch: a workflow that runs `scripts/upstream.sh` weekly and opens an issue when a new upstream tag appears. | Phase 7 |
| F-6 | `fast=False` (closes D-1): port `external.py` and `readability_lxml.py`, add the `fast` option, fill every `// deviation: D-1` branch, regenerate parity fixtures in both modes. Decide between porting the used parts of `justext` and a maintained jusText crate, judged on parity fixtures and the dependency policy. Likely behind a cargo feature. | Phase 7 |
