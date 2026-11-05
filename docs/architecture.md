# Architecture

How the crate is laid out and the contracts it keeps: the module map, the runtime contract, the
public API, and the performance rules. Read it before changing module layout, the public API,
cancellation, input limits or anything performance-related. Items marked planned are decided design
that no code implements yet.

## Module map

Python module (under upstream `trafilatura/`) to Rust module (under `src/`), one per upstream file, a
file or a directory (Porting rules in `AGENTS.md`). The status column says what
exists today; phases are in `phase-plan.md`.

| Upstream | Rust | Status | Scope |
| --- | --- | --- | --- |
| `core.py` | `extraction.rs` (`core` would shadow the `core` crate) | exists: `determine_returnstring`, `trafilatura_sequence`, `bare_extraction` and `internal_extraction` (upstream's `_internal_extraction`, which `extract` wraps), both taking a `Source` (a page or a caller's tree); the XML branch of `determine_returnstring` (its empty-element cleaning loop included, Phase 6) is a stub (`// todo(phase N)`) | the extraction cascade in fast mode, `bare_extraction`, `extract` and the output step |
| `settings.py`, `settings.cfg` | `settings.rs` | partial: `Extractor`, `Config` (the extraction keys of `settings.cfg`), `Focus`, `Format`, `Document` (with `clean_and_trim`), `set_date_params`, the element lists, `BASIC_CLEAN_XPATH` and `DEFAULT_MAX_NODES` exist | `Extractor` options, `Document` (the result: metadata fields, texts and an owned compact tree of the body and comments body), the `settings.cfg` defaults and the shared tag lists |
| `main_extractor.py` | `main_extractor.rs`, `main_extractor/tables.rs`, `main_extractor/content.rs` | exists: every constant, helper and handler, `prune_unwanted_sections`, `recover_wild_text`, `extract_content` and `extract_comments`; `extract_body` is upstream's `_extract`, private because `extract` names the public entry point | content and comment extraction, the element handlers |
| `htmlprocessing.py` | `html_processing.rs`, `html_processing/convert.rs` | exists: tree cleaning, pruning, link density, `handle_textnode`, `process_node`, the conversions, `convert_tags` and the HTML output (`HTML_TAG_MAPPING`, `HTML_CONVERSIONS` as `html_conversion`, `convert_to_html`, `build_html_output`) | tree cleaning, pruning, link density, text-node handling, `convert_tags`, HTML output |
| `xpaths.py` | `xpaths.rs`, `xpaths/discard.rs`, `xpaths/select.rs` | exists: the content, comment, discard and metadata selectors (`AUTHOR_XPATHS`, `AUTHOR_DISCARD_XPATHS`, `CATEGORIES_XPATHS`, `TAGS_XPATHS`, `TITLE_XPATHS`) | the content, comment, discard and metadata selectors |
| `baseline.py` | `baseline.rs` | exists: `baseline`, `baseline_tree`, `html2txt`, `html2txt_tree`, `basic_cleaning`, `build_body`, `block_text` and the JSON-LD readers; only `html2txt` is public (`api.rs`) | `baseline`, `html2txt` |
| `external.py`, `readability_lxml.py` | none | not ported | the readability and jusText fallbacks run only when `fast=False` (D-1) |
| `metadata.py` | `metadata.rs`, `metadata/page.rs` | exists: `extract_metadata` and the meta tag, JSON-LD, title, author, URL, sitename, category, tag and license readers; the date comes from `find_date` (`htmldate/core.rs`) | metadata from meta tags and the tree |
| `json_metadata.py` | `json_metadata.rs`, `json_metadata/author.rs` | exists: `extract_json`, `process_parent`, `extract_json_parse_error`, `extract_json_author`, `normalize_json`, `normalize_authors`, `is_plausible_sitename` and the author, publisher and type patterns; an upstream exception becomes `JsonFailure::Raised`, which the caller treats as `extract_metadata`'s `except Exception` does (D-19 for a value that is no string) | metadata from JSON-LD |
| `deduplication.py` | `deduplication.rs`, `deduplication/cache.rs` | exists: `content_fingerprint`, `sample_tokens`, `Simhash` (`validate`, `similarity`), `generate_bow_hash`, `duplicate_test` and the caller-owned `DedupCache` (D-22 to D-26); `is_similar_domain` is not ported, its callers in `feeds.py` and `sitemaps.py` are out of the port | the duplicate test, simhash, content fingerprint and the duplicate cache |
| `utils.py` | `utils.rs`, `utils/text.rs` | partial: decoding and HTML loading, the text helpers (`trim`, `sanitize`, `textfilter`, `line_processing` and the rest), `image_src`, `is_image_file`, `is_acceptable_length` and `check_html_lang` exist; `safe_base_url` and `safe_relative_url` (over `courlan`) exist, `language_filter` and `language_classifier` are stubs (OI-4), `normalize_unicode` (NFC, over `pyunicodedata`) and `as_list` (over `pyjson`) exist | text normalization, trimming, language and size checks, decoding and HTML loading |
| `xml.py` | `xml.rs`, `xml/markdown.rs`, `xml/to_text.rs` | partial: the tree helpers (`delete_element`, `separates_inline`, `remove_empty_elements`, `keeps_empty`, `is_padding_cell`, `merge_with_parent`, `strip_double_tags`) and the TXT and Markdown output (`replace_element_text`, `xml_to_text`; the escaping, math and link helpers are in `xml/markdown.rs`) exist; `META_ATTRIBUTES` and `meta_items` exist; the XML, XML-TEI, JSON and CSV output and the TEI checks are planned (stubs `control_xml_output`, `xml_to_csv` and `build_json_output` return an empty string until phase 6) | tree helpers and output formats |
| `meta.py` | none | not needed | `reset_caches` clears module-global caches here and in htmldate and courlan; this crate has no global caches (Runtime contract) |
| `downloads.py`, `feeds.py`, `sitemaps.py`, `spider.py`, `cli.py`, `cli_utils.py` | none | out of scope | fetching and crawling belong to the caller |
| none | `dom/` (`mod.rs`, `parse.rs`, `parse/sink.rs`, `scan.rs`, `mutate.rs`, `walk.rs`, `serialize.rs`, `dump.rs`) | exists | infrastructure upstream gets from lxml: the arena tree with many roots, its html5ever tree sink (`dom/parse/sink.rs`), the tag scanner (`TagScanner`, `dom/scan.rs`) that counts the attributes of each tag before html5ever tokenizes it (D-5), the lxml-semantics edits (`mutate.rs`: moves, `remove`, `clear`, `strip_tags`, `strip_elements`), the walks and `copy` (`walk.rs`), the serializer (`serialize.rs`: `Tree::tostring`, lxml's `etree.tostring` with `pretty_print` and `with_tail`, one iterative walk that indents two spaces a level up to 60 spaces and writes a foreign element by its local name, D-2), the tree dump the parity harness compares |
| none | `pyurl.rs`, `pyurl/` | exists | CPython's `urllib.parse` (`urlsplit`, `urlunsplit`, `urljoin`, `SplitResult.hostname` and `port`, `quote`, `quote_plus`, `parse_qs`, `urlencode` with `doseq`) and `html.unescape`, which the URL helpers of `courlan` call; `unescape` calls `pyhtml::unescape`, mapping its error to `UrlError`; `pyurl/tables.rs` (the NFKC netloc breakers) is generated by `tools/parity/parity/gen_url.py` |
| none | `pyidna.rs`, `pyidna/` | exists | CPython's `idna` codec, decode direction for one label (`decode_label`, `ToUnicode` over `punycode`), which `courlan`'s `decode_punycode` calls; nameprep is left out (D-16). Its cases are in `pyurl/cases.rs`, generated by `tools/parity/parity/gen_url.py` |
| none | `pyhtml.rs` | exists | CPython's `html.unescape` (`unescape`) over the entity table of `web_atoms`, which the output calls; a decimal reference of more than `MAX_STR_DIGITS` digits fails with `Rejection::InvalidValue`. `pyurl::unescape` calls it |
| none | `pystr.rs` | exists | Python `str` semantics Rust lacks: `isspace`, `split`, `strip`, `lstrip`, `rstrip`, `splitlines`, `isprintable`, `lower` (final sigma included), `title`, `isupper` (`is_upper`), `isalnum` (`is_alnum`), punctuation (`is_punctuation`, the `P*` categories), the `unicode_escape` decode (`unicode_escape`, D-20), `isdecimal` (`is_decimal`), `isdigit` (`is_digit`), `unicodedata.decimal` (`decimal_value`), `int(str)` as its canonical decimal string (`int_str`) and the `\w` class, plus `MAX_STR_DIGITS`, the digit limit of `int()`; its tables (`pystr/printable.rs`, `pystr/alpha.rs`, `pystr/alnum.rs`, `pystr/lower.rs`, the lowercase mapping and the two classes of its final-sigma rule, `pystr/title.rs`, and `pystr/decimal.rs`, the run starts of the `Nd` digits and the `isdigit` ranges) are generated by `tools/parity/parity/gen_printable.py` and `gen_decimal.py` |
| none | `pydatetime.rs`, `pydatetime/` | exists | CPython's `datetime` and `strptime` pieces htmldate calls: `DateTime::new`, `from_isoformat` (the C parser, which `_pydatetime` does not match), `strptime_ymd`, `from_timestamp_utc` (the Linux range), `timestamp_micros` (a naive value read as UTC, D-21), `strftime_ymd` and `strftime_reference`; the table `pydatetime/cases.rs` is generated by `tools/parity/parity/gen_htmldate_cases.py` |
| `htmldate/settings.py` | `htmldate/settings.rs` | exists | `MIN_DATE` (1995-01-01, naive), the candidate and segment bounds, and `CLEANING_LIST`; `CACHE_SIZE` and `MAX_FILE_SIZE` belong to the lru caches and the download, which the port does not have |
| `htmldate/utils.py` | `htmldate/utils.rs` | exists | `Extractor`, `trim_text`, `clean_html` (a live walk, `LiveIter`, that keeps tails), `remove_if_attached`, and `Pattern` with Python's `findall` and `search`; `NotAfter` stands in for a lookbehind, which `regex` lacks; the decoding and loading helpers are not on the path |
| `htmldate/extractors.py` | `htmldate/extractors.rs` | exists | the string extractors: `try_date_expr` (its digit and discard gates, then `custom_parse`), `regex_parse` (the `MONTH_WORD` lookbehind is checked in code), `extract_url_date`, `idiosyncrasies_search` (60-code-point probe windows and the 1,000-core cap), `pattern_search` and the patterns; `_external_date` gives no date (D-13); the table `htmldate/extractor_cases.rs` is generated by `tools/parity/parity/gen_htmldate_cases.py` |
| `htmldate/validators.py` | `htmldate/validators.rs` | exists | date validation and conversion (`is_valid_date_str`, `validate_and_convert`), the reference comparison (`compare_values`, `update_reference`), the year and YMD filters (`plausible_year_filter` with `Counter`, `filter_ymd_candidate`), and the search bounds (`get_max_date`, D-21); the table `htmldate/validator_cases.rs` is generated by `tools/parity/parity/gen_htmldate_cases.py` |
| `htmldate/core.py` | `htmldate/core.rs`, `htmldate/core/patterns.rs`, `htmldate/core/search.rs`, `htmldate/core/examiners.rs` | exists | `find_date`, `search_page`, the examiners; the tables `htmldate/search_cases.rs`, `htmldate/examine_cases.rs` and `htmldate/find_date_cases.rs` are generated by `tools/parity/parity/gen_htmldate_cases.py` |
| none | `api.rs` | exists | the public API (Public API): `Options`, `Extractor`, `Input`, `Format` and `html2txt` over `extraction.rs` and `baseline.rs` |
| none | `pyjson.rs` | exists | Python's `json.loads` (with and without `strict`) over a flat document (`try_loads` tells a `JSONDecodeError` from the other exceptions of the parse; `Json::is_truthy` is Python's truthiness of a node), and `dumps_str`, the string part of `json.dumps(ensure_ascii=False)`; written in the crate because `serde_json` differs from CPython's decoder where it changes which JSON-LD scripts parse |
| none | `pyunicodedata.rs`, `pyunicodedata/` | exists | `unicodedata.normalize("NFC")` (`normalize_nfc`); `pyunicodedata/nfc.rs` is generated by `tools/parity/parity/gen_nfc.py` |
| none | `context.rs` | exists | `Ctx`, the per-call context ported functions take where upstream passes `options` (Runtime contract) |
| none | `cancel.rs`, `error.rs` | exists | `CancelToken` and `Call` (its optional `DedupCache`, set by `Call::dedup`, D-22), `ExtractError` and `Rejection` |
| none | `async_api.rs` | planned | `extract_async` (feature `tokio`) |

