# Agent guide

trafilatura-rust is a Rust port of the Python library [trafilatura](https://github.com/adbar/trafilatura):
main-text, comment and metadata extraction from web pages. Behavior parity with upstream Python is the
bar. A Rust function is correct when it returns what the upstream function returns for the same input,
not when it looks similar or scores well on a benchmark.

This file says where the port starts, how to follow upstream when it changes, and the traps that make a
faithful port go wrong. `docs/phase-plan.md` holds what ships next and the open items;
`docs/deviations.md` lists every place the port departs from upstream on purpose; `docs/dependencies.md`
records why each dependency is there.

## Upstream reference

- The pinned release is in `Cargo.toml` under `[package.metadata.upstream]` (`version`, `commit`). That
  table is the only place it is stated: never repeat the version in code, comments or docs.
- Upstream checkout: `../trafilatura` beside this repository, or the path in `TRAFILATURA_UPSTREAM`.
  Read upstream code at the pinned commit (`git -C ../trafilatura show <commit>:trafilatura/core.py`),
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
- Borrowed code is written to fit this crate and reads like the rest of it: it keeps its
  `// upstream: file.py::fn` comment, and where the logic is not obvious a comment explains what it
  does and why.
- Agreement with another port's output is not parity; parity is upstream Python output only.

## Fast mode only, for now

The port covers upstream's `fast=True` path; `fast=False` is not ported yet (deviation D-1, followup
F-6 in `docs/phase-plan.md`). Upstream's default runs readability and jusText after the main extractor
and keeps the better result; until F-6 lands, this crate leaves that choice to the caller, who gets a
typed rejection and can run a fallback of their own.

- `Options` has no `fast` field while only fast mode exists.
- At each upstream `if not options.fast` (or `if options.fast`), port the fast branch only and mark
  the spot `// deviation: D-1`, so F-6 finds every place the slow branch goes. Expressions that read `options.fast` as a value (for example
  `keep_teasers=options.fast` in `main_extractor.py`) take `true`.
- An upstream test that runs with the default `fast=False` is ported with the fast-mode result. When
  that differs from the upstream assertion, the test asserts the fast-mode output from upstream Python
  and says so in a comment citing D-1.

## Module map

Python module (under upstream `trafilatura/`) → Rust module (under `src/`). Rust modules carry the
upstream name so a diff of one Python file points at one Rust file. Scope says what the port covers.

| Upstream | Rust | Scope |
| --- | --- | --- |
| `core.py` | `core.rs` | `extract`, `bare_extraction`, the extraction cascade in fast mode |
| `settings.py`, `settings.cfg` | `settings.rs` | `Extractor` options and every default value; one place for all of them |
| `main_extractor.py` | `main_extractor.rs` | content and comment extraction, the element handlers |
| `htmlprocessing.py` | `htmlprocessing.rs` | tree cleaning, pruning, link density, `convert_tags` |
| `xpaths.py` | `xpaths.rs` | the content, comment, discard and metadata selectors |
| `baseline.py` | `baseline.rs` | `baseline`, `html2txt` |
| `external.py`, `readability_lxml.py` | none | not ported yet: the readability and jusText fallbacks run only when `fast=False` (Fast mode only, for now) |
| `metadata.py`, `json_metadata.py` | `metadata.rs`, `json_metadata.rs` | metadata from meta tags, JSON-LD, the tree |
| `deduplication.py` | `deduplication.rs` | the duplicate test, simhash and content fingerprint |
| `utils.py` | `utils.rs` | text normalization, trimming, language and size checks |
| `xml.py` | `xml.rs` | output: TXT, Markdown, HTML, XML, XML-TEI, JSON, CSV |
| `meta.py` | none | `reset_caches` clears module-global caches here and in htmldate and courlan; this crate has no global caches (Runtime contract) |
| `downloads.py`, `feeds.py`, `sitemaps.py`, `spider.py`, `cli.py`, `cli_utils.py` | none | out of scope: fetching and crawling belong to the caller |

Upstream's own dependencies carry extraction logic, so the parts trafilatura calls are ported with it:
`htmldate` (date extraction), `courlan` (URL normalization and filtering), `charset_normalizer`
(decoding bytes). The Rust module for each is named after the package. `justext` is not ported yet: only
the fallbacks use it. The language check (`py3langid`, optional upstream) is tracked in
`docs/phase-plan.md`.

