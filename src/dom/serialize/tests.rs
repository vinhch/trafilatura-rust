//! The serializer's void list against the generated element table.

use super::VOID_ELEMENTS;
use crate::dom::parse::tables::HTML40_ELEMENT_TABLE;

#[test]
fn void_elements_are_the_empty_elements_of_the_table() {
    let mut empty: Vec<&str> = HTML40_ELEMENT_TABLE
        .iter()
        .filter(|(_, desc)| desc.empty)
        .map(|(name, _)| *name)
        .collect();
    let mut void = VOID_ELEMENTS.to_vec();
    empty.sort_unstable();
    void.sort_unstable();
    assert_eq!(void, empty);
}
