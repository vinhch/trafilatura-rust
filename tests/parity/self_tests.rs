use super::*;

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
fn output_report_lists_port_bugs_first() {
    let encoding: &[&str] = &["encoding"];
    let mut rows = vec![encoding; 25];
    rows.push(&["port-bug"]);
    rows.insert(3, &["match"]);
    let order = diff_order(&rows);
    let port_bug = rows.len() - 1;
    assert_eq!(order[0], port_bug);
    assert!(order[..DIFF_PAGES].contains(&port_bug));
    assert_eq!(order.len(), 26);
    assert_eq!(order[1..4], [0, 1, 2]);
}

#[test]
fn classify_output_names_each_class() {
    let rejected = |r| Err(ExtractError::Rejected(r));
    let text: Result<String, ExtractError> = Ok("a".to_string());
    let other: Result<String, ExtractError> = Ok("b".to_string());
    let too_short = rejected(Rejection::TooShort);
    let invalid = rejected(Rejection::InvalidValue);
    let case = |expected, upstream_error, actual| OutputCase {
        id: "page",
        source: "page.html",
        valid_utf8: true,
        expected,
        upstream_error,
        actual,
        known_values: &[],
    };
    let listed = [("page", "element name is not an XML Name")];
    let class = |case: &OutputCase| classify_output(case, &[]).0;

    assert_eq!(class(&case(Some("a"), false, &text)), ["match"]);
    assert_eq!(class(&case(None, false, &too_short)), ["match"]);
    assert_eq!(class(&case(None, false, &text)), ["port-bug"]);
    assert_eq!(class(&case(Some("a"), false, &too_short)), ["port-bug"]);
    assert_eq!(class(&case(Some("a"), false, &other)), ["port-bug"]);

    let gz = OutputCase {
        source: "page.html.gz",
        ..case(Some("a"), false, &other)
    };
    let gz_class = if cfg!(feature = "decompress") {
        "port-bug"
    } else {
        "decompress"
    };
    assert_eq!(class(&gz), [gz_class]);

    let latin1 = |expected, actual| OutputCase {
        valid_utf8: false,
        ..case(expected, false, actual)
    };
    let decoded: Result<String, ExtractError> = Ok("bli¿ej".to_string());
    assert_eq!(class(&latin1(Some("bliżej"), &decoded)), ["encoding"]);
    let ellipsis: Result<String, ExtractError> = Ok("Idealen (…), hinter".to_string());
    assert_eq!(
        class(&latin1(Some("Idealen ( ), hinter"), &ellipsis)),
        ["encoding"]
    );
    assert_eq!(class(&latin1(Some("a"), &other)), ["port-bug"]);
    assert_eq!(class(&latin1(Some("a"), &too_short)), ["port-bug"]);

    assert_eq!(class(&case(None, true, &invalid)), ["upstream-error"]);
    assert_eq!(class(&case(None, true, &too_short)), ["port-bug"]);
    assert_eq!(class(&case(None, true, &text)), ["port-bug"]);

    let dropped = case(Some("a"), false, &invalid);
    assert_eq!(classify_output(&dropped, &listed).0, ["normalized"]);
    assert_eq!(classify_output(&dropped, &[]).0, ["port-bug"]);
    let two_texts = case(Some("a"), false, &other);
    assert_eq!(classify_output(&two_texts, &listed).0, ["port-bug"]);

    let (classes, hunks) =
        classify_output(&latin1(Some("a\nb\nc"), &Ok("a\nx\nc".to_string())), &[]);
    assert_eq!(classes, ["port-bug"]);
    assert_eq!(hunks.len(), 1);
}

#[test]
fn classify_page_none_side_is_port_bug() {
    let dump = Some("html\n".to_string());
    let page = |e: &Option<String>, a: &Option<String>| classify_page("p.html", true, e, a);
    assert_eq!(page(&dump, &None), (vec!["port-bug"], vec![]));
    assert_eq!(page(&None, &dump), (vec!["port-bug"], vec![]));
    assert_eq!(page(&None, &None), (vec!["match"], vec![]));
}

