use super::*;

#[test]
fn hunks_align_an_inserted_line() {
    let found = hunks(&["a", "b", "c"], &["a", "x", "b", "c"]);
    assert_eq!(
        found,
        vec![Hunk {
            expected: 1..1,
            actual: 1..2
        }]
    );
}

#[test]
fn hunks_of_equal_lists_is_empty() {
    assert!(hunks(&["a", "b"], &["a", "b"]).is_empty());
}
