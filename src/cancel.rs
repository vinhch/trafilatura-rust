//! Cancellation and deadline handle that every extraction call carries.

use std::sync::Arc;
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
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
    #[cfg(test)]
    trip: Option<&'a CheckTrip>,
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
        #[cfg(test)]
        if let Some(trip) = self.trip {
            trip.step()?;
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

    /// This call's deadline, when one is set.
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

    /// Test seam: counts this call's checks in `trip` and stops the call as `trip` says.
    #[cfg(test)]
    pub(crate) fn trip(mut self, trip: &'a CheckTrip) -> Self {
        self.trip = Some(trip);
        self
    }
}

/// Test seam for `Call::check`: counts the checks of a call, stops it from check `trip_at` on, and
/// makes it report `DeadlineExceeded` once `pass_deadline` is called. `trip_at == 0` never stops.
/// Atomics, not `Cell`, because the batch workers share the `Call` across threads.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct CheckTrip {
    checks: AtomicUsize,
    trip_at: usize,
    deadline_passed: AtomicBool,
}

#[cfg(test)]
impl CheckTrip {
    /// A trip that only counts checks.
    pub(crate) fn counting() -> Self {
        Self::default()
    }

    /// A trip that stops the call from its `index`th check on, 1-based.
    pub(crate) fn at(index: usize) -> Self {
        Self {
            trip_at: index,
            ..Self::default()
        }
    }

    /// The number of checks counted so far.
    pub(crate) fn checks(&self) -> usize {
        self.checks.load(Ordering::SeqCst)
    }

    /// Makes every later check report `DeadlineExceeded`, as a passed deadline would.
    pub(crate) fn pass_deadline(&self) {
        self.deadline_passed.store(true, Ordering::SeqCst);
    }

    /// Counts one check. Stops with `Cancelled` from the trip index on, and with `DeadlineExceeded`
    /// once the deadline is passed.
    fn step(&self) -> Result<(), ExtractError> {
        let count = self.checks.fetch_add(1, Ordering::SeqCst) + 1;
        if self.trip_at != 0 && count >= self.trip_at {
            return Err(ExtractError::Cancelled);
        }
        if self.deadline_passed.load(Ordering::SeqCst) {
            return Err(ExtractError::DeadlineExceeded);
        }
        Ok(())
    }
}

/// Runs `run` once uncancelled, then once per check index with the trip there. Returns the number
/// of checks of the uncancelled run. Each tripped run must stop with `Cancelled` on exactly its
/// trip index.
#[cfg(test)]
pub(crate) fn assert_every_check_stops<T: std::fmt::Debug>(
    run: impl Fn(&Call<'_>) -> Result<T, ExtractError>,
) -> usize {
    let counting = CheckTrip::counting();
    let uncancelled = run(&Call::default().trip(&counting));
    assert!(
        uncancelled.is_ok(),
        "uncancelled run failed: {uncancelled:?}"
    );
    let total = counting.checks();
    for index in 1..=total {
        let trip = CheckTrip::at(index);
        let stopped = run(&Call::default().trip(&trip));
        assert!(
            matches!(stopped, Err(ExtractError::Cancelled)),
            "check {index} did not cancel: {stopped:?}"
        );
        assert_eq!(trip.checks(), index, "check count at return, trip {index}");
    }
    total
}

#[cfg(test)]
mod tests;
