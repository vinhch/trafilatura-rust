# Agent guide

trafilatura-rust is a Rust port of the Python library [trafilatura](https://github.com/adbar/trafilatura):
main-text, comment and metadata extraction from web pages. Behavior parity with upstream Python is the
bar. A Rust function is correct when it returns what the upstream function returns for the same input,
not when it looks similar or scores well on a benchmark.

This file holds the porting rules, how to follow upstream, and the traps. Plans, deviations and
dependencies live under `docs/` (listed in `README.md`).

## Upstream reference

- The pinned release is in `Cargo.toml` under `[package.metadata.upstream]` (`version`, `commit`). That
  table is the only place it is stated: never repeat the version in code, comments or docs.
- Upstream checkout: `../trafilatura` beside this repository, or the path in `TRAFILATURA_UPSTREAM`.
  Read upstream code at the pinned commit (`git -C <checkout> show <commit>:trafilatura/core.py`),
  not at its working tree, which may be ahead.
- Upstream Python is the only reference for behavior. Other ports may supply technique and code
  (Borrowing from other ports), but their output is never evidence of upstream behavior.

## Borrowing from other ports

Other Rust ports of trafilatura may be borrowed from when a piece is sound and worth it: data
structures, infrastructure code, test tooling, performance techniques, ported logic.

- The repository never names another port: not in code, comments, docs, tests, fixtures or commit
  messages. Borrowed code carries no reference to where it came from.
- Most other ports translate the Go port, which follows an older Python release and has changes of
  its own. Borrowed logic is checked against the upstream Python function before it lands: same
  branches, constants and order. A difference is fixed, or recorded in `docs/deviations.md`.
- Borrowed code is written to fit this crate and reads like the rest of it: it carries the
  `// upstream: file.py::fn` comment of the function it ports, and where the logic is not obvious a
  comment explains what it does and why.
- Agreement with another port's output is not parity; parity is upstream Python output only.

## Fast mode only, for now

The port covers upstream's `fast=True` path; `fast=False` is not ported yet (deviation D-1, followup
F-6 in `docs/phase-plan.md`). Upstream's default runs readability and jusText after the main extractor
and keeps the better result; until F-6 lands, this crate leaves that choice to the caller, who gets a
typed rejection and can run a fallback of their own.

- `Options` has no `fast` field while only fast mode exists.
- At each upstream `if not options.fast` (or `if options.fast`), port the fast branch only and mark
  the spot `// deviation: D-1`, so F-6 finds every place the slow branch goes. An expression that
  reads `options.fast` as a value (`keep_teasers=options.fast` in `main_extractor.py`, the `fast`
  argument `core.py` passes to `extract_metadata`) takes `true`.
- An upstream test that runs with the default `fast=False` is ported with the fast-mode result. When
  that differs from the upstream assertion, the test asserts the fast-mode output from upstream Python
  and says so in a comment citing D-1.

## Module map

Python module (under upstream `trafilatura/`) → Rust module (under `src/`). One Rust module per
upstream file, so a diff of one Python file points at one Rust file. Scope says what the port covers.

