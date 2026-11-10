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
fn input_limit_display() {
    let display = |limit| Rejection::InputTooLarge(limit).to_string();
    assert_eq!(
        display(InputLimit::MaxInputBytes),
        "input is too large: max_input_bytes"
    );
    assert_eq!(
        display(InputLimit::MaxFileSize),
        "input is too large: max_file_size"
    );
    assert_eq!(
        display(InputLimit::JsonSize),
        "input is too large: JSON node count or offset past 32 bits"
    );
}

#[test]
fn missing_fields_lists_all_in_upstream_order() {
    let names = |date, title, url| {
        MissingFields::new(date, title, url)
            .unwrap()
            .names()
            .collect::<Vec<_>>()
    };
    assert_eq!(names(true, false, true), ["date", "url"]);
    assert_eq!(names(true, true, true), ["date", "title", "url"]);
    let missing = MissingFields::new(true, false, true).unwrap();
    assert_eq!(
        Rejection::MissingMetadata(missing).to_string(),
        "required metadata is missing: date, url"
    );
    assert_eq!(MissingFields::new(false, false, false), None);
}

#[test]
fn missing_fields_report_each_flag() {
    let missing = MissingFields::new(false, true, false).unwrap();
    assert!(!missing.date());
    assert!(missing.title());
    assert!(!missing.url());
}

#[test]
fn too_short_display() {
    assert_eq!(
        Rejection::TooShort {
            text_chars: 3,
            comments_chars: 0
        }
        .to_string(),
        "extracted text is too short: 3 text chars, 0 comment chars"
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