#[test]
fn classify_page_mixes_classes() {
    let expected = Some(
        "html\n  p\n    #text \"bliżej\"\n  q\n  select\n    option\n      #text \"1 Person\"\n"
            .to_string(),
    );
    let actual = Some(
        "html\n  p\n    #text \"bli¿ej\"\n  q\n  select\n    option\n      #text \"1 \"\n      span\n        #text \"Person\"\n"
            .to_string(),
    );
    let (classes, hunks) = classify_page("p.html", false, &expected, &actual);
    assert_eq!(classes, ["encoding", "python-parser"]);
    assert_eq!(hunks.len(), 2);
}

#[test]
fn classify_page_names_cell_space_hunk() {
    let expected = Some("html\n  td\n    p\n      #text \" was\"\n".to_string());
    let actual =
        Some("html\n  td\n    p\n      a\n        #text \" \"\n      #text \"was\"\n".to_string());
    let (classes, hunks) = classify_page("p.html", true, &expected, &actual);
    assert_eq!(classes, ["html5lib-cell-space"]);
    assert_eq!(hunks.len(), 1);
}

#[test]
fn classify_page_ascii_change_is_port_bug() {
    let expected = Some("html\n  #text \"a\"\n".to_string());
    let actual = Some("html\n  #text \"b\"\n".to_string());
    let (classes, hunks) = classify_page("p.html", true, &expected, &actual);
    assert_eq!(classes, ["port-bug"]);
    assert_eq!(hunks.len(), 1);
}

