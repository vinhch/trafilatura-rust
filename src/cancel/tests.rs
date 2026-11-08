use super::{Call, CancelToken, CheckTrip, assert_every_check_stops};
use crate::error::ExtractError;

#[test]
fn counting_trip_counts_without_stopping() {
    let trip = CheckTrip::counting();
    let call = Call::default().trip(&trip);

    assert_eq!(call.check(), Ok(()));
    assert_eq!(call.check(), Ok(()));
    assert_eq!(call.check(), Ok(()));
    assert_eq!(trip.checks(), 3);
}

#[test]
fn trip_stops_from_its_index_on() {
    let trip = CheckTrip::at(2);
    let call = Call::default().trip(&trip);

    assert_eq!(call.check(), Ok(()));
    assert_eq!(call.check(), Err(ExtractError::Cancelled));
    assert_eq!(call.check(), Err(ExtractError::Cancelled));
    assert_eq!(trip.checks(), 3);
}

#[test]
fn passed_deadline_switch_gives_deadline_exceeded() {
    let trip = CheckTrip::counting();
    trip.pass_deadline();

    assert_eq!(
        Call::default().trip(&trip).check(),
        Err(ExtractError::DeadlineExceeded)
    );
}

#[test]
fn cancelled_token_wins_over_the_deadline_switch() {
    let token = CancelToken::new();
    token.cancel();
    let trip = CheckTrip::counting();
    trip.pass_deadline();

    assert_eq!(
        Call::default().cancel(&token).trip(&trip).check(),
        Err(ExtractError::Cancelled)
    );
}

#[test]
fn sweep_counts_every_check() {
    let total = assert_every_check_stops(|call| {
        call.check()?;
        call.check()
    });

    assert_eq!(total, 2);
}
