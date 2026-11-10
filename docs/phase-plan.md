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
| 3 | 7. First release: publish 0.1.0 of this crate to crates.io through `release.yml` and hand the move of the .NET binding (`rust-trafilatura-dotnet`, NuGet `RustTrafilatura`) onto it to the binding repo; run as the sub-phases below | Not started | Crate on crates.io, `release.yml` on trusted publishing with the token secret removed; the binding handoff in `rust-trafilatura-dotnet/.work/`; no module-level `allow(dead_code)` left; the minimal-versions job passes | none |
| 4 | 10. Parser and encoding parity (D-2, D-3): a tree builder that follows libxml2's `HTMLparser.c` rules (`htmlStartClose`, `htmlEndPriority`, no implied `tbody`, no adoption agency) for the rules behind measured mismatches only, with `xmloxide` (MIT, pure Rust, modelled on `HTMLparser.c`) as a candidate dependency or reference, checked against those tables first; the builder keeps the sink rules of Phase 8 (text moved into `Tree`, `script` and `style` text dropped, the attribute bounds). A detector that follows charset_normalizer in place of the `chardetng` guess: candidates `charset-norm` and `charset-normalizer-core` (both MIT, young, single author; the second claims behavior-identical to charset_normalizer 3.5.x), checked against its mess and coherence logic on non-UTF-8 fixtures. Upstream on libxml2 and on html5lib differ in `txt` on 33 of 1,044 corpus pages (18 by more than 2%) and in metadata on 34; encoding guesses differ on 5 | Not started | Corpus outputs compared with the `lxml` fixture variant, each remaining difference explained by a deviation row | 7 |
| 5 | 11. Slow mode (closes D-1): port `external.py` and `readability_lxml.py`, add the `fast` option, fill every `// deviation: D-1` branch, regenerate parity fixtures in both modes; port the used parts of `justext` or use a maintained jusText crate, judged on parity fixtures and the dependency policy; likely behind a cargo feature | Optional, not planned | Corpus parity in both modes | 7 |

Phase 7 runs as these sub-phases, in this order; each is one review unit with its own design gate. The choices named below are decided; each gate designs the rest.

