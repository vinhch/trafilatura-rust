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

Phase numbers are ids, not positions: the Order column gives the sequence, and Blocked by names phase ids.

| Order | Phase | Status | Done when | Blocked by |
| --- | --- | --- | --- | --- |
| 1 | 9. Harness and benchmarks: a criterion suite over the parity corpus (time, peak heap, allocations) and a CI job that reports regressions; upstream's `extract()` timed on the corpus with the fixture options (`fast=True`, `dedup=False`) and the crate in the same container, since only `load_html` has a Python number today; mutation testing with `cargo-mutants` in the weekly quality workflow. Start with the survivors in `main_extractor.rs`; known untested branches are the `extract_content` recovery gate on an empty result versus a short one, `strip_elements` with tails, and a non-`div` frame whose descendants are all inline. | Not started | The criterion job and the weekly mutants run in CI; the measurement output holds a Python extraction time per corpus page | none |
| 2 | 4. Metadata: `metadata`, `json_metadata`, and the `htmldate` and `courlan` parts they call; with metadata on, an option to keep the `script` text Phase 8 drops for extraction, since htmldate runs its date patterns over the serialized tree, scripts included | Not started | Upstream metadata tests ported by name pass; metadata fixtures match or have a deviation row | 9 |
| 3 | 5. Deduplication: `deduplication` (the fallbacks are Phase 11, D-1) | Not started | Upstream dedup tests ported by name pass | 9 |
| 4 | 6. Output formats: `xml` (XML, XML-TEI, JSON, CSV), on an XML backend to decide: upstream `xml.py` serializes with `lxml.etree` (`control_xml_output` round-trips through `XMLParser(remove_blank_text=True)` and `tostring(pretty_print=True)`; `validate_tei` checks a DTD). `Tree::tostring` (`dom/serialize.rs`) already writes lxml's XML method with `pretty_print` and `with_tail`, checked against lxml by the generated output cases; left are the XML parse of the `remove_blank_text` round trip and the DTD check (`xmloxide` or hand-written), judged on byte-exact match with the parity fixtures and on the dependency policy; `is_padding_cell` and `merge_with_parent`, quadratic today, become linear before the cleaning loop calls them | Not started | Upstream `xml_tei_tests.py` cases ported by name pass; the XML branch of `determine_returnstring` (the empty-element cleaning loop and `control_xml_output`) replaces its stub | 9 |
| 5 | 7. First release: lift `publish = false`, set the version, publish 0.1.0 through `release.yml`, move the .NET binding onto this crate. Before publishing: `scripts/test.sh` and `.ps1` fail on a `=` version requirement in `Cargo.toml`, which `cargo deny` does not check (`docs/dependencies.md`, Policy); a CI job runs `cargo +nightly update -Z direct-minimal-versions` and the tests, so each declared minimum builds once the crate resolves by ranges, with the rule added to the Policy; the module-level `allow(dead_code)` on `context`, `courlan`, `deduplication`, `dom`, `html_processing`, `main_extractor`, `pyhtml`, `pystr`, `pyurl`, `settings`, `utils`, `xml` and `xpaths` narrowed to what stays unused. For the binding, which reads content and comments as text and as HTML separately (`contentText`, `commentsText`, `contentHtml`, `commentsHtml`): a `Document` method that converts one of its trees with `convert_to_html` and serializes it, since the HTML of one tree alone is not an upstream output | Not started | Crate on crates.io; the .NET binding builds and passes its tests against it; no module-level `allow(dead_code)` left; the minimal-versions job passes | 4, 5 |
| 6 | 10. Parser and encoding parity (D-2, D-3): a tree builder that follows libxml2's `HTMLparser.c` rules (`htmlStartClose`, `htmlEndPriority`, no implied `tbody`, no adoption agency) for the rules behind measured mismatches only, with `xmloxide` (MIT, pure Rust, modelled on `HTMLparser.c`) as a candidate dependency or reference, checked against those tables first; the builder keeps the sink rules of Phase 8 (text moved into `Tree`, `script` and `style` text dropped, the attribute bounds). A detector that follows charset_normalizer in place of the `chardetng` guess: candidates `charset-norm` and `charset-normalizer-core` (both MIT, young, single author; the second claims behavior-identical to charset_normalizer 3.5.x), checked against its mess and coherence logic on non-UTF-8 fixtures. Upstream on libxml2 and on html5lib differ in `txt` on 33 of 1,044 corpus pages (18 by more than 2%) and in metadata on 34; encoding guesses differ on 5 | Not started | Corpus outputs compared with the `lxml` fixture variant, each remaining difference explained by a deviation row | 7 |
| 7 | 11. Slow mode (closes D-1): port `external.py` and `readability_lxml.py`, add the `fast` option, fill every `// deviation: D-1` branch, regenerate parity fixtures in both modes; port the used parts of `justext` or use a maintained jusText crate, judged on parity fixtures and the dependency policy; likely behind a cargo feature | Optional, not planned | Corpus parity in both modes | 7 |

### Shipped phases