| Upstream | Rust | Scope |
| --- | --- | --- |
| `core.py` | `extraction.rs` (`core` would shadow the `core` crate) | `extract`, `bare_extraction`, the extraction cascade in fast mode |
| `settings.py`, `settings.cfg` | `settings.rs` | `Extractor` options and the `settings.cfg` defaults |
| `main_extractor.py` | `main_extractor.rs` | content and comment extraction, the element handlers |
| `htmlprocessing.py` | `html_processing.rs` | tree cleaning, pruning, link density, `convert_tags` |
| `xpaths.py` | `xpaths.rs` | the content, comment, discard and metadata selectors |
| `baseline.py` | `baseline.rs` | `baseline`, `html2txt` |
| `external.py`, `readability_lxml.py` | none | not ported yet: the readability and jusText fallbacks run only when `fast=False` (Fast mode only, for now) |
| `metadata.py`, `json_metadata.py` | `metadata.rs`, `json_metadata.rs` | metadata from meta tags, JSON-LD, the tree |
| `deduplication.py` | `deduplication.rs` | the duplicate test, simhash and content fingerprint |
| `utils.py` | `utils.rs` | text normalization, trimming, language and size checks |
| `xml.py` | `xml.rs` | tree helpers (`delete_element`) and output: TXT, Markdown, HTML, XML, XML-TEI, JSON, CSV |
| `meta.py` | none | `reset_caches` clears module-global caches here and in htmldate and courlan; this crate has no global caches (Runtime contract) |
| `downloads.py`, `feeds.py`, `sitemaps.py`, `spider.py`, `cli.py`, `cli_utils.py` | none | out of scope: fetching and crawling belong to the caller |
| none | `dom/`, `cancel.rs`, `error.rs`, `async_api.rs` | infrastructure upstream gets from lxml or does not have: the arena tree and its html5ever tree sink (`dom/parse.rs`), `CancelToken` and `Call`, the error types, `extract_async` (feature `tokio`) |

The hidden feature `__internals` exposes `parse_document`, `parse_fragment`, `dump` and the tree types as `__internals`: unstable, for the crate's own integration tests only.

Upstream's own dependencies carry extraction logic, so the parts trafilatura calls are ported with it:
`htmldate` (date extraction) and `courlan` (URL normalization and filtering). The Rust module for
each is named after the package. `charset_normalizer` is replaced by `chardetng` (D-3). `justext` is not ported yet:
only the fallbacks use it. The language check (`py3langid`, optional upstream) is tracked in
`docs/phase-plan.md`.

Tests: the upstream `tests/<name>_tests.py` of each module in the map is its specification; tests of
out-of-scope modules are not ported. It becomes `tests/<name>_tests.rs`, case by case, keeping each
test name (`test_extract_with_metadata` becomes `fn test_extract_with_metadata`), so a changed upstream
test points at one Rust test.

## Syncing to a new upstream release

1. `git -C <checkout> fetch --tags`, then `scripts/upstream.sh` (or `.ps1`). It prints the pin, the
   newest tag, the commits and the files changed under `trafilatura/` and `tests/`.
2. For each changed upstream file, find its Rust module in the map above. Read the upstream diff
   (`scripts/upstream.sh --patch` covers `trafilatura/` and `tests/`) and port each hunk to the
   function that carries its `upstream:` trace comment.
3. Port the changed upstream tests to their Rust tests by name. A test upstream deleted is deleted
   here.
4. Check the version bumps of upstream's own dependencies in `pyproject.toml` (`htmldate`, `courlan`,
   `charset_normalizer`, `lxml`): a dependency release can change extraction output with no change in
   trafilatura itself. Diff the ported parts of that package too.
5. Regenerate the parity fixtures from upstream Python at the new tag and run `scripts/test.sh`.
   A fixture that changed without a matching upstream change is a parser or port difference: fix it,
   or record it in `docs/deviations.md`.
6. Re-check every `docs/deviations.md` row against the new release: upstream may have fixed what a row
   works around, and the row then goes.
7. Update `version` and `commit` in `[package.metadata.upstream]` in the same change as the port.

## Porting rules

- Port behavior function by function. Keep upstream's order of steps: the extraction pipeline mutates
  one tree, so the order of operations is behavior.
- Names follow Rust style, not upstream spelling: rename where it reads better (`xmltotxt` becomes
  `xml_to_text`), drop the leading `_` of private helpers (visibility says private), `HtmlParser`
  not `HTMLParser`, a suffix for a Rust keyword (`ref_`). The trace comment below is the link to
  upstream, so a rename never loses it.
- Each ported function carries one trace comment naming its source:
  `// upstream: main_extractor.py::handle_paragraphs`. No line numbers; they drift every release.
- A departure from upstream, deliberate or forced, is a row in `docs/deviations.md` with an id, and
  the code carries `// deviation: D-n` at the spot. An unrecorded difference is a bug.
