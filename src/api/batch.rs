//! Batch extraction: many pages on scoped worker threads, with a bounded channel of results and a
//! shutdown that stops pulling on a cancel or a deadline.

use std::io;
use std::num::NonZeroUsize;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::thread::{self, Builder, Scope, ScopedJoinHandle};
use std::time::{Duration, Instant};

use super::Extractor;
use crate::cancel::Call;
use crate::error::ExtractError;

/// Most workers one batch spawns, whatever `threads` asks for.
pub(super) const MAX_WORKERS: usize = 256;

/// Most results the channel holds between the workers and the calling thread.
const MAX_CHANNEL: usize = 1024;

/// Stack of each worker: four times the 512 KiB that the recursion proofs in `scripts/test.sh` fit.
const WORKER_STACK_BYTES: usize = 2 * 1024 * 1024;

/// Settings of [`Extractor::extract_batch`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BatchOptions {
    /// Worker threads to spawn: at most 256, and fewer when the iterator's `size_hint` gives a
    /// smaller upper bound. The caller chooses this; there is no default.
    pub threads: NonZeroUsize,
    /// Deadline of each item, counted from the moment a worker pulls it. `None` gives items only
    /// the batch deadline.
    pub per_item: Option<Duration>,
}

impl BatchOptions {
    /// Options with `threads` worker threads and no per-item deadline.
    pub fn new(threads: NonZeroUsize) -> Self {
        Self {
            threads,
            per_item: None,
        }
    }

    /// Sets the deadline of each item, counted from the moment a worker pulls it.
    pub fn per_item(mut self, per_item: Duration) -> Self {
        self.per_item = Some(per_item);
        self
    }
}

/// Why an item was pulled but not run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Skipped {
    /// The batch's `CancelToken` was cancelled before the item ran.
    Cancelled,
    /// The batch deadline passed before the item ran.
    DeadlineExceeded,
}

/// How a batch ended, when it did not fail.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BatchEnd {
    /// The iterator ended, and every item it yielded got a callback.
    Completed,
    /// A cancel or the batch deadline stopped the pulls before the iterator ended.
    Stopped,
}

/// Why a batch failed.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BatchError {
    /// A worker thread could not be started. The items already pulled still ran and reached the
    /// callback. The error kind is kept, since an `io::Error` is neither `Clone` nor `Eq`.
    Spawn(io::ErrorKind),
    /// `op` or the iterator panicked on a worker. Every other item still got its callback.
    WorkerPanicked,
}

impl std::fmt::Display for BatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Spawn(kind) => write!(f, "batch worker could not start: {kind:?}"),
            Self::WorkerPanicked => f.write_str("a batch worker panicked"),
        }
    }
}

impl std::error::Error for BatchError {}

