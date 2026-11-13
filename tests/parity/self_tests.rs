use super::*;

mod metadata;
mod output;

#[test]
fn gate_skips_only_when_fixtures_are_not_required() {
    assert_eq!(gate(false, false, None), Ok(false));
    assert!(gate(true, false, None).is_err());
    assert!(gate(true, true, Some(0)).is_err());
    assert_eq!(gate(false, true, Some(0)), Ok(true));
    assert_eq!(gate(true, true, Some(3)), Ok(true));
}

#[test]
fn accepted_page_counts_against_the_cap() {
    let ids: Vec<String> = (0..11).map(|i| format!("page{i}")).collect();
    let accepted: Vec<(&str, &str, &str)> =
        ids.iter().map(|id| (id.as_str(), "txt", "D-1")).collect();
    let classed: Vec<&str> = ids.iter().map(String::as_str).collect();
    assert_eq!(
        accepted_failures(&accepted, "txt", &classed, 10),
        ["txt: 11 accepted pages, the cap is 10"]
    );
    assert!(accepted_failures(&accepted[..10], "txt", &classed[..10], 10).is_empty());
}

#[test]
fn stale_accepted_entry_fails() {
    let accepted = [("page", "tree", "D-n")];
    assert_eq!(
        accepted_failures(&accepted, "tree", &[], 10),
        ["tree: stale accepted entry page"]
    );
    assert!(accepted_failures(&accepted, "tree", &["page"], 10).is_empty());
    assert!(accepted_failures(&accepted, "txt", &[], 10).is_empty());
}

#[test]
fn accept_relabels_only_listed_port_bugs() {
    let accepted = [("page", "tree", "D-n")];
    let mut listed = vec!["match", "port-bug"];
    accept(&accepted, "tree", "page", &mut listed);
    assert_eq!(listed, ["match", "accepted"]);
    let mut other_format = vec!["port-bug"];
    accept(&accepted, "txt", "page", &mut other_format);
    let mut other_page = vec!["port-bug"];
    accept(&accepted, "tree", "other", &mut other_page);
    assert_eq!(other_format, ["port-bug"]);
    assert_eq!(other_page, ["port-bug"]);
}

#[test]
fn non_utf8_page_difference_is_port_bug() {
    let expected = Some("html\n  p\n    #text \"bliżej\"\n".to_string());
    let actual = Some("html\n  p\n    #text \"bli¿ej\"\n".to_string());
    assert_eq!(classify_page("p.html", &expected, &actual).0, ["port-bug"]);
    let decoded: Result<String, ExtractError> = Ok("bli¿ej".to_string());
    let case = OutputCase {
        source: "page.html",
        expected: Some("bliżej"),
        upstream_error: false,
        actual: &decoded,
        known_values: &[],
    };
    assert_eq!(classify_output(&case).0, ["port-bug"]);
}

#[test]
fn gate_counts_non_utf8_port_bugs() {
    // The classes decide, not the encoding of the page: a non-UTF-8 page with `port-bug` counts.
    let rows = vec![
        ("first".to_string(), vec!["port-bug"]),
        ("clean".to_string(), vec!["match"]),
        ("mixed".to_string(), vec!["match", "port-bug"]),
    ];
    assert_eq!(gate_port_bugs(rows.into_iter()), ["first", "mixed"]);
}