- Upstream bugs are ported as they are, unless a row in `docs/deviations.md` says otherwise. Parity
  comes first; a fix belongs upstream.
- A named upstream constant keeps its upstream name and lives in the Rust module of the upstream file
  that defines it (`ESCALATION_MAX_LENGTH` in `extraction.rs`); `settings.cfg` values go in `settings.rs`.
  Never inline one at its use.
- A call into a module a later phase ports stays in place: it calls a stub with the upstream
  signature that returns the neutral value (no duplicate, no metadata), marked
  `// todo(phase N): <module>`. Tests that depend on it are `#[ignore = "phase N"]`. Never delete the
  call site to make a test pass.
- Exceptions. An upstream `raise` that upstream itself catches becomes an `Err` that reaches the same
  handler (the `raise ValueError` rejections in `bare_extraction` become typed rejections). An
  exception that escapes upstream becomes a typed error, never a panic. Arithmetic and indexing where
  Python would raise (`ZeroDivisionError`, `IndexError`) or where Rust would overflow use checked
  forms.

## Runtime contract

These hold for the whole crate. They change no output of a completed run, so they are not deviations;
upstream has no equivalent.

- Synchronous and CPU-bound. The pipeline never uses `async`: there is nothing to await, and async
  would change every signature on the way down. The optional `tokio` feature adds `extract_async`, a
  thin wrapper that runs the sync call on `spawn_blocking` and cancels it when the future is dropped.
  Without it the caller decides where the call runs (a thread pool, `spawn_blocking`, a .NET worker).
- Cancellable. Every public extraction entry point takes a cancellation and deadline handle. It
  travels inside the per-call context passed where upstream passes `options`, and is checked at stage
  boundaries and inside every loop over nodes; a ported function that checks it returns `Result`.
  The handle is `Call` (`cancel.rs`): an optional `CancelToken` (`Arc<AtomicBool>`, no dependency) and
  an optional deadline `Instant`. Loops check it about every 256 nodes (one atomic load and one
  `Instant::now()`), a constant tuned by benchmarks; cancel gives `ExtractError::Cancelled`, the
  deadline `ExtractError::DeadlineExceeded`. A check only returns early with a distinct error; it never
  changes a branch, so a run that completes gives the same output as one without a handle.
- Bounded. `MAX_FILE_SIZE` and `MAX_TREE_SIZE` are ported exactly as upstream applies them
  (`MAX_TREE_SIZE` counts the children of the output body, strips `hi` first, then rejects; empty
  means no limit). Nesting depth stops at 256 as libxml2 does: parsing ends at the cut and the
  partial tree is kept. That is upstream behavior, always on. Limits the port adds (`max_input_bytes`,
  `max_nodes` counted while parsing) are options, off by default; a caller that turns one on accepts
  rejections upstream would not make. No recursion whose depth follows the input, so callers never
  need large thread stacks.
- No hidden clock. Upstream sets `max_date` from `datetime.now()` (`settings.py`); here it is an
  option (`Call`) whose default is computed on each `extract` call, never when the extractor is built,
  and parity fixtures pin it.
- Panic-free on any input. The crate runs behind FFI, where a panic aborts the host process.
  `unwrap`, `expect` and `panic` fail clippy outside tests; `string_slice` fails everywhere, tests
  included; `indexing_slicing` is a warning, and CI denies warnings. `clippy.toml` exempts only
  `#[test]` and `#[cfg(test)]` code: a shared helper under `tests/` starts with
  `#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]`.
  Clippy does not catch arithmetic overflow, division by zero or a `RefCell` double borrow: use
  checked arithmetic (Exceptions in Porting rules) and never let two `RefCell` borrows overlap.
- Every `pub` item has a doc comment: `missing_docs` warns and CI denies warnings.
- No process-global mutable state. Anything upstream keeps in a module global is an explicit value the
  caller owns. `Send + Sync` options and results, so concurrent calls need no locking.
