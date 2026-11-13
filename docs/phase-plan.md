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
| 5 | 14. Release hygiene: F-71, F-76, F-81, F-84 | Not started | Each listed followup closed or rewritten | none |
| 6 | 15. Test hardening: F-65, F-75, F-78, F-82, F-83, F-85, F-88 | Not started | Each listed followup closed or rewritten | none |
| 7 | 16. Performance pass 2: F-25, F-26, F-27, F-28, F-48, F-59, F-60, F-62, F-63, F-74, F-77, F-79 | Not started | Each listed followup closed or rewritten | none |
| 8 | 11. Slow mode (closes D-1): port `external.py` and `readability_lxml.py`, add the `fast` option, fill every `// deviation: D-1` branch, regenerate parity fixtures in both modes; port the used parts of `justext` or use a maintained jusText crate, judged on parity fixtures and the dependency policy; likely behind a cargo feature | Optional, not planned | Corpus parity in both modes | none |

### Shipped phases

| Phase | Shipped in |
| --- | --- |
| 0. Scaffold: Apache-2.0 license and NOTICE, crate skeleton with panic and unsafe lints, `cargo-deny` policy, `.sh`/`.ps1` scripts, CI, release and quality workflows, Dependabot, `AGENTS.md` | `eac2d8d` |
| 1. Core design: tree model, public API, cancellation, limits, module layout, first dependencies (`AGENTS.md`, D-2..D-4) | `521051e` |
| 2. Parity harness: `tools/parity/` fixture generator, `scripts/fixtures` and `scripts/measure`, the parity test and report, `dom/` (arena tree, html5ever parse, depth and node limits), `utils::decode_file`, `error.rs`, `cancel.rs` | `35a3e8a` |
| 3. Extraction core: `settings`, `utils`, `htmlprocessing`, `xpaths`, `main_extractor`, `baseline`, `core`, the `xml` tree helpers, TXT, Markdown (with its escaping) and HTML output, the public API (`Options`, `Extractor`, `Input`, `Document`, `html2txt`), corpus parity on `txt`, `markdown` and `html`, and the extraction measurements | `b42ba5a` |
| 4. Metadata: `metadata`, `json_metadata`, and the `htmldate` and `courlan` parts they call; the `keep_script_text` option keeps the `script` and `style` text the parse drops when metadata is off (D-14), since htmldate runs its date patterns over the serialized tree | `5e72b16` |
| 5. Deduplication: `deduplication` with `content_fingerprint`, `sample_tokens`, `Simhash` (`validate`, `similarity`), `generate_bow_hash`, `duplicate_test` and the `DedupCache` on `Call`; deviations D-22 to D-26 | `723b5e5` |
| 6. Output formats: `xml` (XML, XML-TEI, JSON, CSV): `control_xml_output` with the `remove_blank_text` reparse as a tree pass, `build_tei_output` and `check_tei`, `build_json_output`, `xml_to_csv`, checked against upstream output by the corpus and the generated output cases; `validate_tei`, the TEI DTD check, is not ported (D-27, F-69) | `0cdd91c..d6ca7c5` |
| 7. First release: 0.1.0 on crates.io through `release.yml` on trusted publishing (`rust-lang/crates-io-auth-action`, no token secret), with a GitHub Release with generated notes per tag (`github-release` job). 7a, crate cleanup: the module-level `allow(dead_code)` narrowed, F-45, F-49, F-57, F-64 and F-70 closed, F-71 investigated and recorded, still open (`fbd598b..9b6234e`). 7b, packaging, CI and publish: the `include` list checked by `cargo package`, the check against `=` requirements in `scripts/test.sh` and `.ps1`, the minimal-versions job in `_ci.yml`, F-5, `cargo publish --dry-run` in the release verify (`33588bb..95c89e2`). 7c, the binding handoff `rust-trafilatura-dotnet/.work/trafilatura-rust-migration-handoff.md`: the binding moves to 0.1.0 with its options, errors and cancellation mapped onto `Options`, `Call` and `ExtractError`; a `CancellationToken` reaches native code as `Call::cancel` in place of the time budget, its worker cap and runaway workers; the crate gets no HTML method on `Document` (comments HTML is F-80); the consumer `content-search-connectors` threads its engine token through and drops its worker cap code. The migration, the new `RustTrafilatura` version and the consumer change run in those repos | `fbd598b..35f7bd5` and the 7c handoff |
| 8. Hardening, input limits and parse memory: bounds every input-driven cost, lowers the parse and extraction peak and takes the measured hot-path wins, all without changing output | `accc975` |
| 9. Harness and benchmarks: per-line classification of parity differences, generator hygiene for the fixtures, measurement with the `bench.yml` regression job (`scripts/regression.sh`), and mutation testing: a weekly `cargo-mutants` job in `quality.yml` over `main_extractor.rs`, `metadata.rs`, `htmldate/validators.rs` and `htmldate/core.rs`, sharded 20 ways, with the one equivalent mutant listed in `.cargo/mutants.toml` | `5725382` |
| 10. Parser and encoding parity (D-2, D-3 closed), against lxml 6.1.3 on Linux (libxml2 2.14.6). 10a, the lxml comparator: tree, outputs and metadata compared with the `lxml` fixture variant, per-page classified report, F-72. 10b, the libxml2 tree builder on `html5gum`, replacing html5ever: `HTMLparser.c` rules from generated tables, the `lxml` variant as the gating oracle, the `html5` variant dropped. 10c, `detect_encoding` and `decode_file` on `charset-norm` behind a parity gate against charset_normalizer 3.5.2 (22 synthetic inputs and the 27 non-UTF-8 corpus pages identical), `chardetng` and `encoding_rs` removed, every one of the 1,044 pages gated with no port-bug left, and the parser followups F-86, F-89 and F-91 | 10a `b85a5a6`, 10b `c23e957`, 10c `fe1f48c..28f5aab` |
| 12. File size and test speed: a size guide for hand-written `.rs` files enforced by `scripts/size-check`, module directories for the production files over it, inline test modules moved out and large test files split, tests built at `opt-level = 1` with the stack proofs in a dev-profile step | `9723a84` |
| 13. Resource use: `Extractor::extract_batch` on scoped workers with its how-to and example, the measurement method (output digests on two option sets, dhat allocation shares, a native threads run), one fix (`char_len` counts characters without building the text), the resource rules in `AGENTS.md` and `docs/architecture.md`, and the clippy lints: four perf lints at warn, and `format_collect` left off | `354c12a..0cdd91c` |

