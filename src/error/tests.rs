use super::*;

#[test]
fn tree_limit_display() {
    let display = |limit| Rejection::TreeTooLarge(limit).to_string();
    assert_eq!(
        display(TreeLimit::MaxTreeSize { children: 120 }),
        "tree is too large: max_tree_size (120 children)"
    );
    assert_eq!(display(TreeLimit::MaxNodes), "tree is too large: max_nodes");
    assert_eq!(
        display(TreeLimit::TagAttributes),
        "tree is too large: attributes per tag"
    );
    assert_eq!(
        display(TreeLimit::TableCells),
        "tree is too large: table cells"
    );
}

#[test]
fn caught_maps_errors() {
    assert_eq!(caught::<u8>(Ok(3)), Ok(Some(3)));
    for rejection in [
        Rejection::EmptyTree,
        Rejection::TreeTooLarge(TreeLimit::MaxNodes),
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
