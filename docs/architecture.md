# Architecture

How the crate is laid out and the contracts it keeps: the module map, the runtime contract, the
planned public API, and the performance rules. Read it before changing module layout, the public API,
cancellation, input limits or anything performance-related. Items marked planned are decided design
that no code implements yet.

## Module map

Python module (under upstream `trafilatura/`) to Rust module (under `src/`), one per upstream file
(Porting rules in `AGENTS.md`). The status column says what
exists today; phases are in `phase-plan.md`.

| Upstream | Rust | Status | Scope |
| --- | --- | --- | --- |
| `core.py` | `extraction.rs` (`core` would shadow the `core` crate) | planned | `extract`, `bare_extraction`, the extraction cascade in fast mode |
| `settings.py`, `settings.cfg` | `settings.rs` | partial: `MAX_FILE_SIZE`, `MIN_FILE_SIZE` and `DEFAULT_MAX_NODES` exist | `Extractor` options and the `settings.cfg` defaults |
| `main_extractor.py` | `main_extractor.rs` | planned | content and comment extraction, the element handlers |
| `htmlprocessing.py` | `html_processing.rs` | planned | tree cleaning, pruning, link density, `convert_tags` |
| `xpaths.py` | `xpaths.rs` | planned | the content, comment, discard and metadata selectors |
| `baseline.py` | `baseline.rs` | planned | `baseline`, `html2txt` |
| `external.py`, `readability_lxml.py` | none | not ported | the readability and jusText fallbacks run only when `fast=False` (D-1) |
| `metadata.py`, `json_metadata.py` | `metadata.rs`, `json_metadata.rs` | planned | metadata from meta tags, JSON-LD, the tree |
| `deduplication.py` | `deduplication.rs` | planned | the duplicate test, simhash and content fingerprint |
| `utils.py` | `utils.rs` | partial: `handle_compressed_file`, `detect_encoding`, `decode_file`, `is_dubious_html`, `repair_faulty_html`, `load_html` exist | text normalization, trimming, language and size checks, decoding and HTML loading |
| `xml.py` | `xml.rs` | planned | tree helpers (`delete_element`) and output: TXT, Markdown, HTML, XML, XML-TEI, JSON, CSV |
| `meta.py` | none | not needed | `reset_caches` clears module-global caches here and in htmldate and courlan; this crate has no global caches (Runtime contract) |
| `downloads.py`, `feeds.py`, `sitemaps.py`, `spider.py`, `cli.py`, `cli_utils.py` | none | out of scope | fetching and crawling belong to the caller |
| none | `dom/` (`mod.rs`, `parse.rs`, `dump.rs`) | exists | infrastructure upstream gets from lxml: the arena tree, its html5ever tree sink (`dom/parse.rs`), the tree dump the parity harness compares |
| none | `cancel.rs`, `error.rs` | exists | `CancelToken` and `Call`, `ExtractError` and `Rejection` |
| none | `async_api.rs` | planned | `extract_async` (feature `tokio`) |

The hidden feature `__internals` exposes the tree types, `parse_document`, `parse_fragment`, `dump`,
`ParseLimits`, and the `settings` constants and `utils` functions the integration tests call, as
`__internals`: unstable, for the crate's own integration tests only.

Upstream's own dependencies carry extraction logic, so the parts trafilatura calls are ported with it:

| Package | Handling | Status |
| --- | --- | --- |
| `htmldate` (date extraction) | ported as a module named `htmldate` | planned |
| `courlan` (URL normalization and filtering) | ported as a module named `courlan` | planned |
| `charset_normalizer` | replaced by `chardetng` (D-3) | done |
| `justext` | not ported: only the `fast=False` fallbacks use it | not ported |
| `py3langid` (language check, optional upstream) | open item OI-4 in `phase-plan.md` | undecided |

## Runtime contract

These hold for the whole crate. They change no output of a completed run, so they are not deviations;
upstream has no equivalent.

- Synchronous and CPU-bound. The pipeline never uses `async`: there is nothing to await, and async
  would change every signature on the way down. The optional `tokio` feature (planned) adds
  `extract_async`, a thin wrapper that runs the sync call on `spawn_blocking` and cancels it when the
  future is dropped. Without it the caller decides where the call runs (a thread pool,
  `spawn_blocking`, a .NET worker).