- `unsafe` is forbidden.
- The public API is this crate's own; the pipeline under it follows upstream function by function.
  `Options` (with `format`, as upstream `Extractor`) builds an immutable `Extractor` once, which
  resolves the effective `formatting` and `with_metadata` as upstream does. `Extractor::extract(input,
  &call)` is upstream `bare_extraction` plus the `id` and fingerprint step of upstream `extract`;
  `Extractor::render(&doc)` is `determine_returnstring` in the extractor's format. `Input` is bytes or
  a string with an optional URL and record id; `Call` carries the cancel token, deadline, `max_date`
  and the caller-owned `DedupCache`. These API types never leak into ported functions beyond the
  context they pass down.

## Performance and memory

CPU time and peak memory per page matter: the crate runs many pages at once inside services that
budget both. Order of priority: parity, then code that maps onto upstream, then speed and memory.
An optimization is worth doing when a measurement shows it, and it never changes output; one that
would is a deviation and needs the user's approval before it lands.

- Measure, do not guess: time, peak heap and allocation count per page on the parity corpus. An
  optimization lands with its numbers in the commit message.
- Optimize in the shared layers first (tree, text buffers, selectors, decoding), where it costs the
  ported functions nothing. Keep upstream's function boundaries: do not merge, split or inline ported
  functions for speed, so an upstream diff still lands on one Rust function. Inside a ported function,
  keep upstream's steps and their order; a reshape there carries a comment saying what it does and why
  it is equivalent.
- Upstream deep-copies trees (`copy(tree)`, the cleaned backup). Port each copy with the same
  semantics, but a copy that is only read afterwards may be a borrow, and a real copy should be a
  cheap arena clone, not a node-by-node rebuild.
- Memory is bounded by the input limits: no growth that does not follow input size, no cache that
  outlives a call unless the caller owns it, everything freed when the call returns.
- Borrow the caller's input bytes; decode to a new buffer only when the input is not already UTF-8.
- The library never sets a global allocator; that choice belongs to the binary or binding.

## Dependency policy

- Fewest dependencies that do the job. Write a small piece (a few hundred lines) before taking a crate
  for it.
- A Rust crate named after a Python package is not that package. Where upstream calls `htmldate` or
  `courlan`, port the logic it uses; take a crate only for infrastructure with no upstream logic in it
  (HTML parsing, regex, encoding tables and detection).
- Take a crate only when it is actively maintained (a release in the last 12 months and issues being
  answered), widely used or backed by an organization, and under a license `deny.toml` allows.
  Dev-only crates (tests, benchmarks, measurement) never reach users: stable and widely used is
  enough, without a recent release.
- One version of each crate in `Cargo.lock`: `cargo deny check` fails on duplicates, advisories,
  unmaintained crates, unknown sources and unknown licenses.
- Each dependency has a row in `docs/dependencies.md`: what it replaces upstream and why this crate.
  No exact `=` pins and no git dependencies; Dependabot moves versions weekly.
- Optional functionality goes behind a cargo feature so the default build stays small.

## Gotchas

Traps found while reading upstream. Each one silently changes output.

- lxml text and tail. Text after an element's closing tag belongs to that element (`.tail`), not to
  the parent. `main_extractor.py`, `xml.py` and `htmlprocessing.py` use `.tail` throughout. The Rust
  tree must model text and tail directly; emulating them through text nodes needs care at every move,
  strip and delete.
- Deleting keeps or drops the tail. Upstream's `delete_element(element, keep_tail=...)` (`xml.py`)
  decides per call site, `lxml.etree.strip_tags` merges children's text into the parent, and a plain
  `parent.remove(child)` drops the tail. Port the exact operation each call site uses.
- The parser is not html5. Upstream parses with `lxml.html.HTMLParser` (libxml2), configured in
  `utils.py` (`HTML_PARSER`: comments and processing instructions removed). libxml2 2.14+ tokenizes
  as HTML5 but builds the tree with its own rules: no implied `tbody`, no adoption agency, different
  handling of misnested and stray tags. This crate uses html5ever (D-2). lxml wheels bundle libxml2
  2.14.6 on Linux and macOS but 2.11.9 on Windows, so upstream itself differs by platform: fixtures
  come from Linux. Upstream may switch parsers: check OI-5 on every sync.
