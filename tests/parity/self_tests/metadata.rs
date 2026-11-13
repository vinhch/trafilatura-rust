use super::*;

#[test]
fn stale_known_reports_entries_that_do_not_hold() {
    let page = |id: &str, checked: bool, fields: &[&str]| MetaPage {
        id: id.to_string(),
        classes: vec![],
        checked,
        valid_utf8: true,
        fields: fields
            .iter()
            .map(|name| MetaField {
                name: name.to_string(),
                expected: Value::Null,
                actual: Value::Null,
                class: "port-bug",
            })
            .collect(),
        whole: None,
    };
    let pages = [
        page("absent", true, &[]),
        page("present", true, &["date"]),
        page("unchecked", false, &[]),
    ];
    let known = [
        ("absent", "date", "\"2007-12-16\"", "D-13"),
        ("present", "date", "\"2007-12-16\"", "D-13"),
        ("unchecked", "date", "\"2007-12-16\"", "D-13"),
        ("unlisted", "date", "\"2007-12-16\"", "D-13"),
    ];
    assert_eq!(
        stale_known(&pages, &known),
        ["absent date", "unlisted date"]
    );
}

#[test]
fn known_entry_with_another_value_is_a_port_bug() {
    let want = json!({"date": "2008-07-30", "title": "a"});
    let known = [("page", "date", "\"2007-12-16\"", "D-13")];
    let compare = |got: Value| {
        compare_fields(
            "page",
            want.as_object().unwrap(),
            got.as_object().unwrap(),
            &known,
        )
    };
    let pinned = compare(json!({"date": "2007-12-16", "title": "a"}));
    assert_eq!((pinned.len(), pinned[0].name.as_str()), (1, "date"));
    assert_eq!(pinned[0].class, "known");
    let drifted = compare(json!({"date": "2019-02-09", "title": "a"}));
    assert_eq!((drifted.len(), drifted[0].class), (1, "port-bug"));
}

#[test]
#[should_panic(expected = "on one side only")]
fn crate_only_field_fails() {
    let want = json!({"title": "a"});
    let got = json!({"title": "a", "sitename": "b"});
    compare_fields(
        "page",
        want.as_object().unwrap(),
        got.as_object().unwrap(),
        &[],
    );
}

#[test]
fn field_maps_compares_only_two_objects() {
    let dict = json!({"title": "a"});
    assert!(field_maps(&dict, &dict).is_some());
    assert!(field_maps(&Value::Null, &dict).is_none());
    assert!(field_maps(&dict, &Value::Null).is_none());
    assert!(field_maps(&Value::Null, &Value::Null).is_none());
}

/// A TEI document whose header carries one `date` and whose text is one paragraph.
fn tei_date(date: &str, paragraph: &str) -> String {
    format!(
        "<TEI>\n  <teiHeader>\n    <date>{date}</date>\n  </teiHeader>\n  <text>\n    <p>{paragraph}</p>\n  </text>\n</TEI>\n"
    )
}

#[test]
fn known_metadata_header_only() {
    // Oracle: the METADATA_KNOWN entry of this page and field, and its fixture date 2016-08-15.
    const KNOWN_PAGE: &str = "tests__eval__football.ua.podolski.html";
    let known = known_values(KNOWN_PAGE, &json!({"date": "2016-08-15", "author": null}));
    assert_eq!(known, ["2016-08-15"]);
    let check = |known: &[String], expected: &str, actual: &str| {
        let result: Result<String, ExtractError> = Ok(actual.to_string());
        let case = OutputCase {
            source: "page.html",
            expected: Some(expected),
            upstream_error: false,
            actual: &result,
            known_values: known,
        };
        classify_output(&case)
    };
    let fixture_header = tei_date("2016-08-15", "Body");
    // Synthetic classifier input, not an upstream value: no oracle command backs this date.
    let crate_header = tei_date("2007-12-16", "Body");
    let (classes, hunks) = check(&known, &fixture_header, &crate_header);
    assert_eq!(classes, ["known-metadata"]);
    assert_eq!(hunks.len(), 1);
    // The same difference on a page METADATA_KNOWN does not list.
    assert_eq!(check(&[], &fixture_header, &crate_header).0, ["port-bug"]);
    // A header field the page does not list.
    let author = |name: &str| {
        format!("<TEI>\n  <teiHeader>\n    <author>{name}</author>\n  </teiHeader>\n</TEI>\n")
    };
    assert_eq!(check(&known, &author("A"), &author("B")).0, ["port-bug"]);
    // A text difference on the same page is not confined to the header.
    assert_eq!(
        check(
            &known,
            &tei_date("2016-08-15", "Body"),
            &tei_date("2016-08-15", "Bodx")
        )
        .0,
        ["port-bug"]
    );
    assert_eq!(
        check(&known, &fixture_header, &tei_date("2007-12-16", "Bodx")).0,
        ["known-metadata", "port-bug"]
    );
}

#[test]
fn known_metadata_ignores_a_value_inside_other_text() {
    // "T" is a known value inside "Today" and "Tomorrow". A value this short cannot say which
    // line it explains.
    let known = ["T".to_string()];
    let note = |text: &str| {
        format!("<TEI>\n  <teiHeader>\n    <note>{text}</note>\n  </teiHeader>\n</TEI>\n")
    };
    let expected = note("Tomorrow");
    let actual: Result<String, ExtractError> = Ok(note("Today"));
    let case = OutputCase {
        source: "page.html",
        expected: Some(&expected),
        upstream_error: false,
        actual: &actual,
        known_values: &known,
    };
    assert_eq!(classify_output(&case).0, ["port-bug"]);
}

#[test]
fn known_value_matches_whole_segment_or_tail() {
    // Lines of the `xmltei` fixture of tests__eval__archive.org.dimido.de.marketing.html.
    let date = "2008-07-30";
    assert!(mentions_value(
        "        <bibl>DimidoBlog, 2008-07-30</bibl>",
        date
    ));
    assert!(mentions_value(
        "        <bibl type=\"sigle\">2008-07-30</bibl>",
        date
    ));
    assert!(mentions_value("            <date>2008-07-30</date>", date));
    assert!(!mentions_value("        <title>Maine news</title>", "Main"));
    assert!(!mentions_value("        <note>x2008-07-30</note>", date));
}

#[test]
fn known_metadata_ignores_a_short_value_that_fills_a_segment() {
    // "T" fills the whole segment of one line, but a value under four characters never explains it.
    let known = ["T".to_string()];
    let note = |text: &str| {
        format!("<TEI>\n  <teiHeader>\n    <note>{text}</note>\n  </teiHeader>\n</TEI>\n")
    };
    let expected = note("T");
    let actual: Result<String, ExtractError> = Ok(note("U"));
    let case = OutputCase {
        source: "page.html",
        expected: Some(&expected),
        upstream_error: false,
        actual: &actual,
        known_values: &known,
    };
    assert_eq!(classify_output(&case).0, ["port-bug"]);
}
