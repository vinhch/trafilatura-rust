use std::collections::BTreeMap;
use std::io;
use std::num::NonZeroUsize;
use std::panic::{self, AssertUnwindSafe};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::thread::{self, Builder};
use std::time::{Duration, Instant};

use super::PAGE;
use crate::api::batch::{MAX_WORKERS, run_batch};
use crate::api::{BatchEnd, BatchError, BatchOptions, Extractor, Input, Options, Skipped};
use crate::cancel::{Call, CancelToken, CheckTrip};
use crate::error::ExtractError;

/// One of twelve distinct pages: the paragraph text carries `index`.
fn page(index: usize) -> String {
    PAGE.replace("First paragraph", &format!("Paragraph {index}"))
}

fn threads(count: usize) -> BatchOptions {
    BatchOptions::new(NonZeroUsize::new(count).unwrap())
}

/// Sorts results by index, so an order that depends on the worker timing compares equal.
fn by_index<R>(mut results: Vec<(usize, R)>) -> Vec<(usize, R)> {
    results.sort_by_key(|(index, _)| *index);
    results
}

fn text_of(extractor: &Extractor) -> String {
    extractor
        .extract(&Input::text(PAGE), &Call::default())
        .unwrap()
}

#[test]
fn matches_sequential_extract() {
    let extractor = Options::new().build();
    let pages: Vec<String> = (0..12).map(page).collect();

    let mut results = Vec::new();
    let end = extractor.extract_batch(
        pages.iter().enumerate(),
        threads(4),
        &Call::default(),
        |(_, page): (usize, &String), call: &Call<'_>| extractor.extract(&Input::text(page), call),
        |index, result| results.push((index, result)),
    );

    let expected: Vec<_> = pages
        .iter()
        .enumerate()
        .map(|(index, page)| {
            (
                index,
                Ok(extractor.extract(&Input::text(page), &Call::default())),
            )
        })
        .collect();
    assert_eq!(end, Ok(BatchEnd::Completed));
    assert_eq!(by_index(results), expected);
}

#[test]
fn one_thread_keeps_order() {
    let extractor = Options::new().build();
    let pages: Vec<String> = (0..12).map(page).collect();

    let mut order = Vec::new();
    let end = extractor.extract_batch(
        pages.iter().enumerate(),
        threads(1),
        &Call::default(),
        |(_, page): (usize, &String), call: &Call<'_>| extractor.extract(&Input::text(page), call),
        |index, _| order.push(index),
    );

    assert_eq!(end, Ok(BatchEnd::Completed));
    assert_eq!(order, (0..12).collect::<Vec<_>>());
}

#[test]
fn empty_items_spawn_nothing() {
    let spawns = AtomicUsize::new(0);
    let mut callbacks = 0;
    let end = run_batch(
        Vec::<usize>::new(),
        threads(4),
        &Call::default(),
        |item: usize, _: &Call<'_>| item,
        |_, _| callbacks += 1,
        |scope, work| {
            spawns.fetch_add(1, Ordering::Relaxed);
            Builder::new().spawn_scoped(scope, work)
        },
    );

    assert_eq!(end, Ok(BatchEnd::Completed));
    assert_eq!(spawns.load(Ordering::Relaxed), 0);
    assert_eq!(callbacks, 0);
}

#[test]
fn more_threads_than_items() {
    let mut next = 0;
    let items = std::iter::from_fn(move || {
        next += 1;
        (next <= 3).then_some(next)
    });
    let mut callbacks = 0;
    let end = Options::new().build().extract_batch(
        items,
        threads(8),
        &Call::default(),
        |item: usize, _: &Call<'_>| item,
        |_, _| callbacks += 1,
    );

    assert_eq!(end, Ok(BatchEnd::Completed));
    assert_eq!(callbacks, 3);
}

#[test]
fn huge_threads_finite_items_no_panic() {
    let items: Vec<usize> = vec![1, 2, 3];
    let mut callbacks = 0;
    let end = Options::new().build().extract_batch(
        items,
        BatchOptions::new(NonZeroUsize::MAX),
        &Call::default(),
        |item: usize, _: &Call<'_>| item,
        |_, _| callbacks += 1,
    );

    assert_eq!(end, Ok(BatchEnd::Completed));
    assert_eq!(callbacks, 3);
}