impl Extractor {
    /// Extracts each item of `items` with `op` on `options.threads` scoped worker threads, and
    /// hands each result to `on_result` on the calling thread, with the item's index: its position
    /// in `items`. Results arrive as their items finish, so with one thread they arrive in input
    /// order. Every worker ends before the call returns.
    ///
    /// `op` gets the item by value and a `Call` for it: the batch's `Call` (cancel token, batch
    /// deadline, `max_date`, duplicate cache) with the deadline cut to `per_item` after the item was
    /// pulled, when that comes first. Before each pull a worker checks the token and the batch
    /// deadline and stops pulling when either has passed, so an endless iterator ends at a cancel.
    /// An item pulled when the cancel or deadline arrives gets `Err(Skipped)` without running `op`.
    /// Finished results wait in a channel of at most `2 * threads` (up to 1,024) slots, so memory
    /// stays bounded by the threads, not by the batch.
    ///
    /// `next()` of `items` runs under a lock, one worker at a time, so it must be cheap and must
    /// not block on what `on_result` or `op` produce. Keep the I/O in `op`. Each concurrent call
    /// spawns its own workers, so 32 concurrent batches at 16 threads are 512 threads.
    ///
    /// With a shared `DedupCache`, a result depends on the order in which items run and commit
    /// (D-23); without one, each result equals a lone call of `op` on its item.
    ///
    /// Returns `Ok(BatchEnd::Stopped)` when a cancel or the batch deadline stopped the pulls,
    /// `Err(BatchError::Spawn)` when a worker could not start and `Err(BatchError::WorkerPanicked)`
    /// when `op` or `items` panicked. A panic in `on_result` is not caught: it propagates to the
    /// caller after the workers have stopped.
    pub fn extract_batch<I, R, Op, Cb>(
        &self,
        items: I,
        options: BatchOptions,
        call: &Call<'_>,
        op: Op,
        on_result: Cb,
    ) -> Result<BatchEnd, BatchError>
    where
        I: IntoIterator,
        I::IntoIter: Send,
        I::Item: Send,
        R: Send,
        Op: Fn(I::Item, &Call<'_>) -> R + Sync,
        Cb: FnMut(usize, Result<R, Skipped>),
    {
        run_batch(items, options, call, op, on_result, |scope, work| {
            Builder::new()
                .stack_size(WORKER_STACK_BYTES)
                .spawn_scoped(scope, work)
        })
    }
}

/// The iterator and the index of the next item, behind the lock.
struct Source<T> {
    items: T,
    next_index: usize,
}

/// The engine of [`Extractor::extract_batch`]. `spawn` starts one worker and is the seam the tests
/// use to inject a failure or count the spawns.
pub(super) fn run_batch<I, R, Op, Cb, S>(
    items: I,
    options: BatchOptions,
    call: &Call<'_>,
    op: Op,
    mut on_result: Cb,
    mut spawn: S,
) -> Result<BatchEnd, BatchError>
where
    I: IntoIterator,
    I::IntoIter: Send,
    I::Item: Send,
    R: Send,
    Op: Fn(I::Item, &Call<'_>) -> R + Sync,
    Cb: FnMut(usize, Result<R, Skipped>),
    S: for<'scope> FnMut(
        &'scope Scope<'scope, '_>,
        Box<dyn FnOnce() + Send + 'scope>,
    ) -> io::Result<ScopedJoinHandle<'scope, ()>>,
{
    let items = items.into_iter();
    let upper = items.size_hint().1;
    let wanted = options.threads.get().min(MAX_WORKERS);
    let workers = upper.map_or(wanted, |upper| upper.min(wanted));
    let capacity = workers.saturating_mul(2).min(MAX_CHANNEL);
    let per_item = options.per_item;

    let source = Mutex::new(Source {
        items,
        next_index: 0,
    });
    // Set when a pull is refused for a cancel or the deadline. The only other setter is a failed
    // spawn, whose result is an error anyway, so on `Ok` the flag means `Stopped`.
    let stop = AtomicBool::new(false);

    let work = |sender: SyncSender<(usize, Result<R, Skipped>)>| {
        while !stop.load(Ordering::Acquire) {
            if refusal(call).is_some() {
                stop.store(true, Ordering::Release);
                break;
            }
            let Some((index, item)) = pull(&source) else {
                break;
            };
            let outcome = match refusal(call) {
                Some(skipped) => Err(skipped),
                None => Ok(op(item, &item_call(call, per_item, Instant::now()))),
            };
            if sender.send((index, outcome)).is_err() {
                break;
            }
        }
    };

    thread::scope(|scope| {
        let (sender, receiver) = mpsc::sync_channel(capacity);
        let mut handles = Vec::with_capacity(workers);
        let mut spawn_error = None;
        for _ in 0..workers {
            let sender = sender.clone();
            match spawn(scope, Box::new(move || work(sender))) {
                Ok(handle) => handles.push(handle),
                Err(error) => {
                    stop.store(true, Ordering::Release);
                    spawn_error = Some(error.kind());
                    break;
                }
            }
        }
        // The workers' senders keep the channel open; once they all finish, the loop ends.
        drop(sender);
        for (index, outcome) in receiver {
            on_result(index, outcome);
        }

        let mut panicked = false;
        for handle in handles {
            panicked |= handle.join().is_err();
        }
        if let Some(kind) = spawn_error {
            return Err(BatchError::Spawn(kind));
        }
        if panicked {
            return Err(BatchError::WorkerPanicked);
        }
        Ok(if stop.load(Ordering::Acquire) {
            BatchEnd::Stopped
        } else {
            BatchEnd::Completed
        })
    })
}

/// The next item and its index, or `None` at the end. A poisoned lock also gives `None`: a panic in
/// `next` leaves the iterator in an unknown state, so the batch never reads it again.
fn pull<T: Iterator>(source: &Mutex<Source<T>>) -> Option<(usize, T::Item)> {
    let mut guard = source.lock().ok()?;
    let item = guard.items.next()?;
    let index = guard.next_index;
    guard.next_index = index.saturating_add(1);
    Some((index, item))
}

/// Why the batch stops before running an item, or `None` while it may go on. `check` returns only
/// `Cancelled` or `DeadlineExceeded`.
fn refusal(call: &Call<'_>) -> Option<Skipped> {
    match call.check() {
        Ok(()) => None,
        Err(ExtractError::Cancelled) => Some(Skipped::Cancelled),
        Err(_) => Some(Skipped::DeadlineExceeded),
    }
}

/// The `Call` of one item: the batch's `Call` with the deadline cut to `start + per_item` when that
/// comes first. An overflowing sum leaves the batch deadline in place.
fn item_call<'a>(call: &Call<'a>, per_item: Option<Duration>, start: Instant) -> Call<'a> {
    let item_deadline = per_item.and_then(|per_item| start.checked_add(per_item));
    match call.deadline_at().into_iter().chain(item_deadline).min() {
        Some(deadline) => call.deadline(deadline),
        None => *call,
    }
}
