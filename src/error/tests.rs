use super::*;

#[test]
fn caught_maps_errors() {
    assert_eq!(caught::<u8>(Ok(3)), Ok(Some(3)));
    for rejection in [
        Rejection::EmptyTree,
        Rejection::TreeTooLarge,
        Rejection::InvalidValue,
    ] {
        assert_eq!(
            caught::<u8>(Err(ExtractError::Rejected(rejection))),
            Ok(None)
        );
    }
    assert_eq!(
        caught::<u8>(Err(ExtractError::Cancelled)),
        Err(ExtractError::Cancelled)
    );
    assert_eq!(
        caught::<u8>(Err(ExtractError::DeadlineExceeded)),
        Err(ExtractError::DeadlineExceeded)
    );
}