## Open items register

| Id | Item | Needed by |
| --- | --- | --- |
| OI-4 | Language check. Upstream's `target_language` uses the optional `py3langid`. Decide whether the port includes it, behind a feature, and with which model. | After Phase 7: the parity lock has no `py3langid` and the fixtures carry `language` null, so the stubs give upstream's output |
| OI-5 | Open upstream changes to watch on each sync: the proposed switch from `lxml.html` to another parser, the Markdown and XML output fixes, and the concurrent `extract()` crash report. A merged one changes Phases 3 and 6; the parser switch also changes the parse limits of Phase 8 and the libxml2 rules of Phase 10. lxml wheels on libxml2 2.15 make `source` and `track` void, which changes 83 corpus pages (measured on 2.15.3); regenerate the fixtures and re-run the gate when upstream's lxml moves to such a wheel. | Each sync |
| OI-6 | The CI toolchain pins (`dtolnay/rust-toolchain`, 1.98.1, in `_ci.yml`, `bench.yml`, `quality.yml` (the fuzz and mutants jobs) and `release.yml`), the nightly date of the `minimal-versions` job in `_ci.yml` (its `dtolnay/rust-toolchain` pin is the master commit, which takes a `toolchain` input), the base image in `scripts/fuzz.Dockerfile` and `scripts/measure.Dockerfile` and `rust-version` must move together; Dependabot cannot move the action because its ref is a branch tip. After a bump, repeat the base-against-head check of `scripts/regression` with a base that differs from head in `src/` only: the gated peak must move, because cargo's freshness rules decide whether head reuses a base build. | Each toolchain bump |

## Followups

Work worth doing after the phases it depends on, not scheduled yet.