Tests: upstream `tests/*_tests.py` are the specification. Port a test case by case and keep its name
(`test_extract_with_metadata` becomes `fn test_extract_with_metadata`), so a changed upstream test
points at one Rust test.

## Syncing to a new upstream release

1. `git -C ../trafilatura fetch --tags`, then `scripts/upstream.sh` (or `.ps1`). It prints the pin,
   the newest tag, the commits and the files changed under `trafilatura/` and `tests/`.
2. For each changed upstream file, find its Rust module in the map above. Read the upstream diff
   (`scripts/upstream.sh --patch`, or `git diff <pin> <tag> -- trafilatura/<file>`) and port each
   hunk to the function that carries its `upstream:` trace comment.
3. Port the changed upstream tests the same way. A test upstream deleted is deleted here.
4. Check the version bumps of upstream's own dependencies in `pyproject.toml` (`htmldate`, `courlan`,
   `charset_normalizer`, `lxml`): a dependency release can change extraction output with
   no change in trafilatura itself. Diff the ported parts of that package too.
5. Regenerate the parity fixtures from upstream Python at the new tag and run `scripts/test.sh`.
   A fixture that changed without a matching upstream change is a parser or port difference: fix it,
   or record it in `docs/deviations.md`.
6. Re-check every `docs/deviations.md` row against the new release: upstream may have fixed what a row
   works around, and the row then goes.
7. Update `version` and `commit` in `[package.metadata.upstream]` in the same change as the port.

## Porting rules

- Port behavior function by function and keep upstream names in snake_case. Keep upstream's order of
  steps: the extraction pipeline mutates one tree, so the order of operations is behavior.
- Each ported function carries one trace comment naming its source:
  `// upstream: main_extractor.py::handle_paragraphs`. No line numbers; they drift every release.
- A departure from upstream, deliberate or forced, is a row in `docs/deviations.md` with an id, and
  the code carries `// deviation: D-n` at the spot. An unrecorded difference is a bug.
- Upstream bugs are ported as they are, unless a row in `docs/deviations.md` says otherwise. Parity
  comes first; a fix belongs upstream.
- Every default value and threshold lives in `settings.rs`, named after its upstream key
  (`MIN_EXTRACTED_SIZE`), never inline at its use.

## Runtime contract

These hold for the whole crate and are not departures from upstream; upstream has no equivalent.

- Synchronous and CPU-bound. The pipeline never uses `async`: there is nothing to await, and async
  would change every signature on the way down. The optional `tokio` feature adds `extract_async`, a
  thin wrapper that runs the sync call on `spawn_blocking` and cancels it when the future is dropped.
  Without it the caller decides where the call runs (a thread pool, `spawn_blocking`, a .NET worker).
- Cancellable. Every public extraction entry point takes a cancellation and deadline handle, checked
  at stage boundaries and inside every loop over nodes, returning a distinct error when it fires.
  A runaway page must stop inside the crate, not by the caller abandoning the thread. A check only
  returns early; it never changes a branch, so a run that completes gives the same output as one
  without a handle.
- Bounded. Input size, tree size and nesting depth have limits (upstream's `MAX_FILE_SIZE`,
  `MAX_TREE_SIZE`, plus a depth limit the port adds); exceeding one is an error, not a slow path.
  No recursion whose depth follows the input.
- Panic-free on any input. `unwrap`, `expect`, `panic`, string slicing and indexing fail clippy
  outside tests (`indexing_slicing` is a warning, and CI denies warnings). The crate runs behind FFI,
  where a panic aborts the host process. `clippy.toml` exempts only `#[test]` and `#[cfg(test)]`
  code: a shared helper under `tests/` starts with `#![allow(clippy::unwrap_used, clippy::expect_used)]`.
- No process-global mutable state. Anything upstream keeps in a module global is an explicit value the
  caller owns. `Send + Sync` options and results, so concurrent calls need no locking.
- `unsafe` is forbidden.
- The public API is this crate's own; the pipeline under it follows upstream function by function.
  `extract` and `bare_extraction` keep upstream's names and meaning, while the extractor handle,
  the cancellation handle, byte input and the error types belong to the API layer and never leak
  into ported functions beyond the handle they pass down.

## Performance and memory

CPU time and peak memory per page matter: the crate runs many pages at once inside services that
budget both. Order of priority: parity, then code that maps onto upstream, then speed and memory.
An optimization is worth doing when a measurement shows it, and it never changes output.

- Measure, do not guess: time, peak heap and allocation count per page on the parity corpus. An
  optimization lands with its numbers in the commit message.