#[test]
fn huge_threads_no_size_hint_capped() {
    let mut next = 0;
    let items = std::iter::from_fn(move || {
        next += 1;
        (next <= 3).then_some(next)
    });
    let spawns = AtomicUsize::new(0);
    let mut callbacks = 0;
    let end = run_batch(
        items,
        BatchOptions::new(NonZeroUsize::MAX),
        &Call::default(),
        |item: usize, _: &Call<'_>| item,
        |_, _| callbacks += 1,
        |scope, work| {
            spawns.fetch_add(1, Ordering::Relaxed);
            Builder::new().spawn_scoped(scope, work)
        },
    );

    assert_eq!(end, Ok(BatchEnd::Completed));
    assert_eq!(spawns.load(Ordering::Relaxed), MAX_WORKERS);
    assert_eq!(callbacks, 3);
}

#[test]
fn cancel_mid_batch_stops() {
    let token = CancelToken::new();
    let mut results = Vec::new();
    let end = Options::new().build().extract_batch(
        std::iter::repeat(PAGE).enumerate(),
        threads(4),
        &Call::default().cancel(&token),
        |(index, _): (usize, &str), _: &Call<'_>| {
            // Items from index 3 on wait until the token is cancelled, so later pulls stay in flight.
            if index >= 3 {
                while !token.is_cancelled() {
                    thread::sleep(Duration::from_millis(1));
                }
            }
            index
        },
        |index, result| {
            if index == 2 {
                token.cancel();
            }
            results.push((index, result));
        },
    );

    assert_eq!(end, Ok(BatchEnd::Stopped));
    assert!(
        results
            .iter()
            .all(|(_, result)| *result != Err(Skipped::DeadlineExceeded))
    );
    let by_index: BTreeMap<_, _> = results.into_iter().collect();
    assert_eq!(by_index.get(&0), Some(&Ok(0)));
    assert_eq!(by_index.get(&1), Some(&Ok(1)));
    assert_eq!(by_index.get(&2), Some(&Ok(2)));
}

#[test]
fn endless_iterator_cancel_returns() {
    let token = CancelToken::new();
    let mut callbacks = 0;
    let end = Options::new().build().extract_batch(
        std::iter::repeat(PAGE).enumerate(),
        threads(4),
        &Call::default().cancel(&token),
        |(index, _): (usize, &str), _: &Call<'_>| index,
        |_, _| {
            callbacks += 1;
            if callbacks == 5 {
                token.cancel();
            }
        },
    );

    assert_eq!(end, Ok(BatchEnd::Stopped));
    // Results already in the channel still reach the callback after the cancel, so the count is
    // at least five and depends on timing.
    assert!(callbacks >= 5);
}

#[test]
fn blocked_next_then_cancel_returns() {
    let token = CancelToken::new();
    let mut pulled = 0;
    let items = std::iter::from_fn(move || {
        pulled += 1;
        if pulled == 4 {
            thread::sleep(Duration::from_millis(200));
        }
        Some(pulled)
    });
    let started = Instant::now();
    let mut callbacks = 0;
    let end = Options::new().build().extract_batch(
        items,
        threads(2),
        &Call::default().cancel(&token),
        |item: usize, _: &Call<'_>| item,
        |_, _| {
            callbacks += 1;
            token.cancel();
        },
    );

    assert_eq!(end, Ok(BatchEnd::Stopped));
    assert!(callbacks >= 1);
    // A bound against a hang, not a timing claim: the batch must return once the cancel is seen.
    assert!(started.elapsed() < Duration::from_secs(10));
}

#[test]
fn batch_deadline_stops() {
    let call = Call::default().deadline(Instant::now() + Duration::from_millis(50));
    let end = Options::new().build().extract_batch(
        std::iter::repeat(PAGE),
        threads(2),
        &call,
        |_: &str, _: &Call<'_>| thread::sleep(Duration::from_millis(20)),
        |_, _| {},
    );

    assert_eq!(end, Ok(BatchEnd::Stopped));
}