| Id | Followup | After |
| --- | --- | --- |
| F-16 | `prune_xpath` (D-7): an XPath engine, or a caller-supplied predicate in place of the XPath strings. | After Phase 7 |
| F-25 | Shrink the trees if a measurement of an element-dense page under the node limit shows its peak needs it: `Node` (128 bytes) with links as `Option<NonZeroU32>` and `attrs` as a boxed slice; text shared between a tree and its `Tree::copy` (copies are about 10% of `extract` time). `text` and `tail` stay growable strings: the ported code appends to them, and a reallocation per append would make that quadratic. | A measured need |
| F-26 | `html2txt_tree` serves the short-extraction check only as a character count, yet copies and cleans the whole tree on about half the corpus pages (200 to 300 ms over the corpus): a read-only count that skips what `basic_cleaning` removes, checked against the oracle. Needs an upstream-shaped design: a count that skips the copy and cleaning steps reshapes a ported function (`AGENTS.md`, porting rules). | none |
| F-27 | `normalize_nfc`: one table for the quick check in place of three binary searches per character above U+0300 (about 3x on non-Latin output). | none |
| F-28 | URLs: split the base once per extraction instead of per link in `fix_relative_urls` and `urljoin`, and let `urlsplit` borrow when nothing is removed (about 3 µs per relative link with `links` or `images` on). | none |
| F-29 | Runs that yield no token until they end have no deadline check inside them; their measured times are in `D-5` (`docs/deviations.md`). Either charge the no-token runs (a byte-count check inside the tokenizer's text and attribute reads, which needs a wrapper over html5gum) or document the input cap as their bound. | none |
| F-31 | Slow mode reads the raw tree (readability, jusText), so it runs with script text kept unless each reader is shown to delete `script` and `style` before it reads text. Metadata needs no change: with metadata extracted the parse keeps the script text (D-14), and `keep_script_text(true)` restores upstream's parse for the other calls. | Phase 11 |
| F-48 | Performance, unmeasured: `format!` per escaped attribute byte in `dom/serialize.rs`, and `examine_elements` calls `text_content` per candidate (up to 1,000) over the full text. Measure with date candidates nested under a multi-megabyte text node before changing either. | A measured need |
| F-59 | DOM text storage: the parser builds one `String` per text node (`src/dom/parse/builder.rs`, `add_text`, through `Tree::add_text_owned`). Measured in Phase 13, before 10b, at about 46% of the load set's allocation blocks, 18% of TXT and 14% of rich-html. Changing it changes the `Node` layout every ported function reads, so it is a design, not a local fix. | A measured need |
| F-60 | Arena compaction: the tree's node vector (`src/dom/mod.rs`, `Tree::new`, filled by `Tree::add_child` from `src/dom/parse/builder.rs`) holds most of a page's peak. Phase 13 found that a site's share of one page's peak does not carry to the summed peak (9.9% of one page moved the summed TXT peak 0.19%), so measure the summed peak before any design. | A measured need |
| F-62 | Attribute sharing (`src/dom/parse/emitter.rs`, `commit_attr`): the values are already `Arc<str>`, and the per-node attribute `Vec` is a node-layout change in F-25's area. | none |
| F-63 | Clones and trim below Phase 13's threshold: `trim` allocates its owned result (`src/utils/text.rs`, about 3.6% of TXT allocation blocks, a signature change across callers), text and tail clones in `src/xml.rs`, `src/main_extractor/content.rs` and `src/main_extractor.rs` (1.0% to 2.1%), and `joined_text` (1.3% of one page's peak). | none |
| F-65 | The thread-scaling target (`tests/measure_threads.rs`) has no untimed warm-up before its 1-thread count, so the ratios read slightly high. Add a warm-up pass and re-measure the threads figure. | A measured need |
| F-68 | Keep the digest baseline: `.superpowers/sdd/phase10c/baseline` (untracked, from commit `28f5aab`) is the reference for `--check-digests` in later phases. Regenerate it from that commit if it is lost. | none |
| F-69 | TEI DTD check (D-27): port `validate_tei` behind a cargo feature once a pure-Rust validator loads `tei_corpus.dtd` (parameter entities), or hand-write one. The `tei_validation` field of `Extractor` comes back with it. | After Phase 7 |
| F-71 | The native Windows build and the Docker build differed on `tests/resources/webpage.html.gz` in the txt, markdown and html digests. Not reproduced at commit bd96f79. Native Windows (rustc 1.99.0 and 1.98.1, with and without `decompress`) and the measure container (rustc 1.98.1, with and without `decompress`) give identical txt, markdown and html output for `tests/resources/webpage.html.gz`, byte-compared or by Blake2b digest where the container bytes were not kept. The corpus digests (1044 pages, 18 synthetic) are equal between native rustc 1.98.1 and the container, without `decompress`: `scripts/measure.sh` builds `measure_time` without that feature, so they cover the raw gzip bytes, not a decompressed page. Ruled out: CRLF and any other input difference (both sides read the same file, length compared, no checksum), and the rustc version (1.99.0 against 1.98.1). Reopen with the Windows and container output of one commit, byte-diffed. | A reproduced difference |
| F-74 | `text_bodies` (`src/xml/tei.rs`) walks every descendant of its root and filters by tag and parent. `iterdescendants` with a tag list filters after the same walk, so a tag filter does not shorten it. The cost is not measured. | A measured need |
| F-75 | The 17 fast-mode oracle values that the tests cite (`run` in `tools/parity/parity/oracle_fast_unit.py`) are checked only when someone runs them. Add a periodic re-run of all 17. | none |
| F-76 | The measure build compiles `measure_time` with `--features __internals` only (`scripts/measure.sh` line 128, `scripts/measure.ps1` line 123), without `decompress`. The corpus digests and the `tests/resources/webpage.html.gz` digests therefore cover the raw gzip bytes, not the decompressed page. Add `decompress` to both scripts and measure the before side again. | Before F-71 is reopened |
| F-77 | Metadata-on extraction costs more after F-70: the `inline_script_20m_meta` single run takes 483 ms against 133 ms before, about 3.6 times, and the rich Markdown peak heap rose 17%. Profile the parse of the kept script text to find whether the cost drops without changing output. | A measured need |
| F-78 | The probe-window rows of `htmldate` (F-46 b) check that the crate and the oracle agree on each input, not the window size. A window of another size that returns the same first match passes. Add rows where the match sits just outside the window, or a test on the window bounds. | none |
| F-79 | The `pystr::title` sweep table (`src/pystr/title.rs`, generated, 6,966 rows) is 722 KB. Ranges that share one result shape would take a fraction of the size. | A measured need |
| F-80 | Comments as HTML, an extension upstream lacks (upstream's `build_html_output` reads the main text only): `convert_to_html` over the comments tree, opt-in. No new `Format` variant, since `Format` mirrors upstream's `SUPPORTED_FORMATS`, and `Format::Html` output stays as it is. Reopens Phase 7's ruling of no HTML method on `Document`. Its deviation row comes with the code. | A consumer asks |
| F-81 | The brotli exclusion of D-29 (`docs/deviations.md`) is measured with CPython brotli 1.2.1 only. With the locked `brotli-decompressor`, the raw `Quick read <html>...` text does not decode at any cap (Task 4 probe; no test feeds it). `src/utils/tests/decompress.rs` covers a hand-built brotli stream, not that text. Re-measure the reason with the crate's decoder, then state the result in the D-29 row. | Before the next release |
| F-82 | The script-text rerun guard (`src/extraction.rs`, around line 579) and its recursive call are not observed by the stack tests, which assert only `needs_script_text` (`assert_rerun_taken` in `src/api/tests/stack.rs`). Add an observable for the rerun, such as a counter under `__internals`, so a mutation of the guard fails a test. | The recall path changes |
| F-83 | `assert_rerun_taken` (`src/api/tests/stack.rs`) builds its options from `Format::Txt` for every format. The recall setup is shared today; if it varies by format, the assertion stops covering the format it was set for. Build the options per format. | The recall frame varies by format |
| F-84 | The `JsonSize` branch of `to_u32` (`src/pyjson.rs`) runs only on 64-bit hosts; on a 32-bit target the conversion cannot fail. Check that CI runs a 64-bit target only and decide whether a test that does not depend on `usize` width is needed. | none |
| F-85 | Remove `Ns` once no code builds a non-`Html` element: the parser builds none since Phase 10b, and removing it touches every hand-built test tree. | none |
| F-87 | `subtree` with a context under `document_roots[1..]` walks only its own tree, while lxml walks the whole document. No caller reaches it today (`src/xpaths/select.rs`). | A caller reaches it |
| F-88 | The formatting attack test (`src/dom/parse/tests/limits.rs`) has a 2 s bound and can exceed it when the suite runs in parallel on a loaded Windows CI runner. Decide on a per-test serial override or a CI-side measure. | none |
| F-90 | A literal `<` in text that does not start a tag, when the element on top is `frameset`, is placed differently from libxml2 (deviation D-30, `docs/deviations.md`). Port the char-data split in the emitter to close it, if wanted. | none |
