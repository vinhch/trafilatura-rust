//! Cancellation and deadline handle that every extraction call carries.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use crate::deduplication::{Dedup, DedupCache};
use crate::error::ExtractError;

/// How many nodes a loop visits between two `Call::check` calls; tuned by benchmarks
/// (`docs/architecture.md`, Runtime contract).
pub(crate) const CHECK_INTERVAL: usize = 256;

/// A flag a caller sets from any thread to stop a running extraction.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// Creates a token that is not cancelled.
    pub fn new() -> Self {
        Self::default()
    }

    /// Cancels every call holding a clone of this token.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Relaxed);
    }

    /// Whether `cancel` has been called.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}

/// Per-call context: an optional cancellation token, an optional deadline, an optional latest date
/// for the metadata and an optional duplicate cache.
#[derive(Clone, Copy, Debug, Default)]
pub struct Call<'a> {
    cancel: Option<&'a CancelToken>,
    deadline: Option<Instant>,
    pub(crate) max_date: Option<&'a str>,
    dedup: Dedup<'a>,
}

impl<'a> Call<'a> {
    /// Stops the call with `ExtractError::Cancelled` once `token` is cancelled.
    pub fn cancel(mut self, token: &'a CancelToken) -> Self {
        self.cancel = Some(token);
        self
    }

    /// Stops the call with `ExtractError::DeadlineExceeded` once `deadline` has passed.
    pub fn deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    /// Sets the latest date the metadata accepts, `YYYY-MM-DD`, and the value of `filedate`. The
    /// text is passed on as given; a value that is not a date only changes the date search, which
    /// then uses the end of the current UTC day. Without this, the call uses today's UTC date.
    pub fn max_date(mut self, date: &'a str) -> Self {
        self.max_date = Some(date);
        self
    }

    /// Counts repeated texts in `cache` (upstream's `deduplicate`), so a later call sharing the
    /// cache sees them. Without a cache no text is flagged. The counts of a call reach the cache
    /// when the call returns `Ok` or a rejection; a cancellation, a deadline or any other error
    /// leaves none.
    pub fn dedup(mut self, cache: &'a DedupCache) -> Self {
        self.dedup = Dedup::Direct(cache);
        self
    }

    /// Returns the error to stop with, `Cancelled` before `DeadlineExceeded` when both apply.
    pub(crate) fn check(&self) -> Result<(), ExtractError> {
        if self.cancel.is_some_and(CancelToken::is_cancelled) {
            return Err(ExtractError::Cancelled);
        }
        if self
            .deadline
            .is_some_and(|deadline| Instant::now() >= deadline)
        {
            return Err(ExtractError::DeadlineExceeded);
        }
        Ok(())
    }

    /// Where this call's duplicate counts go.
    pub(crate) fn dedup_state(&self) -> Dedup<'a> {
        self.dedup
    }

    /// The batch deadline of this call, when one is set.
    pub(crate) fn deadline_at(&self) -> Option<Instant> {
        self.deadline
    }

    /// This call with its duplicate counts going to `dedup`, the rest unchanged.
    pub(crate) fn with_dedup(mut self, dedup: Dedup<'a>) -> Self {
        self.dedup = dedup;
        self
    }

    /// Drops the counts a scoped call has buffered, for the rerun of a pass. No-op otherwise.
    pub(crate) fn dedup_reset(&self) {
        if let Dedup::Scoped(scope) = self.dedup {
            scope.reset();
        }
    }
}