#[test]
fn per_item_deadline_reaches_op() {
    let extractor = Options::new().build();
    let mut results = Vec::new();
    let end = extractor.extract_batch(
        vec![PAGE; 3],
        threads(2).per_item(Duration::ZERO),
        &Call::default(),
        |page: &str, call: &Call<'_>| extractor.extract(&Input::text(page), call),
        |index, result| results.push((index, result)),
    );

    assert_eq!(end, Ok(BatchEnd::Completed));
    let deadline = Err(ExtractError::DeadlineExceeded);
    assert_eq!(
        by_index(results),
        vec![
            (0, Ok(deadline.clone())),
            (1, Ok(deadline.clone())),
            (2, Ok(deadline))
        ]
    );
}

#[test]
fn per_item_duration_max_no_panic() {
    let extractor = Options::new().build();
    let expected = text_of(&extractor);
    let mut results = Vec::new();
    let end = extractor.extract_batch(
        vec![PAGE; 3],
        threads(2).per_item(Duration::MAX),
        &Call::default(),
        |page: &str, call: &Call<'_>| extractor.extract(&Input::text(page), call),
        |index, result| results.push((index, result)),
    );

    assert_eq!(end, Ok(BatchEnd::Completed));
    let all_ok: Vec<_> = (0..3)
        .map(|index| (index, Ok(Ok(expected.clone()))))
        .collect();
    assert_eq!(by_index(results), all_ok);
}

#[test]
fn channel_bounds_pulls() {
    let token = CancelToken::new();
    let pulls = AtomicUsize::new(0);
    let items = std::iter::repeat_with(|| pulls.fetch_add(1, Ordering::SeqCst));
    let mut first = true;
    let end = Options::new().build().extract_batch(
        items,
        threads(2),
        &Call::default().cancel(&token),
        |item: usize, _: &Call<'_>| item,
        |_, _| {
            if first {
                first = false;
                // The workers fill the channel while the callback sleeps; then the batch stops.
                thread::sleep(Duration::from_millis(200));
                token.cancel();
            }
        },
    );

    assert_eq!(end, Ok(BatchEnd::Stopped));
    // Two workers, a channel of four results, and one item in the callback's hands: 3 * workers + 1.
    assert!(pulls.load(Ordering::SeqCst) <= 3 * 2 + 1);
}

#[test]
fn callback_panic_propagates() {
    let extractor = Options::new().build();
    let outcome = panic::catch_unwind(AssertUnwindSafe(|| {
        extractor.extract_batch(
            std::iter::repeat_n(PAGE, 10).enumerate(),
            threads(4),
            &Call::default(),
            |(index, _): (usize, &str), _: &Call<'_>| index,
            |index, _| {
                if index == 1 {
                    panic!("callback panic");
                }
            },
        )
    }));

    assert!(outcome.is_err());
}

#[test]
fn op_panic_reports_worker_panicked() {
    let mut results = Vec::new();
    let end = Options::new().build().extract_batch(
        0..10usize,
        threads(4),
        &Call::default(),
        |item: usize, _: &Call<'_>| {
            if item == 3 {
                panic!("op panic");
            }
            item
        },
        |index, result| results.push((index, result)),
    );

    assert_eq!(end, Err(BatchError::WorkerPanicked));
    let others: Vec<_> = (0..10usize)
        .filter(|index| *index != 3)
        .map(|index| (index, Ok(index)))
        .collect();
    assert_eq!(by_index(results), others);
}