- Optimize in the shared layers first (tree, text buffers, selectors, decoding), where it costs the
  ported functions nothing. Inside a ported function, keep upstream's steps and their order; a reshape
  there carries a comment saying what it does and why it is equivalent.
- Upstream deep-copies trees (`copy(tree)`, the cleaned backup). Port each copy with the same
  semantics, but a copy that is only read afterwards may be a borrow, and a real copy should be a
  cheap arena clone, not a node-by-node rebuild.
- Memory is bounded by the input limits: no growth that does not follow input size, no cache that
  outlives a call unless the caller owns it, everything freed when the call returns.
- No recursion whose depth follows the input (Runtime contract), so callers never need large thread
  stacks.
- Borrow the caller's input bytes; decode to a new buffer only when the input is not already UTF-8.
- The library never sets a global allocator; that choice belongs to the binary or binding.
- An optimization that changes output is a deviation and needs a `docs/deviations.md` row and the
  user's approval first.

## Dependency policy

- Fewest dependencies that do the job. Write a small piece (a few hundred lines) before taking a crate
  for it.
- A Rust crate named after a Python package is not that package. Where upstream calls `htmldate` or
  `courlan`, port the logic it uses; take a crate only for infrastructure with no upstream logic in it
  (HTML tokenizing, regex, encoding tables). Encoding detection is open item OI-2.
- Take a crate only when it is actively maintained (a release in the last 12 months and issues being
  answered), widely used or backed by an organization, and under a license `deny.toml` allows.
- One version of each crate in `Cargo.lock`: `cargo deny check` fails on duplicates, advisories,
  unmaintained crates, unknown sources and unknown licenses.
- Each dependency has a row in `docs/dependencies.md`: what it replaces upstream and why this crate.
  No exact `=` pins and no git dependencies; Dependabot moves versions weekly.
- Optional functionality goes behind a cargo feature so the default build stays small.

## Gotchas

Traps found while reading upstream. Each one silently changes output.

- lxml text and tail. Text after an element's closing tag belongs to that element (`.tail`), not to
  the parent. `.tail` appears about 46 times in `main_extractor.py`, 58 in `xml.py` and 11 in
  `htmlprocessing.py`.
  The Rust tree must model text and tail directly. Emulating them through text nodes needs
  care at every move, strip and delete.
- Deleting keeps or drops the tail. Upstream's `delete_element(element, keep_tail=...)` (`xml.py`)
  decides per call site, `lxml.etree.strip_tags` merges children's text into the parent, and a plain
  `parent.remove(child)` drops the tail. Port the exact operation each call site uses.
- The parser is not html5. Upstream parses with `lxml.html.HTMLParser` (libxml2), configured in
  `utils.py` (`HTML_PARSER`: comments and processing instructions removed). libxml2 builds a
  different tree from an html5 parser for malformed markup: no implied `tbody`, different handling
  of misnested and stray tags. Parity fixtures expose this; record parser-caused differences as
  deviations. An upstream proposal to switch parsers is open, so check the parser on every sync.
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
  others) compares `chars().count()`.
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

## Commands

| Command | Does |
| --- | --- |
| `scripts/test.sh` / `scripts/test.ps1` | every CI check: fmt, clippy `-D warnings`, tests, `cargo deny check` |
| `scripts/upstream.sh` / `scripts/upstream.ps1` | upstream changes since the pin (`--to REF`, `--patch`) |
| `scripts/version.sh` / `scripts/version.ps1` | prints the crate version; `--check`, `--tag vX.Y.Z` |

CI (`.github/workflows/`): `ci.yml` calls `_ci.yml` on pushes to `main` and pull requests targeting
`main`, on Ubuntu and Windows with the toolchain pinned to `rust-version`. `release.yml` checks that
the tag names the crate version and is on `main`, runs `_ci.yml`, and publishes to crates.io through
the `crates-io` environment. `quality.yml` re-checks advisories weekly. Actions are pinned by commit
SHA and Dependabot moves them, except `dtolnay/rust-toolchain`, which moves by hand with
`rust-version` (OI-6 in `docs/phase-plan.md`).

Scripts come in `.sh` and `.ps1` pairs with the same arguments; change both together. Workflows only
call scripts, so CI and a local run execute the same commands.

## Commits

Conventional commits (`feat:`, `fix:`, `docs:`, `ci:`, `chore:`). A port of an upstream change names
the upstream release or commit in the body.
