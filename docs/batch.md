# Running many extractions

This page is for a caller that extracts many pages: a service, a crawler or a batch job. It covers
how to share one `Extractor`, how to size the work, and when to use `extract_batch`.

## Share one Extractor

An `Extractor` is immutable, `Clone` and `Send + Sync` (`docs/architecture.md`, Public API). Build it
once from `Options` and pass a reference to every thread. It holds no per-call state, so calls on
one `Extractor` never wait for each other.

## One Call per extraction

Each extraction takes its own `Call`. A `Call` holds an optional cancel token, an optional deadline,
the `max_date` and an optional duplicate cache. `Call` is `Copy`, so a caller can build one per page
and drop it when the page is done. A single `Call` may serve several extractions, but then its
deadline and token cover all of them.

## Shared duplicate cache

A `DedupCache` is the only state that calls share. Pass one cache to each call through `Call::dedup`
when the calls should see each other's texts. Each cache method holds its own lock for the duration
of the call, and a poisoned lock is recovered, so no method panics.

With a shared cache, a completed call's output depends on the cache's state and on the order in
which calls commit (D-22). A call's counts reach the cache when the call returns `Ok` or a rejection.
Another call can commit between a text's read and its own count, so results can change between runs
with threads (D-23). Without a cache, each result equals a lone call on the same input.

## Memory per call

A call's memory is bounded by the input limits: `max_input_bytes` (64 MiB by default, D-11) and
`max_nodes` (D-5). A caller that lifts a limit with `None` takes on the bound itself. A `Document`
owns its text and comments as a compact tree of its own, so the call's working tree is freed when
the call returns. A caller that keeps only the `Document` keeps only that tree.

## Choosing a path

A service that already runs requests on its own pool calls `extract` or `bare_extraction` for each
page on that pool. This adds no threads and keeps one scheduler in charge.

`extract_batch` suits a caller that has a list of pages and no pool. It spawns its own scoped
workers, and every worker ends before the call returns, so no thread outlives the call. Use it with
a small `threads` count when a pool already exists.

Each concurrent `extract_batch` spawns its own workers. Thirty-two concurrent batches at 16 threads
are 512 OS threads. A caller that runs batches in parallel should divide the thread budget among
them, or move the batches onto the pool.

`threads` is capped at 256 (`MAX_WORKERS`) and falls to the iterator's upper size bound when that is
smaller. Results wait in a channel of at most twice the thread count, and at most 512 slots, so
memory grows with the threads and not with the batch.

## Threads, not processes

Upstream's command line runs CPU-bound extraction in a process pool, because CPython's global
interpreter lock blocks threads from running Python code in parallel. This crate has no such lock.
Each extraction works on its own tree and reads the shared `Extractor` only, so threads give the
same isolation as processes without the cost of starting them.

## Deadlines and cancel

The batch deadline and the cancel token stop the pulls. A worker checks both before it pulls an item
and again before it runs one. An item pulled after either fires is reported as `Skipped`, and `op`
does not run for it. `BatchOptions::per_item` adds a deadline per item, counted from the pull.

`next()` of the iterator runs under a lock, one worker at a time. Keep it cheap. Do the file reads
and other I/O inside `op`, so the workers overlap them.

## Collecting results

`on_result` receives each result with its index in the input, and results arrive as their items
finish. With one thread they arrive in input order. A caller who wants a `Vec` writes into a slot
per index:

```rust
if let Some(slot) = out.get_mut(index) {
    *slot = Some(value);
}
```

The slot pattern avoids indexing and a panic on an out-of-range index. Collecting every result gives
up the memory bound of the channel: all results sit in `out` until the batch ends. Keep only what
the caller needs, such as a label or a count, when the pages are many.

## Stack of the workers

Each worker has a stack of `WORKER_STACK_BYTES`, 2 MiB, four times the 512 KiB that the recursion
proofs in `scripts/test.sh` fit. `tests/batch_stack.rs` runs every page of the parity corpus through
`extract_batch` on one worker in the dev profile, where frames are largest:

```sh
PARITY_REQUIRE_FIXTURES=1 cargo test --locked --profile dev --all-features --test batch_stack -- --ignored
```

A stack overflow aborts the process instead of failing the test, so a run that ends without a test
result is the overflow case.

## Example

`examples/batch.rs` extracts the HTML files of a directory with four threads and a 30-second batch
deadline. Run it as `cargo run --example batch -- examples/batch`. It prints one line per file:
`index path outcome chars`.

## Bindings

This crate ships no binding. The rules below are guidance for a binding author.

A binding should run `extract_batch` on a thread it owns, so the calling thread does not block for
the whole batch. A .NET binding should use a thread created with `TaskCreationOptions.LongRunning`,
not a thread-pool thread.

A binding should convert each `Document` into its own form inside `op`, on the workers. The
`on_result` callback should only hand the result to managed code. It should catch every managed
exception, because none may cross a reverse P/Invoke, and cancel the token to stop the batch.

The allocator belongs to the binary or the binding, never to the library. The thread-scaling target
of `scripts/measure.sh --threads-native` measures a choice on the corpus sample.