- html5ever's tree sink cannot stop the parse. After the depth cut it must ignore every later call,
  and depth is checked again when html5ever moves a subtree (reparenting, foster parenting, the
  adoption agency). Attribute values are stored as `String`: `StrTendril` is not `Send`.
- Decode strictly. Upstream tries each guessed codec and falls back to UTF-8 with replacement only
  when all fail; `encoding_rs`'s `Encoding::decode` sniffs a BOM and replaces silently, which skips
  that fallback. Feed the detector the sample upstream uses (whole input under 10000 bytes, else the
  first and last 5000).
- XPath semantics. `contains(@class, "x")` is a substring test, not a class-token test;
  `translate(...)` is the case-folding idiom; results come in document order. Port each predicate
  exactly, including the substrings that look like accidents.
- Python `re` is not Rust `regex`. Rust `regex` has no lookaround and no backreferences, which
  upstream uses (`xml.py` Markdown escaping uses `(?<!...)`, `(?!...)` and `\2`). Rewrite those as
  explicit code, and test them against upstream's regex on the same inputs. The selectors use regex
  too: EXSLT `re:test(...)` in `xpaths.py` and `settings.py` runs Python `re`, including the
  lookahead in `[Cc]omment(?!ary)` that keeps "commentary" sections out of the comment selector.
- `\s` and whitespace differ. Python's `\s`, `str.split()`, `str.strip()` and `str.isspace()` treat
  `\x1c`-`\x1f` as whitespace; Rust's `\s` and `char::is_whitespace` do not. Use one helper that
  matches Python's definition everywhere upstream does.
- A character class upstream did not mean. `utils.py` `LINES_TRIMMING` is `(?<![p{P}>])\n`: Python
  `re` has no `\p{P}`, so the class is the literal characters `p`, `{`, `P`, `}`, `>`. Port the
  literal behavior and keep a comment saying so.
- Lengths are code points. Python `len(text)` and `text[:n]` count code points; Rust `str::len` counts
  bytes and byte slicing panics mid-character. Every size threshold (`MIN_EXTRACTED_SIZE` and the
  others) compares `chars().count()`. On an lxml element, `len(element)` is its child count, not a
  text length (`len(postbody)` in `core.py`).
- Truthiness. `if text:` is false for both `None` and `""`; `if element:` on an lxml element is
  false when it has no children, even when it has text. Port the test upstream wrote, not the one it
  reads like.
- Process-global state. `deduplication.py` keeps `LRU_TEST` as a module global, cleared by
  `meta.reset_caches()`. It becomes caller-owned state here. `functools.lru_cache` on pure functions
  is only memoization: drop it unless profiling says otherwise.
- Defaults come from `settings.cfg` as well as `settings.py`: `MAX_FILE_SIZE = 20000000`,
  `MIN_FILE_SIZE = 10`, an empty `MAX_TREE_SIZE` meaning no limit. Read both files on every sync.
- Ordering. Python `dict` keeps insertion order and `set` does not. Where upstream output depends on
  iterating a dict, keep the order (an ordered map or a `Vec`); where it iterates a set, the order is
  undefined upstream too, so sort and record it in `docs/deviations.md`.

## Parity harness

How crate output is compared with upstream. Fixtures are generated, never committed.

- Corpus: pages are read from an upstream checkout at the pin (`$TRAFILATURA_UPSTREAM`, else
  `../trafilatura`): every HTML page in `tests/eval/` and `tests/cache/`, and the HTML files of
  `tests/resources/` listed by name in `tools/parity/`. No page is copied into this repo: they are
  third-party content upstream's license does not cover. Extra local pages come in through an
  environment variable pointing outside the repo.