- Cancellable. Every public extraction entry point takes a cancellation and deadline handle. It
  travels inside the per-call context passed where upstream passes `options`, and is checked at stage
  boundaries and inside every loop over nodes; a ported function that checks it returns `Result`.
  The handle is `Call` (`cancel.rs`): an optional borrowed `CancelToken` (`Arc<AtomicBool>`, no
  dependency) and an optional deadline `Instant`. Loops check it about every 256 nodes
  (`CHECK_INTERVAL`: one atomic load and one `Instant::now()`), a constant tuned by benchmarks; cancel
  gives `ExtractError::Cancelled`, the deadline `ExtractError::DeadlineExceeded`. A check only returns
  early with a distinct error; it never changes a branch, so a run that completes gives the same
  output as one without a handle.
- Bounded. `MAX_FILE_SIZE` and `MAX_TREE_SIZE` are ported exactly as upstream applies them
  (`MAX_TREE_SIZE` counts the children of the output body, strips `hi` first, then rejects; empty
  means no limit). Nesting depth stops at 256 as libxml2 does (`MAX_DEPTH` in `dom/parse.rs`): parsing
  ends at the cut and the partial tree is kept. That is upstream behavior, always on. Limits the port
  adds are options: `max_input_bytes` (planned) is off by default, and a caller that turns it on
  accepts rejections upstream would not make; `max_nodes` (`ParseLimits::max_nodes`: elements created
  while parsing, html5ever's clones included) is on by default at `DEFAULT_MAX_NODES` (D-5), and a
  caller may lift it with `None`. No recursion whose depth follows the input, so callers never need
  large thread stacks.
- No hidden clock. Upstream sets `max_date` from `datetime.now()` (`settings.py`); here it is an
  option (`Call`, planned) whose default is computed on each `extract` call, never when the extractor
  is built, and parity fixtures pin it.
- Panic-free on any input. The crate runs behind FFI, where a panic aborts the host process.
  `unwrap`, `expect` and `panic` fail clippy outside tests; `string_slice` fails everywhere, tests
  included; `indexing_slicing` is a warning, and CI denies warnings. `clippy.toml` exempts only
  `#[test]` and `#[cfg(test)]` code, so a shared helper under `tests/` starts with the `allow` line
  given in `AGENTS.md` (Runtime rules).
  Clippy does not catch arithmetic overflow, division by zero or a `RefCell` double borrow: use
  checked arithmetic (Exceptions in the porting rules of `AGENTS.md`) and never let two `RefCell`
  borrows overlap.
- Every `pub` item has a doc comment: `missing_docs` warns and CI denies warnings.
- No process-global mutable state. Anything upstream keeps in a module global is an explicit value the
  caller owns. `Send + Sync` options and results, so concurrent calls need no locking.
- `unsafe` is forbidden.

## Public API (planned)

The public API is this crate's own; the pipeline under it follows upstream function by function.
Today only `Call`, `CancelToken`, `ExtractError` and `Rejection` are public. The rest is decided
design with no code yet:

- `Options` (with `format`, as upstream `Extractor`) builds an immutable `Extractor` once, which
  resolves the effective `formatting` and `with_metadata` as upstream does.
- `Extractor::extract(input, &call)` is upstream `bare_extraction` plus the `id` and fingerprint step
  of upstream `extract`.
- `Extractor::render(&doc)` is `determine_returnstring` in the extractor's format.
- `Input` is bytes or a string with an optional URL and record id.
- `Call` also carries `max_date` and the caller-owned `DedupCache`.

These API types never leak into ported functions beyond the context they pass down.

## Performance and memory

CPU time and peak memory per page matter: the crate runs many pages at once inside services that
budget both. Order of priority: parity, then code that maps onto upstream, then speed and memory.
An optimization is worth doing when a measurement shows it, and it never changes output; one that
would is a deviation and needs the user's approval before it lands.

- Measure, do not guess: time, peak heap and allocation count per page on the parity corpus
  (`scripts/measure.sh`, see `parity-harness.md`). An optimization lands with its numbers in the
  commit message.
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