| Sub-phase | Scope |
| --- | --- |
| 7a. Crate cleanup | The 20 module-level `allow(dead_code)` in `src/lib.rs` narrowed to what stays unused, measured by a compile first; the stale comment above the `dom` allow goes. F-57, F-64 (sites pinned by clippy), F-45, F-49. F-70: the script-text drop keyed on the resolved `with_metadata`. F-71: the Windows and Docker digests compared on the same input, the cause fixed or recorded |
| 7b. Packaging, CI and publish | An `include` list in `Cargo.toml` (`src/`, `build.rs`, `README.md`, `LICENSE`, `NOTICE`), checked by `cargo package` with its verify step, so the fixture archive (24 MB, over crates.io's 10 MB cap) and the repo tooling stay out. `scripts/test.sh` and `.ps1` fail on a `=` requirement in `Cargo.toml`. A minimal-versions job in `_ci.yml` (`cargo minimal-versions test --direct --all-features` on a nightly pinned by date, added to the OI-6 pins), so each declared minimum, optional and dev dependencies included, builds and passes the tests, with the rule in the Policy of `docs/dependencies.md`. F-5. Then publish: `publish = false` lifted, the version set to 0.1.0 and `cargo publish --dry-run` added to the release verify in the same change (the dry run fails while `publish = false` stands); the first publish through `release.yml` with the `CARGO_REGISTRY_TOKEN` secret (crates.io needs a token for a crate's first publish), then trusted publishing configured, `release.yml` moved to `rust-lang/crates-io-auth-action` and the secret removed |
| 7c. Binding handoff | A handoff in `rust-trafilatura-dotnet/.work/` for a session in that repo, which moves the binding from `rust-trafilatura` to this crate's 0.1.0 from crates.io; an API gap it finds ships here as 0.1.1. The handoff maps the binding's options, errors and time budget onto `Options`, `Call` and `ExtractError`. Breaking changes are fine (one consumer) and the package name stays. Dropped: `contentHtml`, `commentsHtml` (no upstream output for comments, no consumer), `enableFallback` (D-1), `pruneSelector`, `inputEncoding`, `deduplicate`, and the worker cap with its runaway workers: `TimeBudgetMs` becomes a `Call` deadline that stops the extraction. The crate gets no HTML method on `Document`. The handoff also names the follow-on change in the consumer `content-search-connectors`: its `WorkerCapExceeded` branch and runaway tests go. The migration, the new `RustTrafilatura` version and the consumer change run in those repos, not in this phase |

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
| 8. Hardening, input limits and parse memory: bounds every input-driven cost, lowers the parse and extraction peak and takes the measured hot-path wins, all without changing output | `accc975` |
| 9. Harness and benchmarks: per-line classification of parity differences, generator hygiene for the fixtures, measurement with the `bench.yml` regression job (`scripts/regression.sh`), and mutation testing: a weekly `cargo-mutants` job in `quality.yml` over `main_extractor.rs`, `metadata.rs`, `htmldate/validators.rs` and `htmldate/core.rs`, sharded 20 ways, with the one equivalent mutant listed in `.cargo/mutants.toml` | `5725382` |
| 12. File size and test speed: a size guide for hand-written `.rs` files enforced by `scripts/size-check`, module directories for the production files over it, inline test modules moved out and large test files split, tests built at `opt-level = 1` with the stack proofs in a dev-profile step | `9723a84` |
| 13. Resource use: `Extractor::extract_batch` on scoped workers with its how-to and example, the measurement method (output digests on two option sets, dhat allocation shares, a native threads run), one fix (`char_len` counts characters without building the text), the resource rules in `AGENTS.md` and `docs/architecture.md`, and the clippy lints: four perf lints at warn, and `format_collect` left off | `354c12a..0cdd91c` |

## Open items register

| Id | Item | Needed by |
| --- | --- | --- |
| OI-4 | Language check. Upstream's `target_language` uses the optional `py3langid`. Decide whether the port includes it, behind a feature, and with which model. | After Phase 7: the parity lock has no `py3langid` and the fixtures carry `language` null, so the stubs give upstream's output |
| OI-5 | Open upstream changes to watch on each sync: the proposed switch from `lxml.html` to another parser, the Markdown and XML output fixes, and the concurrent `extract()` crash report. A merged one changes Phases 3 and 6; the parser switch also changes the sink rules of Phase 8 and Phase 10. | Each sync |
| OI-6 | The CI toolchain pins (`dtolnay/rust-toolchain`, 1.98.1, in `_ci.yml`, `bench.yml`, `quality.yml` (the fuzz and mutants jobs) and `release.yml`), the base image in `scripts/fuzz.Dockerfile` and `scripts/measure.Dockerfile` and `rust-version` must move together; Dependabot cannot move the action because its ref is a branch tip. After a bump, repeat the base-against-head check of `scripts/regression` with a base that differs from head in `src/` only: the gated peak must move, because cargo's freshness rules decide whether head reuses a base build. | Each toolchain bump |

## Followups

Work worth doing after the phases it depends on, not scheduled yet.