Unit tests (`src/**/tests.rs` or `src/**/tests/`) and generated case tables (`src/**/*cases.rs`) sit beside their module
because they reach private items; `#[cfg(test)]` keeps them out of the library build. Integration tests
under `tests/` reach internals through `__internals`. The generated case tables of the integration targets sit in `tests/<target>/cases.rs`.

The hidden feature `__internals` exposes, for the crate's own integration tests only and unstable, the tree types
(`LiveIter` included), `parse_document`, `parse_fragment`, `dump`, `dump_from`, `ParseLimits`, `Ctx`, the
`settings` types and element lists, the `utils` functions, the `html_processing` functions and constants,
the `main_extractor` handlers, `span`, `prune_unwanted_sections`, `recover_wild_text`, `extract_content`
and `extract_comments`, the `baseline` functions, the `extraction` entry points (`Source`,
`bare_extraction`, `internal_extraction`, `determine_returnstring`, `yaml_scalar`), the `xml` functions
(`delete_element`, `remove_empty_elements`, `merge_with_parent`, `strip_double_tags`,
`replace_element_text`, `xml_to_text`, `md_link`), the `json_metadata` functions (`extract_json`, `extract_json_author`,
`extract_json_parse_error`, `normalize_authors`, `normalize_json`, `process_parent`, `JSON_AUTHOR_1`,
`JSON_AUTHOR_2`), the `metadata` functions (`extract_metadata`, `extract_meta_json`, `extract_metainfo`,
`extract_title`, `extract_url`, `normalize_tags`, `check_authors`, `JSON_MINIFY`), `set_date_params`,
`DateParams`, the `deduplication` items `HashInput`, `Simhash`, `content_fingerprint`, `sample_tokens`, `duplicate_test` and `generate_bow_hash`,
`pyhtml::unescape`, `pyjson` (`loads`, `dumps_str`, `Json`, `JsonRef`, `Kind`),
`pystr::{PY_WORD_CLASS, is_alpha, repr, splitlines}`, `pyunicodedata::normalize_nfc` and the `xpaths` items (`Selector`, `alt`,
`BODY_XPATH`, `OVERALL_DISCARD_XPATH`, `PRECISION_DISCARD_XPATH`). `Document` has accessors for its tree under
it (`body_tree`, `body`, `comments_body`).