| Phase | Shipped in |
| --- | --- |
| 0. Scaffold: Apache-2.0 license and NOTICE, crate skeleton with panic and unsafe lints, `cargo-deny` policy, `.sh`/`.ps1` scripts, CI, release and quality workflows, Dependabot, `AGENTS.md` | `eac2d8d` |
| 1. Core design: tree model, public API, cancellation, limits, module layout, first dependencies (`AGENTS.md`, D-2..D-4) | `521051e` |
| 2. Parity harness: `tools/parity/` fixture generator, `scripts/fixtures` and `scripts/measure`, the parity test and report, `dom/` (arena tree, html5ever parse, depth and node limits), `utils::decode_file`, `error.rs`, `cancel.rs` | `35a3e8a` |
| 3. Extraction core: `settings`, `utils`, `htmlprocessing`, `xpaths`, `main_extractor`, `baseline`, `core`, the `xml` tree helpers, TXT, Markdown (with its escaping) and HTML output, the public API (`Options`, `Extractor`, `Input`, `Document`, `html2txt`), corpus parity on `txt`, `markdown` and `html`, and the extraction measurements | `b42ba5a` |
| 8. Hardening, input limits and parse memory: bounds every input-driven cost, lowers the parse and extraction peak and takes the measured hot-path wins, all without changing output | `accc975` |

## Open items register

| Id | Item | Needed by |
| --- | --- | --- |
| OI-4 | Language check. Upstream's `target_language` uses the optional `py3langid`. Decide whether the port includes it, behind a feature, and with which model. | Phase 4 |
| OI-5 | Open upstream changes to watch on each sync: the proposed switch from `lxml.html` to another parser, the Markdown and XML output fixes, and the concurrent `extract()` crash report. A merged one changes Phases 3 and 6; the parser switch also changes the sink rules of Phase 8 and Phase 10. | Each sync |
| OI-6 | The CI toolchain pins (`dtolnay/rust-toolchain`, 1.98.1, in `_ci.yml`, `quality.yml` (the fuzz job) and `release.yml`), the base image in `scripts/fuzz.Dockerfile` and `rust-version` must move together; Dependabot cannot move the action because its ref is a branch tip. | Each toolchain bump |

## Followups

Work worth doing after the phases it depends on, not scheduled yet.

| Id | Followup | After |
| --- | --- | --- |
| F-5 | A scheduled upstream watch: a workflow that runs `scripts/upstream.sh` weekly and opens an issue when a new upstream tag appears; the issue starts an agent-run sync that follows `docs/upstream-sync.md`. | Phase 7 |
| F-14 | `dom/`: replace the walk to the root per placement and the subtree walk on reparent with stored depth if measurement shows parse time above twice upstream. | Phase 9 |
| F-16 | `prune_xpath` (D-7): an XPath engine, or a caller-supplied predicate in place of the XPath strings. | After Phase 7 |
| F-25 | Shrink the trees if a measurement of an element-dense page under the node limit shows its peak needs it: `Node` (128 bytes) with links as `Option<NonZeroU32>` and `attrs` as a boxed slice; `SinkNode` (120 bytes, also spent on every text node) with the element fields apart from text nodes; text shared between a tree and its `Tree::copy` (copies are about 10% of `extract` time). `text` and `tail` stay growable strings: the ported code appends to them, and a reallocation per append would make that quadratic. | A measured need |
| F-26 | `html2txt_tree` serves the short-extraction check only as a character count, yet copies and cleans the whole tree on about half the corpus pages (200 to 300 ms over the corpus): a read-only count that skips what `basic_cleaning` removes, checked against the oracle. | Phase 9 |
| F-27 | `normalize_nfc`: one table for the quick check in place of three binary searches per character above U+0300 (about 3x on non-Latin output). | Phase 9 |
| F-28 | URLs: split the base once per extraction instead of per link in `fix_relative_urls` and `urljoin`, and let `urlsplit` borrow when nothing is removed (about 3 µs per relative link with `links` or `images` on). | Phase 9 |
| F-29 | html5ever's own per-token costs that no budget charges and that only a caller's deadline bounds at the input cap (`D-5`): a start-tag-free tail of `</x>` end tags behind a deep stack of open elements (64 MiB behind about 250 open elements takes about 28 s: each end tag walks the open elements, bounded by the nesting depth of 256) and 64 MiB of 3-byte `<!>` bogus comments (about 8 s: 22 million comments, a `call.check` every 8 KiB piece). Measure them against the deadline contract of the entry points, then either charge them or document the deadline as the bound. | Phase 9 |
| F-30 | The deduplication cache must start the script-text rerun of `bare_extraction` as it was before the first run: the first run already called `duplicate_test`, so a rerun that reuses the cache would reject its own page as a duplicate. | Phase 5 |
| F-31 | Slow mode reads the raw tree (readability, jusText), so it runs with script text kept unless each reader is shown to delete `script` and `style` before it reads text. Metadata needs no change: the Phase 4 row already plans keeping script text with metadata on. | Phase 11 |
