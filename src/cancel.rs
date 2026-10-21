//! Cancellation and deadline handle that every extraction call carries.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

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

/// Per-call context: an optional cancellation token and an optional deadline.
#[derive(Clone, Copy, Debug, Default)]
pub struct Call<'a> {
    cancel: Option<&'a CancelToken>,
    deadline: Option<Instant>,
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
}