- Fixtures: `tools/parity/` (the only Python in the repo, locked by `uv.lock`) runs upstream at the pin
  on Linux, where lxml bundles libxml2 2.14.6, and writes `.temp/fixtures/<pin>/`. Two per page:
  `lxml` is upstream as is and is only reported; `html5` is upstream with `utils.fromstring` and
  `baseline.fragment_fromstring` replaced by an HTML5 parse copied into `lxml.html` elements, and
  crate output must match it exactly.
- Each fixture holds the output of every format, the metadata fields, the rejection if any, and the
  parsed tree dump. Generation is deterministic: `fast=True`, a full `date_params` dict with a fixed
  `max_date`, `dedup=False`, one fresh extractor per page. The crate gets the same `max_date`.
- Copy rule for the `html5` tree: attributes whose name is not a valid XML name are dropped and
  XML-invalid characters are stripped, because lxml cannot store them. The comparator applies the same
  rule to crate output; the crate keeps everything.
- Tree dump: one line per node in document order, indented by depth; local name (namespace only for
  SVG and MathML), attributes sorted by name, text and tail as JSON strings with whitespace kept; no
  comments, doctype or processing instructions.
- Every mismatch gets a class: `port-bug` (crate differs from `html5`), `parser` (the two fixtures
  differ, D-2), `python-parser` (html5lib predates a WHATWG rule html5ever follows), `encoding` (input
  not valid UTF-8, D-3), `decompress` (D-4), `normalized` (the copy rule touched the page; check by
  hand). Only `port-bug` fails the test. The report is `.temp/parity/report.md`.
- The parity test checks for fixtures when it runs and returns early, naming `scripts/fixtures.sh`, when
  they are missing, so `cargo test` works without Python or Docker. With `PARITY_REQUIRE_FIXTURES` set
  (CI's Ubuntu job), missing fixtures or an index without pages fail the test instead.
- F1 against upstream's `tests/evaldata.json`, resolved as upstream's `eval_common.py` does, is
  reported for crate and upstream as a quality signal, not a gate.

## Commands

| Command | Does |
| --- | --- |
| `scripts/test.sh` / `scripts/test.ps1` | every check the CI test job runs: version check, fmt, clippy `-D warnings` (all features and targets, default features, no default features), tests (the parity test when fixtures exist), `cargo deny check` |
| `scripts/fixtures.sh` / `scripts/fixtures.ps1` | generates parity fixtures for the pin: `uv` on Linux, Docker elsewhere, both from `tools/parity/uv.lock` |
| `scripts/measure.sh` / `scripts/measure.ps1` | time per page for crate and upstream, and heap per page (comparable only on the Rust side: tracemalloc misses the lxml tree), into `.temp/measure/<pin>.json`; run by hand |
| `scripts/upstream.sh` / `scripts/upstream.ps1` | upstream changes since the pin (`--to REF`, `--patch`) |
| `scripts/version.sh` / `scripts/version.ps1` | prints the crate version; `--check`, `--tag vX.Y.Z` |

CI (`.github/workflows/`): `ci.yml` calls `_ci.yml` on pushes to `main` and pull requests targeting
`main`, on Ubuntu and Windows with the toolchain pinned to `rust-version`. `release.yml` checks that
the tag names the crate version and is on `main`, runs `_ci.yml`, and publishes to crates.io through
the `crates-io` environment. `quality.yml` re-checks advisories weekly. Actions are pinned by commit
SHA and Dependabot moves them, except `dtolnay/rust-toolchain`, which moves by hand with
`rust-version` (OI-6 in `docs/phase-plan.md`).

Scripts come in `.sh` and `.ps1` pairs with the same arguments; change both together. The CI test job
calls `scripts/test.sh`, so CI and a local run execute the same checks. On Ubuntu a step before it
runs `scripts/fixtures.sh`, cached by pin and `tools/parity/uv.lock`, so the parity test runs there;
the Windows job runs without fixtures. The parity report is uploaded as an artifact.

## Commits

Conventional commits (`feat:`, `fix:`, `docs:`, `ci:`, `chore:`). A port of an upstream change names
the upstream release or commit in the body.
