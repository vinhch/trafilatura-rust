# trafilatura-rust

A Rust port of [trafilatura](https://github.com/adbar/trafilatura), the Python library that extracts
the main text, comments and metadata of web pages. The goal is the same output as upstream Python at
the pinned release, with a synchronous, cancellable, bounded and panic-free API that is safe to call
from other languages.

The extraction pipeline of upstream's fast mode works end to end: main text and comments from bytes or
a string, as text, Markdown, HTML, XML, XML-TEI, JSON or CSV, or as a `Document` with the page's
metadata. Upstream's unit and filter tests, ported by name, check it, along with cases generated from
upstream Python and a comparison of the text, Markdown, HTML, XML, XML-TEI and JSON output of the
corpus pages with upstream's.

## Installation

```sh
cargo add trafilatura-rust
# or, to accept gzip, zstd, brotli and zlib input:
cargo add trafilatura-rust --features decompress
```

```toml
[dependencies]
trafilatura-rust = { version = "0.1", features = ["decompress"] }
```

The minimum supported Rust version is the `rust-version` in `Cargo.toml`.

## Quick Start

```rust
use trafilatura_rust::{Call, Input, Options};

const HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<title>Council approves the new library</title>
<meta name="author" content="Ada Lovelace">
</head>
<body>
<nav><a href="/">Home</a> <a href="/news">News</a></nav>
<main><article>
<h1>Council approves the new library</h1>
<p>The council approved the new central library on Tuesday, after a debate that ran past midnight and filled every seat in the chamber with residents who had waited four hours to speak.</p>
<p>The building will open in the spring of 2028 on the site of the old bus depot, and the architects said the reading rooms will face the river so that the afternoon light reaches the desks.</p>
</article></main>
<footer><p>Copyright Example Journal. All rights reserved.</p></footer>
</body>
</html>"#;

fn main() -> Result<(), trafilatura_rust::ExtractError> {
    // One Extractor serves any number of calls, on any number of threads.
    let extractor = Options::new().with_metadata(true).build();
    let input = Input::text(HTML).url("https://example.org/news/library");
    let call = Call::default();

    // The main text as a string, in the format of the options (plain text here).
    let text = extractor.extract(&input, &call)?;
    assert!(text.contains("approved the new central library"));
    assert!(!text.contains("All rights reserved"));

    // The same page as a Document: the text plus the metadata fields.
    let document = extractor.bare_extraction(&input, &call)?;
    assert_eq!(document.title.as_deref(), Some("Council approves the new library"));
    assert_eq!(document.author.as_deref(), Some("Ada Lovelace"));
    Ok(())
}
```

## API Reference

The tables name each item and what it is for; [docs.rs](https://docs.rs/trafilatura-rust) has the
full contracts and the default of every option.

### Entry points

| Item | Does |
| --- | --- |
| `Options::build(&self) -> Extractor` | Resolves the options once into an immutable `Extractor`, which is `Send + Sync` |
| `Extractor::extract(&self, &Input, &Call) -> Result<String, ExtractError>` | The main text in the options' format (`Format::Txt` when unset) |
| `Extractor::bare_extraction(&self, &Input, &Call) -> Result<Document, ExtractError>` | A `Document`: `text` and `comments` with `Format::Python` (the default here), `raw_text` with any other format |
| `Extractor::extract_batch(items, BatchOptions, &Call, op, on_result) -> Result<BatchEnd, BatchError>` | Runs `op` on each item on scoped worker threads and hands each result, with its index, to `on_result` on the calling thread |
| `html2txt(&Input, clean: bool, &Call) -> Result<String, ExtractError>` | The whole `body` text with word breaks at block tags, `clean` removing undesirable elements first |

### Input and Call

| Item | Does |
| --- | --- |
| `Input::bytes(&[u8])` | A page as raw bytes in an unknown encoding; compressed bytes need the `decompress` feature |
| `Input::text(&str)` | A page as text |
| `Input::url(&str)` | The page's URL, which resolves relative links; it belongs to the call, so one `Extractor` serves many sites |
| `Input::record_id(&str)` | The record id, which `extract` writes to its output: the metadata header of the text, Markdown and HTML formats, the `id` field of the others |
| `Call::cancel(&CancelToken)` | Stops the call with `ExtractError::Cancelled` once the token is cancelled |
| `Call::deadline(Instant)` | Stops the call with `ExtractError::DeadlineExceeded` once the instant has passed |
| `Call::max_date(&str)` | The latest date the metadata accepts, `YYYY-MM-DD`; without it, today's UTC date |
| `Call::dedup(&DedupCache)` | Counts repeated texts in a cache that later calls share (upstream's `deduplicate`) |
| `CancelToken::{new, cancel, is_cancelled}` | A flag any thread sets; every clone sees it |

### Options

`Options::new()` starts from upstream's defaults plus the crate's own limits (`max_nodes`, `max_input_bytes`: D-5, D-11); each setter takes and returns the options.

| Setter | Sets |
| --- | --- |
| `format(Format)` | The output format of both entry points |
| `focus(Focus)` | The trade-off between text kept and noise let in: `Balanced`, `Precision` or `Recall` |
| `comments(bool)` | Whether comments are extracted along with the main text |
| `formatting(bool)` | Whether headings, emphasis and lists are kept; unset, Markdown keeps them and the other formats do not |
| `links(bool)` | Whether links are kept with their targets |
| `images(bool)` | Whether images are kept |
| `tables(bool)` | Whether tables are kept |
| `target_language(impl Into<String>)` | A two-letter code; pages in another language are rejected |
| `config(Config)` | The values of upstream's `settings.cfg` |
| `max_nodes(Option<usize>)` | The element count above which a page is rejected while parsing; `None` lifts it |
| `max_input_bytes(Option<usize>)` | The raw input length above which a page is rejected before any work; `None` lifts it |
| `with_metadata(bool)` | Whether metadata is extracted into the `Document` and the outputs |
| `only_with_metadata(bool)` | Whether a page without a date, a title or a URL is rejected; implies metadata |
| `author_blacklist(authors)` | Authors dropped from the metadata; a non-empty set implies metadata |
| `url_blacklist(urls)` | URLs of pages to reject; a non-empty set implies metadata |
| `keep_script_text(bool)` | Whether the text of `script` and `style` elements is kept in the parse |

### Format

| Variant | Output |
| --- | --- |
| `Txt` | Plain text |
| `Markdown` | Markdown, with formatting unless `formatting(false)` |
| `Html` | HTML |
| `Xml` | XML, as upstream's `control_xml_output` writes it |
| `XmlTei` | TEI XML with a `teiHeader`; forces metadata on |
| `Json` | JSON, as upstream's `build_json_output` writes it |
| `Csv` | One tab-separated row of the metadata fields, the text and the comments |
| `Python` | The `Document` with `text` and `comments` as strings; `bare_extraction` only |

### Config

`Config` holds upstream's `settings.cfg` keys as public fields; build one with
`Config { min_extracted_size: 500, ..Config::default() }`.

| Field | Means |
| --- | --- |
| `min_extracted_size` | Shortest extracted text, in characters, that skips the recall fallback |
| `min_output_size` | Shortest main text, in characters, that is kept |
| `min_output_comm_size` | Shortest comments text, in characters, that is kept |
| `min_duplcheck_size` | Shortest text, in characters, that the duplicate test looks at |
| `max_repetitions` | Repetitions after which a text counts as a duplicate |
| `max_file_size` | Largest decompressed size, in bytes, of byte input (feature `decompress`) |
| `min_file_size` | Upstream's `MIN_FILE_SIZE`; no extraction path reads it |
| `max_tree_size` | Element count of the extracted body above which a page is discarded; `None` = no limit |
| `extensive_date_search` | Whether the date search runs its free-text and page-text passes |

### Document

Every field is an `Option`, and metadata fields are set only when metadata is extracted.

| Field | Holds |
| --- | --- |
| `title`, `author`, `description`, `sitename` | Strings from the page |
| `url`, `hostname` | The page's URL and its host |
| `date` | Publication date, `YYYY-MM-DD` |
| `categories`, `tags` | `Vec<String>` |
| `fingerprint`, `id` | Content fingerprint and record id; only `extract` computes them, for its outputs, so they are `None` from `bare_extraction` |
| `license`, `language`, `image`, `pagetype` | License, page language, main image, page type |
| `text`, `comments` | Main text and comments, with `Format::Python` only |
| `raw_text` | The main text before the conversion to the output format |
| `filedate` | Date of the file: the call's `max_date`, else today's UTC date |

### Errors

`ExtractError` is `Rejected(Rejection)`, `Cancelled` or `DeadlineExceeded`. A rejection stands for
upstream's `None` result, and its variant says which check fired:

| `Rejection` | When |
| --- | --- |
| `EmptyTree` | The document has no usable tree |
| `WrongHtmlLang` | The `html` element's `lang` does not match `target_language` |
| `BlacklistedUrl` | The URL is in `url_blacklist` |
| `MissingMetadata` | `only_with_metadata` is set and a date, title or URL is missing |
| `TreeTooLarge` | A node limit is exceeded: `max_nodes` while parsing, `Config::max_tree_size` on the extracted body, or the cell budget of a table |
| `TooShort` | The extracted text is under the `Config` minimums |
| `Duplicate` | A shared `DedupCache` has seen the text more than `max_repetitions` times |
| `WrongLanguage` | The detected language does not match `target_language` |
| `InputTooLarge` | The raw input exceeds `max_input_bytes`, or a JSON node count or offset does not fit 32 bits |
| `InvalidValue` | A value upstream raises on, such as `Format::Python` passed to `extract` |

## Advanced Usage

### Cancellation and deadlines

Every call carries a `Call`. A `CancelToken` stops it from another thread, a deadline stops it at an
instant; the pipeline checks both at stage boundaries and inside its loops over nodes.

```rust
use std::time::{Duration, Instant};
use trafilatura_rust::{Call, CancelToken, ExtractError, Input, Options};

let extractor = Options::new().build();
let token = CancelToken::new();
let call = Call::default()
    .cancel(&token)
    .deadline(Instant::now() + Duration::from_millis(500));

token.cancel(); // from any thread holding a clone
let result = extractor.extract(&Input::text("<html><body><p>Hi</p></body></html>"), &call);
assert!(matches!(result, Err(ExtractError::Cancelled)));
```

### One Extractor, many threads

`Extractor` is immutable and `Send + Sync`: build it once and share it by reference.

```rust
use trafilatura_rust::{Call, Input, Options};

let extractor = Options::new().build();
let pages = ["<html><body><p>One</p></body></html>", "<html><body><p>Two</p></body></html>"];
std::thread::scope(|scope| {
    for page in pages {
        let extractor = &extractor;
        scope.spawn(move || extractor.extract(&Input::text(page), &Call::default()));
    }
});
```

### Batches

`Extractor::extract_batch` runs a closure over an iterator on a fixed number of scoped worker
threads, under one `Call` for the whole batch and an optional deadline per item. Results reach a
callback on the calling thread, with each item's index.

```rust
use std::num::NonZeroUsize;
use trafilatura_rust::{BatchEnd, BatchOptions, Call, Input, Options};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let extractor = Options::new().build();
    let pages = vec!["<html><body><p>One</p></body></html>"; 8];
    let threads = NonZeroUsize::new(4).ok_or("thread count is zero")?;
    let mut outcomes = vec![None; pages.len()];

    let end = extractor.extract_batch(
        pages.iter(),
        BatchOptions::new(threads),
        &Call::default(),
        |page, call| extractor.extract(&Input::text(page), call).is_ok(),
        |index, result| {
            if let Some(slot) = outcomes.get_mut(index) {
                *slot = result.ok();
            }
        },
    )?;
    assert_eq!(end, BatchEnd::Completed);
    assert!(outcomes.iter().all(Option::is_some));
    Ok(())
}
```

[`docs/batch.md`](https://github.com/vinhch/trafilatura-rust/blob/main/docs/batch.md) covers thread
counts, deadlines, memory bounds and running batches from a host's own pool.

### Deduplication

Deduplication is on when a call carries a `DedupCache`; there is no option for it. Calls that share
the cache count each text they see, and a text seen more than `Config::max_repetitions` times is
rejected.

```rust
use trafilatura_rust::{Call, DedupCache, ExtractError, Input, Options, Rejection};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let extractor = Options::new().build();
    let cache = DedupCache::new(4096)?;
    let page = format!("<html><body>{}</body></html>", "<p>abc</p>".repeat(50));
    let call = Call::default().dedup(&cache);

    for _ in 0..3 {
        assert!(extractor.extract(&Input::text(&page), &call).is_ok());
    }
    let fourth = extractor.extract(&Input::text(&page), &call);
    assert!(matches!(fourth, Err(ExtractError::Rejected(Rejection::Duplicate))));
    Ok(())
}
```

### Compressed input

With the `decompress` feature, `Input::bytes` accepts gzip, zstd, brotli and zlib payloads as they
come off the wire, and `Config::max_file_size` bounds the decompressed size.

```rust,no_run
use trafilatura_rust::{Call, Input, Options};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bytes = std::fs::read("page.html.gz")?;
    let text = Options::new().build().extract(&Input::bytes(&bytes), &Call::default())?;
    Ok(())
}
```

### Reproducible dates

The date search rejects dates after a latest date, which defaults to today's UTC date, so the same
page can give a different `date` on another day. `Call::max_date` fixes it per call.

```rust
use trafilatura_rust::Call;

let call = Call::default().max_date("2026-12-31");
```

## Scope and limitations

- Fast mode only. The port covers upstream's `fast=True` path; the readability and jusText fallbacks
  that upstream runs by default are not ported yet
  ([D-1](https://github.com/vinhch/trafilatura-rust/blob/main/docs/deviations.md)). When extraction
  finds nothing usable it returns a `Rejection` that says why, and a caller that wants a fallback runs
  its own extractor on that error.
- Bounded input. Besides upstream's limits, `max_nodes`, `max_input_bytes`, a nesting depth of 256
  and 1,000 attributes per tag bound each call; the
  [deviations](https://github.com/vinhch/trafilatura-rust/blob/main/docs/deviations.md) list every
  difference from upstream.
- No I/O. The crate fetches nothing and writes nothing; it takes a page you already have.
- Threads. Only `extract_batch` starts threads, and none outlives the call.

The [roadmap](https://github.com/vinhch/trafilatura-rust/blob/main/docs/phase-plan.md) lists what
comes next.

## Why another Rust port

Other Rust ports exist, but most translate the Go port, which follows an older Python release. This
one ports Python directly:

- Upstream tracking: one Rust module per Python module, each function names its Python source, and
  `scripts/upstream.sh` lists upstream changes since the pinned release.
- Parity: output is compared with upstream Python output, not a benchmark score; intended differences
  are listed in `docs/deviations.md`.
- Production safety: cancellation, deadlines and input limits in the API, no panics, few dependencies.

Is it better? `scripts/measure` times `extract` per page on both sides and prints the Python / Rust
comparison (`docs/parity-harness.md`); the numbers depend on the machine, so run it on yours.
The bet is that staying close to upstream makes each Python release cheaper to follow.

## Documents

- [`docs/architecture.md`](https://github.com/vinhch/trafilatura-rust/blob/main/docs/architecture.md): module map, runtime contract, public API, performance rules.
- [`docs/batch.md`](https://github.com/vinhch/trafilatura-rust/blob/main/docs/batch.md): running many extractions: sharing an `Extractor`, `extract_batch`, thread counts.
- [`docs/gotchas.md`](https://github.com/vinhch/trafilatura-rust/blob/main/docs/gotchas.md): traps found reading upstream.
- [`docs/upstream-sync.md`](https://github.com/vinhch/trafilatura-rust/blob/main/docs/upstream-sync.md): how to move to a new upstream release.
- [`docs/parity-harness.md`](https://github.com/vinhch/trafilatura-rust/blob/main/docs/parity-harness.md): fixtures, mismatch classes, measurement.
- [`docs/dependencies.md`](https://github.com/vinhch/trafilatura-rust/blob/main/docs/dependencies.md): the dependency policy and why each dependency is there.
- [`docs/deviations.md`](https://github.com/vinhch/trafilatura-rust/blob/main/docs/deviations.md): every departure from upstream behavior.
- [`docs/development.md`](https://github.com/vinhch/trafilatura-rust/blob/main/docs/development.md): prerequisites, scripts, CI and releasing.

## Development

The scripts, docs and workflows named here live in the
[GitHub repository](https://github.com/vinhch/trafilatura-rust), not in the published package.
`scripts/test.sh` (or `scripts/test.ps1`) runs every check the CI test job runs.
[`docs/development.md`](https://github.com/vinhch/trafilatura-rust/blob/main/docs/development.md)
lists the prerequisites, the scripts, the workflows and the release steps, and `AGENTS.md` holds the
porting rules.

## Credits

[adbar/trafilatura](https://github.com/adbar/trafilatura) defines the extraction this crate follows.

## License

Apache-2.0, as upstream. See `LICENSE` and `NOTICE`.
