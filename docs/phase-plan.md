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
| 4 | Metadata: `metadata`, `json_metadata`, and the `htmldate` and `courlan` parts they call | Not started | Upstream metadata tests ported by name pass; metadata fixtures match or have a deviation row | none |
| 5 | Deduplication: `deduplication` (the fallbacks are followup F-6, D-1) | Not started | Upstream dedup tests ported by name pass | none |
| 6 | Output formats: `xml` (XML, XML-TEI, JSON, CSV) | Not started | Upstream `xml_tei_tests.py` cases ported by name pass; the XML branch of `determine_returnstring` (the empty-element cleaning loop and `control_xml_output`) replaces its stub | none |
| 7 | First release: lift `publish = false`, set the version, publish 0.1.0 through `release.yml`, move the .NET binding onto this crate | Not started | Crate on crates.io; the .NET binding builds and passes its tests against it | 4, 5 |

### Shipped phases

| Phase | Shipped in |
| --- | --- |
| 0. Scaffold: Apache-2.0 license and NOTICE, crate skeleton with panic and unsafe lints, `cargo-deny` policy, `.sh`/`.ps1` scripts, CI, release and quality workflows, Dependabot, `AGENTS.md` | `eac2d8d` |
| 1. Core design: tree model, public API, cancellation, limits, module layout, first dependencies (`AGENTS.md`, D-2..D-4) | `521051e` |
| 2. Parity harness: `tools/parity/` fixture generator, `scripts/fixtures` and `scripts/measure`, the parity test and report, `dom/` (arena tree, html5ever parse, depth and node limits), `utils::decode_file`, `error.rs`, `cancel.rs` | `35a3e8a` |
| 3. Extraction core: `settings`, `utils`, `htmlprocessing`, `xpaths`, `main_extractor`, `baseline`, `core`, the `xml` tree helpers, TXT, Markdown (with its escaping) and HTML output, the public API (`Options`, `Extractor`, `Input`, `Document`, `html2txt`), corpus parity on `txt`, `markdown` and `html`, and the extraction measurements | `b42ba5a` |

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
| F-4 | Mutation testing with `cargo-mutants` in the weekly quality workflow. Start with the survivors in `main_extractor.rs`; known untested branches are the `extract_content` recovery gate on an empty result versus a short one, `strip_elements` with tails, and a non-`div` frame whose descendants are all inline. In the extraction layer: the generated extraction cases hold no case that expects an `Err`, and `extract_matches` lets `Ok(None)` match `Rejected(InvalidValue)`; `sequence_matches_python` has no `recall` row and no cancelled run of the whole cascade; `loads_checks_call` cannot tell a tick mid-parse from the first value's. | Phase 3 |
| F-5 | A scheduled upstream watch: a workflow that runs `scripts/upstream.sh` weekly and opens an issue when a new upstream tag appears. | Phase 7 |
| F-6 | `fast=False` (closes D-1): port `external.py` and `readability_lxml.py`, add the `fast` option, fill every `// deviation: D-1` branch, regenerate parity fixtures in both modes. Decide between porting the used parts of `justext` and a maintained jusText crate, judged on parity fixtures and the dependency policy. Likely behind a cargo feature. | Phase 7 |
| F-7 | libxml2-style tree construction: a tree builder that follows libxml2's `HTMLparser.c` rules (`htmlStartClose`, `htmlEndPriority`, no implied `tbody`, no adoption agency) instead of HTML5 tree construction. Only if the Phase 2 harness shows parser-caused mismatches worth closing; port just the rules behind them. `xmloxide` (MIT, pure Rust, HTML parser modelled on `HTMLparser.c`) is a candidate, as a dependency or as reference; check it against those tables first. | Phase 2 |
| F-8 | charset_normalizer parity for non-UTF-8 input: replace the `chardetng` guess with a detector that follows upstream's charset_normalizer, if the Phase 2 harness shows encoding-caused mismatches worth closing. Candidates: `charset-norm` and `charset-normalizer-core` (both MIT, young, single author; the second claims behavior-identical to charset_normalizer 3.5.x). Check each against charset_normalizer's mess and coherence logic and on non-UTF-8 fixtures first. | Phase 2 |
| F-9 | Enforce the no-exact-pin rule mechanically: `scripts/test.sh` and `.ps1` fail when `Cargo.toml` declares a `=` version requirement, which `cargo deny` does not check (`docs/dependencies.md`, Policy). | Phase 2 |
| F-10 | Minimal-versions check: a CI job runs `cargo +nightly update -Z direct-minimal-versions` and the tests, so each declared minimum really builds; add the rule to the Policy in `docs/dependencies.md`. Matters once the crate is published and resolved by ranges instead of this repo's `Cargo.lock`. | Phase 2, before Phase 7 |
| F-12 | Tighten parity classification: a page in class `encoding`, `python-parser` or `decompress` is excluded whole from the tree dump check, and a page in class `encoding` or `decompress` from the output check (which has no `python-parser` class); the `normalized` class excludes a page from the tree dump only (the output check names its `normalized` pages in `NORMALIZED_REJECTS` of `tests/parity.rs`, currently none). Classify each differing line and fail on any line no class explains. | Phase 3 |
| F-13 | Harness hardening: restore logging and warnings state after the generator tests; write fixture pages and index atomically (temp directory, then rename); parametrize the identity test over pin, upstream version, `max_date` and lock hash. | Phase 3 |
| F-14 | `dom/`: replace the walk to the root per placement and the subtree walk on reparent with stored depth if measurement shows parse time above twice upstream; narrow the `allow(dead_code)` on `context`, `courlan`, `deduplication`, `dom`, `html_processing`, `main_extractor`, `pyhtml`, `pystr`, `pyurl`, `settings`, `utils`, `xml` and `xpaths` as callers land. | Phase 3 |
| F-15 | XML output backend for Phase 6: upstream `xml.py` serializes with `lxml.etree` (`control_xml_output` round-trips through `XMLParser(remove_blank_text=True)` and `tostring(pretty_print=True)`; `validate_tei` checks a DTD). `Tree::tostring` (`dom/serialize.rs`) already writes lxml's XML method with `pretty_print` and `with_tail`, and the generated output cases check it against lxml. Left to decide: the XML parse of the `remove_blank_text` round trip and the DTD check (`xmloxide`, already a candidate for F-7, or hand-written). Judge on byte-exact match with the parity fixtures and on the dependency policy. | Phase 6 |
| F-16 | `prune_xpath` (D-7): an XPath engine, or a caller-supplied predicate in place of the XPath strings. | After Phase 7 |
| F-19 | Tune `MAX_OUTPUT_NODES` (D-9, `4 * DEFAULT_MAX_NODES`) from the peak-memory numbers of the fuzz targets (F-2) and benchmarks (F-3). Measured: a table of exactly 4,000,000 cells built from a 1.5 MB page peaks at 2.1 GB, about 530 bytes per cell in the whole extraction; one more row is rejected with `TreeTooLarge` at a peak of 1.1 GB. | F-2, F-3 |
| F-21 | Decide whether `xml::list_marker` carries a running index. It counts the preceding `item` siblings per item, so numbering an ordered list is quadratic; cancellation still works because `process_element` ticks per item. Upstream uses the same algorithm through `itersiblings`. Measured, with the list reaching the Markdown renderer: 50k items take 3.9 s and 200k items 148 s, 37.5 times for four times the items. The list must survive extraction (inside `article`): a bare `<ol>` in `<body>` is flattened and is linear (0.17 s and 0.74 s). | none |
| F-22 | Per-tree HTML render for the .NET binding: the binding reads the content and comments as text and as HTML, separately (`contentText`, `commentsText`, `contentHtml`, `commentsHtml`). The Python format gives the two texts; HTML of the body and of the comments body alone is not an upstream output. Add a `Document` method that converts one of its trees with `convert_to_html` and serializes it, when the binding moves onto this crate. | Phase 7 |
| F-23 | Arena growth of the cascade: the `prepare_tree` copy and its backup, the forum and escalation copies, the `baseline_tree` and `html2txt_tree` copies and the recall retry's own copies stay in the call's arena until it drops, an estimated 4N to 5N nodes for the first pass and 9N with the retry on a page of N elements. Decide whether to free or reuse them. Measured: a page of 1.1M `<p>` elements (16.5 MB, `max_nodes` raised) peaks at 1.41 GB, about 1,280 bytes per element and 85 times the input, with 4.5 GB allocated in 20.9M allocations in 5 s; on the corpus the largest extraction peak is 12.9 MB, 1.77 times that page's load peak, and no page's extraction peak exceeds 3.24 times its own load peak. | none |
| F-24 | Time upstream's `extract()` on the corpus with the fixture options (`fast=True`, `dedup=False`) and the crate in the same container, so the extraction time has a comparable reference; only `load_html` has a Python number today. | none |
