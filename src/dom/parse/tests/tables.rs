use std::collections::BTreeSet;

use crate::dom::parse::tables::{
    BOOLEAN_ATTRIBUTES, DataMode, HTML_START_CLOSE, HTML40_ELEMENT_TABLE, element, end_priority,
    start_closes,
};

#[test]
fn start_close_pairs_match_the_source() {
    let names: BTreeSet<&str> = HTML_START_CLOSE
        .iter()
        .flat_map(|&(open, new)| [open, new])
        .collect();
    let pairs = names
        .iter()
        .map(|new| names.iter().filter(|open| start_closes(new, open)).count())
        .sum::<usize>();
    assert_eq!(pairs, 249);
    assert!(start_closes("li", "p"));
    assert!(start_closes("td", "p"));
    assert!(start_closes("form", "form"));
    assert!(start_closes("a", "a"));
    assert!(!start_closes("section", "p"));
}

#[test]
fn void_elements_are_2_14_6() {
    assert_eq!(element("img").map(|desc| desc.empty), Some(true));
    assert_eq!(element("br").map(|desc| desc.empty), Some(true));
    assert_eq!(element("source"), None);
    assert_eq!(element("track"), None);
}

#[test]
fn data_modes_match_the_switch_list() {
    let switch_list = [
        ("title", DataMode::RcData),
        ("textarea", DataMode::RcData),
        ("style", DataMode::RawText),
        ("iframe", DataMode::RawText),
        ("noembed", DataMode::RawText),
        ("noframes", DataMode::RawText),
        ("xmp", DataMode::RawText),
        ("script", DataMode::Script),
        ("plaintext", DataMode::PlainText),
    ];
    for (name, mode) in switch_list {
        assert_eq!(
            element(name).map(|desc| desc.data_mode),
            Some(mode),
            "{name}"
        );
    }
    assert_eq!(
        element("noscript").map(|desc| desc.data_mode),
        Some(DataMode::Normal)
    );
    let mut others: Vec<&str> = HTML40_ELEMENT_TABLE
        .iter()
        .filter(|(_, desc)| desc.data_mode != DataMode::Normal)
        .map(|&(name, _)| name)
        .collect();
    others.sort_unstable();
    let mut expected: Vec<&str> = switch_list.iter().map(|&(name, _)| name).collect();
    expected.sort_unstable();
    assert_eq!(others, expected);
}

#[test]
fn end_priorities() {
    assert_eq!(end_priority("html"), 220);
    assert_eq!(end_priority("div"), 150);
    assert_eq!(end_priority("p"), 100);
}

#[test]
fn unknown_names_take_the_defaults() {
    assert_eq!(element(""), None);
    assert!(!start_closes("", "p"));
    assert_eq!(end_priority("span"), 100);
}

#[test]
fn boolean_attributes() {
    for name in ["checked", "defer", "disabled", "selected"] {
        assert!(BOOLEAN_ATTRIBUTES.contains(&name), "{name}");
    }
    for name in ["hidden", "async", "value"] {
        assert!(!BOOLEAN_ATTRIBUTES.contains(&name), "{name}");
    }
}