Upstream's own dependencies carry extraction logic, so the parts trafilatura calls are ported with it:

| Package | Handling | Status |
| --- | --- | --- |
| `htmldate` (date extraction) | ported as a module named `htmldate` | exists: `find_date` (`htmldate/core.rs`) runs upstream's stages on a copy of the tree; its inputs are `DateParams` (`max_date` from the caller) and the D-21 clock |
| `courlan` (URL normalization and filtering) | ported as a module named `courlan` | partial: `get_base_url`, `fix_relative_urls`, `get_tldinfo` and `extract_domain` (`courlan.rs`), `validate_url` and `is_valid_url` (`courlan/filters.rs`) and `normalize_url` (`courlan/clean.rs`) exist; `tld`'s `get_tld` is `courlan/tld.rs` over the `psl` crate, with `courlan/tld_tables.rs` generated; the other courlan functions are not ported |
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
  travels inside the per-call context (`Ctx`, `context.rs`: the `Extractor` as a `Cow`, so a ported
  function that edits a copy of upstream's options clones it, plus `&Call`) passed where upstream
  passes `options`, and is checked at stage
  boundaries and inside every loop over nodes; a ported function that checks it returns `Result`.
  The `dom` primitives (edits, walks, `copy`, `tostring`) are linear in the subtree and take no `Call`; a ported
  function that loops over nodes checks it itself, and `convert_tags`, which runs several walks in a
  row, checks it before each one. A single `dom` walk runs unchecked; its bound is `max_nodes` (D-5).
  The handle is `Call` (`cancel.rs`): an optional borrowed `CancelToken` (`Arc<AtomicBool>`, no
  dependency) and an optional deadline `Instant`. Loops check both about every 256 nodes
  (`CHECK_INTERVAL`: one atomic load and one `Instant::now()`), a constant tuned by benchmarks; cancel
  gives `ExtractError::Cancelled`, the deadline `ExtractError::DeadlineExceeded`. A check only returns
  early with a distinct error; it never changes a branch, so a run that completes gives the same
  output as one with no token and no deadline. `Call` also carries `max_date`, which sets the latest
  date the metadata accepts, and an optional duplicate cache (`Call::dedup`): with a cache, a completed
  run's output depends on the cache's state (D-22).
- Bounded. `MAX_FILE_SIZE` and `MAX_TREE_SIZE` are ported exactly as upstream applies them
  (`MAX_TREE_SIZE` counts the children of the output body, strips `hi` first, then rejects; empty
  means no limit). Nesting depth stops at 256 as libxml2 does (`MAX_DEPTH` in `dom/parse.rs`): parsing
  ends at the cut and the partial tree is kept. That is upstream behavior, always on. Limits the port
  adds are options: `max_input_bytes` (`ParseLimits::max_input_bytes`: raw input length, checked first in
  `load_html` before decompression) is on by default at `DEFAULT_MAX_INPUT_BYTES`, 64 MiB (D-11), and a
  caller may lift it with `None`; `max_nodes` (`ParseLimits::max_nodes`: elements created
  while parsing, html5ever's clones included) is on by default at `DEFAULT_MAX_NODES` (D-5), and a
  caller may lift it with `None`. Attributes per tag, and merged on `html` or `body`, are capped at
  `MAX_TAG_ATTRIBUTES` (`dom/parse.rs`), always on, while the attributes of a whole parse
  (`ATTRIBUTES_PER_NODE` per allowed node, clones included) and html5ever's work on formatting
  elements, compares and clones (`COMPARE_WORK_PER_NODE` per allowed node) are budgeted from `max_nodes` and lifted with it (D-5); the parse stops at the first token after a budget is crossed.
  Table construction is bounded by the cells
  it mints: it fails with `Rejection::TreeTooLarge` once it has minted `MAX_OUTPUT_NODES` of them in
  one tree (D-9). Recursion only where `MAX_DEPTH` bounds its depth, with a test that proves it fits a
  512 KiB stack in a debug build (the list in `scripts/test.sh`), so a default thread stack is enough (a musl host that spawns its own
  threads sets 512 KiB).
- No hidden clock. Upstream sets `max_date` from `datetime.now()` (`settings.py`). Here a call sets it
  with `Call::max_date`; without one, `extract_metadata` takes today's UTC date, read once per call
  and never when the extractor is built (D-15).
- Panic-free on any input. The crate runs behind FFI, where a panic aborts the host process.
  `unwrap`, `expect` and `panic` fail clippy outside tests; `string_slice` fails everywhere, tests
  included; `indexing_slicing` is a warning, and CI denies warnings. `clippy.toml` exempts only
  `#[test]` and `#[cfg(test)]` code, so a shared helper under `tests/` starts with the `allow` line
  given in `AGENTS.md` (Runtime rules). The `fuzz/` targets (`scripts/fuzz.sh`) check it on arbitrary
  input, weekly in `quality.yml`.
  Clippy does not catch arithmetic overflow, division by zero or a `RefCell` double borrow: use
  checked arithmetic (Exceptions in the porting rules of `AGENTS.md`) and never let two `RefCell`
  borrows overlap.
- Every `pub` item has a doc comment: `missing_docs` warns and CI denies warnings.
- No process-global mutable state. Anything upstream keeps in a module global is an explicit value the
  caller owns. `Send + Sync` options and results, so concurrent calls need no locking of their own. A
  `DedupCache` shared by calls takes a lock in each of its methods (D-23 for what they see).
- `unsafe` is forbidden.

## Public API

The public API is this crate's own and mirrors upstream's `extract` and `bare_extraction`; the pipeline
under it follows upstream function by function. It lives in `api.rs` and is re-exported at the crate
root with `Call`, `CancelToken`, `ExtractError`, `Rejection`, `Config`, `Focus`, `Document`, `DedupCache` and
`DedupError`.

| Item | Contract |
| --- | --- |
| `Options` | Builder over upstream's defaults; each method takes and returns `Self`, `build(&self)` makes an `Extractor`. |
| `Extractor` | Immutable, `Clone`, `Send + Sync`: one value serves any number of threads and URLs. `extract(&self, &Input, &Call) -> Result<String, ExtractError>` is upstream `extract`; `bare_extraction(&self, &Input, &Call) -> Result<Document, ExtractError>` is upstream `bare_extraction`. |
| `Input<'a>` | `Input::bytes(&[u8])` or `Input::text(&str)`, with `.url(&str)` and `.record_id(&str)`. The URL belongs to the call: the call's `Ctx` borrows the stored settings, or owns a copy with `url` set when the input has one. |
| `Format` | `Txt`, `Markdown`, `Html`, `Python`; non-exhaustive. Maps onto `settings::Format`, which also holds the XML, XML-TEI, JSON and CSV variants of a later phase. |
| `Focus`, `Config` | `Focus` is `Balanced`, `Precision` or `Recall`, in place of upstream's two `favor_*` flags, so upstream's recall-wins resolution cannot arise. `Config` holds the values of `settings.cfg`; `Config::default()` is the cfg file. |
| `Document` | Non-exhaustive result: the metadata fields (`Option<String>`; `categories` and `tags` `Option<Vec<String>>`), `text`, `comments` and `raw_text`. It owns the body and comments body as a compact tree of its own, so the call's arena is freed on return. |
| `html2txt(&Input, clean, &Call)` | Upstream `html2txt`. An empty document gives `Ok("")`; `TreeTooLarge` and cancellation are errors. |

`Options` defaults and what they map to:

| Method | Default | Notes |
| --- | --- | --- |
| `format` | unset | Resolved per entry point as upstream's defaults are: `extract` takes `Txt`, `bare_extraction` takes `Python`. A set format applies to both. `extract` with `Python` fails with `Rejected(InvalidValue)` after a successful extraction, upstream's `ValueError`. |
| `focus` | `Balanced` | |
| `comments` | `true` | |
| `formatting` | unset | Resolved as upstream: Markdown implies it, other formats do not. |
| `links`, `images` | `false` | |
| `tables` | `true` | |
| `target_language` | none | Two-letter code; with the language classifier a stub (OI-4) only the `html` `lang` check rejects. |
| `config` | `Config::default()` | |
| `max_input_bytes` | `Some(DEFAULT_MAX_INPUT_BYTES)` | `ParseLimits::max_input_bytes` (D-11): raw input length above which `Rejection::InputTooLarge` is returned, before decompression; `None` lifts the limit. The free `html2txt` always uses the default. |
| `max_nodes` | `Some(DEFAULT_MAX_NODES)` | `ParseLimits::max_nodes` (D-5); `None` lifts the limit. `ParseLimits::max_depth` is not exposed. |
| `with_metadata` | `false` | Metadata goes into the `Document` of `bare_extraction` and into the header of the text, Markdown and HTML outputs. A URL or author blacklist, or `only_with_metadata`, turns it on. |
| `only_with_metadata` | `false` | Rejects a page without a date, title or URL with `Rejected(MissingMetadata)`. Implies `with_metadata`. |
| `author_blacklist` | empty | Names dropped from the author, matched without regard to case; replaces the set. A non-empty set implies `with_metadata`. |
| `url_blacklist` | empty | URLs rejected with `Rejected(BlacklistedUrl)`; replaces the set. A non-empty set implies `with_metadata`. |
| `keep_script_text` | `false` | Keeps the text of `script` and `style` elements in the parse, as upstream does (D-14). |

`Err(Rejected(_))` stands for upstream's `None` and for a raise that escapes upstream's entry point.
Options that no phase makes work yet are absent, so none silently does nothing: date extraction parameters
(`find_date` runs inside metadata; its `Options` method waits), `prune_xpath` and the XML, XML-TEI, JSON and
CSV formats. Metadata extraction, its filters and its `Options` methods are in the table above.
Deduplication has no `Options` method: its cache is passed on `Call` (D-22). Each absent option gets its
method when its phase lands.

Tree input (upstream's `HtmlElement` for `filecontent`) is internal: `extraction.rs` takes a `Source`,
a page or an element of a caller's tree, which the public methods and the tests that pass a tree use.

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
  semantics, but a copy that is only read afterwards may be a borrow. A real copy is `Tree::copy`:
  one iterative walk of the subtree that appends its nodes to the arena as one contiguous block in
  document order after a single `reserve`, so the copy's k-th node pairs with the original's k-th.
  Text, tail and attributes are copied (attribute values stay shared `Arc<str>`). The arena never
  frees: a removed node and every copy stay until the `Tree` is dropped, so memory grows with each
  copy a call makes.
- Memory is bounded by the input limits: no growth that does not follow input size, no cache that
  outlives a call unless the caller owns it, everything freed when the call returns.
- Borrow the caller's input bytes; decode to a new buffer only when the input is not already UTF-8.
- The library never sets a global allocator; that choice belongs to the binary or binding.
- `Extractor` parses with the text of `style` and `script` elements dropped, except JSON-LD and
  `settings+json` scripts (`ParseLimits::drop_script_text`); the parity tree dump, `html2txt` and
  fragment parses keep that text.
  When such an element survives `tree_cleaning`, `bare_extraction` runs again on a full parse.