#[test]
fn spawn_failure_drains() {
    let (signal_tx, signal_rx) = mpsc::channel::<()>();
    let mut results = Vec::new();
    let mut spawns = 0;
    let end = run_batch(
        0..5usize,
        threads(2),
        &Call::default(),
        |item: usize, _: &Call<'_>| {
            if item == 0 {
                let _ = signal_tx.send(());
            }
            item
        },
        |index, result| results.push((index, result)),
        |scope, work| {
            spawns += 1;
            if spawns == 1 {
                return Builder::new().spawn_scoped(scope, work);
            }
            // The second worker fails once item 0 has run on the first.
            let _ = signal_rx.recv();
            Err(io::Error::from(io::ErrorKind::Other))
        },
    );

    assert_eq!(end, Err(BatchError::Spawn(io::ErrorKind::Other)));
    assert!(results.iter().any(|(index, _)| *index == 0));
}

#[test]
fn pulled_item_after_cancel_is_skipped() {
    let token = CancelToken::new();
    let mut pulls = 0;
    // The third pull cancels, so the item it returns is pulled while the cancel is already set.
    let items = std::iter::from_fn(|| {
        pulls += 1;
        if pulls == 3 {
            token.cancel();
        }
        (pulls <= 5).then_some(pulls - 1)
    });
    let ran = Mutex::new(Vec::new());
    let mut results = Vec::new();
    let end = Options::new().build().extract_batch(
        items,
        threads(1),
        &Call::default().cancel(&token),
        |item: usize, _: &Call<'_>| {
            ran.lock().unwrap().push(item);
            item
        },
        |index, result| results.push((index, result)),
    );

    assert_eq!(end, Ok(BatchEnd::Stopped));
    assert_eq!(
        results,
        vec![(0, Ok(0)), (1, Ok(1)), (2, Err(Skipped::Cancelled))]
    );
    assert_eq!(ran.into_inner().unwrap(), vec![0, 1]);
}

#[test]
fn pulled_item_after_deadline_is_skipped() {
    let trip = CheckTrip::counting();
    let mut pulls = 0;
    // The third pull passes the deadline, so its item is pulled after it.
    let items = std::iter::from_fn(|| {
        pulls += 1;
        if pulls == 3 {
            trip.pass_deadline();
        }
        (pulls <= 5).then_some(pulls - 1)
    });
    let ran = Mutex::new(Vec::new());
    let mut results = Vec::new();
    let end = Options::new().build().extract_batch(
        items,
        threads(1),
        &Call::default().trip(&trip),
        |item: usize, _: &Call<'_>| {
            ran.lock().unwrap().push(item);
            item
        },
        |index, result| results.push((index, result)),
    );

    assert_eq!(end, Ok(BatchEnd::Stopped));
    assert_eq!(
        results,
        vec![(0, Ok(0)), (1, Ok(1)), (2, Err(Skipped::DeadlineExceeded))]
    );
    assert_eq!(ran.into_inner().unwrap(), vec![0, 1]);
}

#[test]
fn pre_cancelled_batch_pulls_nothing() {
    let token = CancelToken::new();
    token.cancel();
    let pulls = AtomicUsize::new(0);
    let items = std::iter::repeat_with(|| pulls.fetch_add(1, Ordering::SeqCst));
    let mut callbacks = 0;
    let end = Options::new().build().extract_batch(
        items,
        threads(4),
        &Call::default().cancel(&token),
        |item: usize, _: &Call<'_>| item,
        |_, _| callbacks += 1,
    );

    assert_eq!(end, Ok(BatchEnd::Stopped));
    assert_eq!(pulls.load(Ordering::SeqCst), 0);
    assert_eq!(callbacks, 0);
}

#[test]
fn iterator_panic_poisons_lock_and_reports_worker_panicked() {
    let mut calls = 0;
    let items = std::iter::from_fn(move || {
        calls += 1;
        // The fourth pull panics inside `next`, which poisons the lock. The other worker then sees
        // the end of input.
        assert!(calls < 4, "iterator panic");
        Some(calls)
    });
    let mut results = Vec::new();
    let end = Options::new().build().extract_batch(
        items,
        threads(2),
        &Call::default(),
        |item: usize, _: &Call<'_>| item,
        |index, result| results.push((index, result)),
    );

    assert_eq!(end, Err(BatchError::WorkerPanicked));
    assert_eq!(by_index(results), vec![(0, Ok(1)), (1, Ok(2)), (2, Ok(3))]);
}