#[test]
fn hunk_diff_puts_port_bug_hunks_first() {
    let lines: Vec<String> = (0..31).map(|i| format!("line {i}")).collect();
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let classed: Vec<(Hunk, &'static str)> = (0..31)
        .map(|i| {
            let class = if i == 30 { "port-bug" } else { "encoding" };
            (
                Hunk {
                    expected: i..i + 1,
                    actual: i..i + 1,
                },
                class,
            )
        })
        .collect();
    let out = hunk_diff(&refs, &refs, &classed);
    assert!(
        out.starts_with("@@ -31,1 +31,1 (port-bug)\n-line 30\n+line 30\n"),
        "{out}"
    );
    assert_eq!(
        out.lines().filter(|l| l.starts_with("@@")).count(),
        DIFF_LINES
    );
}

#[test]
fn shown_diff_names_a_final_newline_difference() {
    let shown = |expected: &str, actual: &str| {
        shown_diff(&Some(expected.into()), &Some(actual.into()), &[])
    };
    assert!(shown("a\nb\n", "a\nb").contains("differs only in the final newline"));
    assert!(!shown("a\r\nb", "a\nb").contains("differs only in the final newline"));
}

#[test]
fn stale_known_reports_entries_that_do_not_hold() {
    let page = |id: &str, checked: bool, fields: &[&str]| MetaPage {
        id: id.to_string(),
        classes: vec![],
        checked,
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
        ("absent", "date", "D-13"),
        ("present", "date", "D-13"),
        ("unchecked", "date", "D-13"),
        ("unlisted", "date", "D-13"),
    ];
    assert_eq!(
        stale_known(&pages, &known),
        ["absent date", "unlisted date"]
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
    let check = |id: &str, known: &[String], expected: &str, actual: &str| {
        let result: Result<String, ExtractError> = Ok(actual.to_string());
        let case = OutputCase {
            id,
            source: "page.html",
            valid_utf8: true,
            expected: Some(expected),
            upstream_error: false,
            actual: &result,
            known_values: known,
        };
        classify_output(&case, &[])
    };
    let fixture_header = tei_date("2016-08-15", "Body");
    // Synthetic classifier input, not an upstream value: no oracle command backs this date.
    let crate_header = tei_date("2007-12-16", "Body");
    let (classes, hunks) = check(KNOWN_PAGE, &known, &fixture_header, &crate_header);
    assert_eq!(classes, ["known-metadata"]);
    assert_eq!(hunks.len(), 1);
    // The same difference on a page METADATA_KNOWN does not list.
    assert_eq!(
        check("page.html", &[], &fixture_header, &crate_header).0,
        ["port-bug"]
    );
    // A header field the page does not list.
    let author = |name: &str| {
        format!("<TEI>\n  <teiHeader>\n    <author>{name}</author>\n  </teiHeader>\n</TEI>\n")
    };
    assert_eq!(
        check(KNOWN_PAGE, &known, &author("A"), &author("B")).0,
        ["port-bug"]
    );
    // A text difference on the same page is not confined to the header.
    assert_eq!(
        check(
            KNOWN_PAGE,
            &known,
            &tei_date("2016-08-15", "Body"),
            &tei_date("2016-08-15", "Bodx")
        )
        .0,
        ["port-bug"]
    );
    assert_eq!(
        check(
            KNOWN_PAGE,
            &known,
            &fixture_header,
            &tei_date("2007-12-16", "Bodx")
        )
        .0,
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
        id: "page.html",
        source: "page.html",
        valid_utf8: true,
        expected: Some(&expected),
        upstream_error: false,
        actual: &actual,
        known_values: &known,
    };
    assert_eq!(classify_output(&case, &[]).0, ["port-bug"]);
}

#[test]
fn fingerprint_on_encoding_page() {
    let check = |valid_utf8: bool, expected: &str, actual: &str| {
        let result: Result<String, ExtractError> = Ok(actual.to_string());
        let case = OutputCase {
            id: "page.html",
            source: "page.html",
            valid_utf8,
            expected: Some(expected),
            upstream_error: false,
            actual: &result,
            known_values: &[],
        };
        classify_output(&case, &[])
    };
    // A fingerprint over the D-3 text: its hunk follows the encoding hunks of the page.
    let expected = "<note type=\"fingerprint\">aa</note>\n<p>same</p>\n<p>bli\u{17c}ej</p>\n";
    let actual = "<note type=\"fingerprint\">bb</note>\n<p>same</p>\n<p>bli\u{bf}ej</p>\n";
    let (classes, hunks) = check(false, expected, actual);
    assert_eq!(classes, ["encoding"]);
    assert_eq!(hunks.len(), 2);

    // The same fingerprint with a text hunk that is not an encoding difference.
    let ascii_change = "<note type=\"fingerprint\">bb</note>\n<p>same</p>\n<p>blaej</p>\n";
    assert_eq!(check(false, expected, ascii_change).0, ["port-bug"]);

    // The xml form, `<doc fingerprint="...">`.
    let xml_expected = "<doc fingerprint=\"aa\">\n  <p>same</p>\n  <p>bli\u{17c}ej</p>\n</doc>\n";
    let xml_actual = "<doc fingerprint=\"bb\">\n  <p>same</p>\n  <p>bli\u{bf}ej</p>\n</doc>\n";
    assert_eq!(check(false, xml_expected, xml_actual).0, ["encoding"]);

    // Valid UTF-8 has no D-3 text, so its fingerprint is a port bug.
    let fingerprint_only = "<note type=\"fingerprint\">aa</note>\n";
    let other = "<note type=\"fingerprint\">bb</note>\n";
    assert_eq!(check(true, fingerprint_only, other).0, ["port-bug"]);
}

#[test]
fn fingerprint_only_is_port_bug() {
    // No text hunk carries the D-3 difference, so the fingerprint is unexplained on either encoding.
    let expected = "<note type=\"fingerprint\">aa</note>\n<p>same</p>\n";
    let result: Result<String, ExtractError> =
        Ok("<note type=\"fingerprint\">bb</note>\n<p>same</p>\n".to_string());
    for valid_utf8 in [false, true] {
        let case = OutputCase {
            id: "page.html",
            source: "page.html",
            valid_utf8,
            expected: Some(expected),
            upstream_error: false,
            actual: &result,
            known_values: &[],
        };
        assert_eq!(
            classify_output(&case, &[]).0,
            ["port-bug"],
            "valid_utf8 {valid_utf8}"
        );
    }
}
