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
fn strip_lxml_namespaces_removes_only_ns_prefixes() {
    assert_eq!(
        strip_lxml_namespaces(
            r#"<p><ns0:svg xmlns:ns0="http://www.w3.org/2000/svg"><ns0:path/></ns0:svg></p>"#
        ),
        "<p><svg><path/></svg></p>"
    );
    assert_eq!(
        strip_lxml_namespaces("<ns12:g xmlns:ns12=\"u\"></ns12:g>"),
        "<g></g>"
    );
    assert_eq!(
        strip_lxml_namespaces("<svg:g>x</svg:g>"),
        "<svg:g>x</svg:g>"
    );
    assert_eq!(strip_lxml_namespaces("<p>ns0:x</p>"), "<p>ns0:x</p>");
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
    let accepted = [("page", "tree", "D-2")];
    assert_eq!(
        accepted_failures(&accepted, "tree", &[], 10),
        ["tree: stale accepted entry page"]
    );
    assert!(accepted_failures(&accepted, "tree", &["page"], 10).is_empty());
    assert!(accepted_failures(&accepted, "txt", &[], 10).is_empty());
}

#[test]
fn accept_relabels_only_listed_port_bugs() {
    let accepted = [("page", "tree", "D-2")];
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