| Id | Followup | After |
| --- | --- | --- |
| F-5 | A scheduled upstream watch: a workflow that runs `scripts/upstream.sh` weekly and opens an issue when a new upstream tag appears; the issue starts an agent-run sync that follows `docs/upstream-sync.md`. | In Phase 7b |
| F-14 | `dom/`: replace the walk to the root per placement and the subtree walk on reparent with stored depth if the `scripts/measure` table shows `load Python / Rust` below 0.5 (parse time above twice upstream). | A measured need |
| F-16 | `prune_xpath` (D-7): an XPath engine, or a caller-supplied predicate in place of the XPath strings. | After Phase 7 |
| F-25 | Shrink the trees if a measurement of an element-dense page under the node limit shows its peak needs it: `Node` (128 bytes) with links as `Option<NonZeroU32>` and `attrs` as a boxed slice; `SinkNode` (120 bytes, also spent on every text node) with the element fields apart from text nodes; text shared between a tree and its `Tree::copy` (copies are about 10% of `extract` time). `text` and `tail` stay growable strings: the ported code appends to them, and a reallocation per append would make that quadratic. | A measured need |
| F-26 | `html2txt_tree` serves the short-extraction check only as a character count, yet copies and cleans the whole tree on about half the corpus pages (200 to 300 ms over the corpus): a read-only count that skips what `basic_cleaning` removes, checked against the oracle. Needs an upstream-shaped design: a count that skips the copy and cleaning steps reshapes a ported function (`AGENTS.md`, porting rules). | none |
| F-27 | `normalize_nfc`: one table for the quick check in place of three binary searches per character above U+0300 (about 3x on non-Latin output). | none |
| F-28 | URLs: split the base once per extraction instead of per link in `fix_relative_urls` and `urljoin`, and let `urlsplit` borrow when nothing is removed (about 3 µs per relative link with `links` or `images` on). | none |
| F-29 | html5ever's own per-token costs that no budget charges and that only a caller's deadline bounds at the input cap (`D-5`): a start-tag-free tail of `</x>` end tags behind a deep stack of open elements (64 MiB behind about 250 open elements takes about 28 s: each end tag walks the open elements, bounded by the nesting depth of 256) and 64 MiB of 3-byte `<!>` bogus comments (about 8 s: 22 million comments, a `call.check` every 8 KiB piece). Measure them against the deadline contract of the entry points, then either charge them or document the deadline as the bound. | none |
| F-31 | Slow mode reads the raw tree (readability, jusText), so it runs with script text kept unless each reader is shown to delete `script` and `style` before it reads text. Metadata needs no change: with metadata extracted the parse keeps the script text (D-14), and `keep_script_text(true)` restores upstream's parse for the other calls. | Phase 11 |
| F-48 | Performance, unmeasured: `format!` per escaped attribute byte in `dom/serialize.rs`, and `examine_elements` calls `text_content` per candidate (up to 1,000) over the full text. Measure with date candidates nested under a multi-megabyte text node before changing either. | A measured need |
| F-52 | `last_parse_normalized()` does not flag a page with `<isindex>`. html5lib rewrites the tag into a form, while html5ever and lxml keep it, so a generated table can take html5lib's tree for such a page without a warning. Check the guard and make it flag `<isindex>`, or list the page in its exception list, then regenerate the affected tables. | Phase 10 |
| F-59 | DOM text storage: the parser builds one `String` per text node (`src/dom/parse.rs`, the text insert). Measured in Phase 13 at about 46% of the load set's allocation blocks, 18% of TXT and 14% of rich-html. Changing it changes the `Node` layout every ported function reads, so it is a design, not a local fix. | A measured need |
| F-60 | Arena compaction: the tree's node vector (`src/dom/mod.rs`, `Tree::new`) and the sink's node list (`src/dom/parse.rs`, `new_node`) hold most of a page's peak. Phase 13 found that a site's share of one page's peak does not carry to the summed peak (9.9% of one page moved the summed TXT peak 0.19%), so measure the summed peak before any design. | A measured need |
| F-61 | html5ever's tokenizer and tree-builder allocations (`src/dom/parse/sink.rs`, `feed_all` and `process_token`): about 35% and 14% of the load set's allocation blocks. The allocations are the dependency's; a change means a parser change (`docs/dependencies.md`). | none |
| F-62 | Attribute sharing (`src/dom/parse/sink.rs`, the attribute map): the values are already `Arc<str>`, and the per-node attribute `Vec` is a node-layout change in F-25's area. | none |
| F-63 | Clones and trim below Phase 13's threshold: `trim` allocates its owned result (`src/utils/text.rs`, about 3.6% of TXT allocation blocks, a signature change across callers), text and tail clones in `src/xml.rs`, `src/main_extractor/content.rs` and `src/main_extractor.rs` (1.0% to 2.1%), and `joined_text` (1.3% of one page's peak). | none |
| F-65 | The thread-scaling target (`tests/measure_threads.rs`) has no untimed warm-up before its 1-thread count, so the ratios read slightly high. Add a warm-up pass and re-measure the threads figure. | A measured need |
| F-68 | Keep the digest baseline: `.superpowers/sdd/phase13/baseline` (untracked, from commit `cda6ff1`) is the reference for `--check-digests` in later phases. Regenerate it from that commit if it is lost. | none |
| F-69 | TEI DTD check (D-27): port `validate_tei` behind a cargo feature once a pure-Rust validator loads `tei_corpus.dtd` (parameter entities), or hand-write one. The `tei_validation` field of `Extractor` comes back with it. | After Phase 7 |
| F-71 | The native Windows build and the Docker build differed on `tests/resources/webpage.html.gz` in the txt, markdown and html digests. Not reproduced at commit bd96f79. Native Windows (rustc 1.99.0 and 1.98.1, with and without `decompress`) and the measure container (rustc 1.98.1, with and without `decompress`) give identical txt, markdown and html output for `tests/resources/webpage.html.gz`, byte-compared or by Blake2b digest where the container bytes were not kept. The corpus digests (1044 pages, 18 synthetic) are equal between native rustc 1.98.1 and the container, without `decompress`: `scripts/measure.sh` builds `measure_time` without that feature, so they cover the raw gzip bytes, not a decompressed page. Ruled out: CRLF and any other input difference (both sides read the same file, length compared, no checksum), and the rustc version (1.99.0 against 1.98.1). Reopen with the Windows and container output of one commit, byte-diffed. | A reproduced difference |
| F-72 | The known-metadata match in the parity classifier (`tests/parity/output.rs`) is a substring match with a four-character minimum. A whole-element match breaks 13 xmltei pages, whose citation lines carry the date as a tail. A tail-aware match is needed. | none |
| F-74 | `text_bodies` (`src/xml/tei.rs`) walks every descendant of its root and filters by tag and parent. `iterdescendants` with a tag list filters after the same walk, so a tag filter does not shorten it. The cost is not measured. | A measured need |
| F-75 | The 17 fast-mode oracle values that the tests cite (`run` in `tools/parity/parity/oracle_fast_unit.py`) are checked only when someone runs them. Add a periodic re-run of all 17. | none |
| F-76 | The measure build compiles `measure_time` with `--features __internals` only (`scripts/measure.sh` line 128, `scripts/measure.ps1` line 123), without `decompress`. The corpus digests and the `tests/resources/webpage.html.gz` digests therefore cover the raw gzip bytes, not the decompressed page. Add `decompress` to both scripts and measure the before side again. | Before F-71 is reopened |
| F-77 | Metadata-on extraction costs more after F-70: the `inline_script_20m_meta` single run takes 483 ms against 133 ms before, about 3.6 times, and the rich Markdown peak heap rose 17%. Profile the parse of the kept script text to find whether the cost drops without changing output. | A measured need |
| F-78 | The probe-window rows of `htmldate` (F-46 b) check that the crate and the oracle agree on each input, not the window size. A window of another size that returns the same first match passes. Add rows where the match sits just outside the window, or a test on the window bounds. | none |
| F-79 | The `pystr::title` sweep table (`src/pystr/title.rs`, generated, 6,966 rows) is 722 KB. Ranges that share one result shape would take a fraction of the size. | A measured need |
