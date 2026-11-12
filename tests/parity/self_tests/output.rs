use super::*;

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
    let too_short = rejected(Rejection::TooShort {
        text_chars: 0,
        comments_chars: 0,
    });
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
    let page = |e: &Option<String>, a: &Option<String>| {
        classify_page("p.html", true, Variant::Html5, e, a)
    };
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
    let (classes, hunks) = classify_page("p.html", false, Variant::Html5, &expected, &actual);
    assert_eq!(classes, ["encoding", "python-parser"]);
    assert_eq!(hunks.len(), 2);
}

#[test]
fn classify_page_names_cell_space_hunk() {
    let expected = Some("html\n  td\n    p\n      #text \" was\"\n".to_string());
    let actual =
        Some("html\n  td\n    p\n      a\n        #text \" \"\n      #text \"was\"\n".to_string());
    let (classes, hunks) = classify_page("p.html", true, Variant::Html5, &expected, &actual);
    assert_eq!(classes, ["html5lib-cell-space"]);
    assert_eq!(hunks.len(), 1);
}

#[test]
fn classify_page_lxml_has_no_html5_classes() {
    let expected = Some(
        "html\n  p\n    #text \"bliżej\"\n  q\n  select\n    option\n      #text \"1 Person\"\n"
            .to_string(),
    );
    let actual = Some(
        "html\n  p\n    #text \"bli¿ej\"\n  q\n  select\n    option\n      #text \"1 \"\n      span\n        #text \"Person\"\n"
            .to_string(),
    );
    let (classes, hunks) = classify_page("p.html", false, Variant::Lxml, &expected, &actual);
    assert_eq!(classes, ["encoding", "port-bug"]);
    assert_eq!(hunks.len(), 2);

    let expected = Some("html\n  td\n    p\n      #text \" was\"\n".to_string());
    let actual =
        Some("html\n  td\n    p\n      a\n        #text \" \"\n      #text \"was\"\n".to_string());
    let (classes, _) = classify_page("p.html", true, Variant::Lxml, &expected, &actual);
    assert_eq!(classes, ["port-bug"]);
}

#[test]
fn report_names_per_variant() {
    assert_eq!(Variant::Html5.report_name("report.md"), "report.md");
    assert_eq!(
        Variant::Lxml.report_name("output-report.md"),
        "lxml-output-report.md"
    );
}

#[test]
fn lxml_upstream_error_reads_lxml_errors() {
    let fixture = json!({
        "lxml": {"tree": null, "errors": {"tree": "ValueError"}},
        "html5": {"tree": "html\n", "errors": {"tree": null}},
    });
    let actual = Some("html\n".to_string());
    let page = |variant| tree_page("p".to_string(), "p.html", true, variant, &fixture, &actual);
    assert_eq!(page(Variant::Lxml).classes, ["upstream-error"]);
    assert_eq!(page(Variant::Html5).classes, ["match"]);
}

#[test]
fn lxml_output_skips_namespace_strip() {
    let fixture = json!({
        "lxml": {"formats": {"xml": "<doc xmlns:ns0=\"x\"/>"}},
        "html5": {"formats": {"xml": "<doc xmlns:ns0=\"x\"/>"}},
    });
    assert_eq!(
        expected_output(Variant::Lxml, "p", "xml", &fixture).as_deref(),
        Some("<doc xmlns:ns0=\"x\"/>")
    );
    assert_eq!(
        expected_output(Variant::Html5, "p", "xml", &fixture).as_deref(),
        Some("<doc/>")
    );
}

#[test]
fn classify_page_ascii_change_is_port_bug() {
    let expected = Some("html\n  #text \"a\"\n".to_string());
    let actual = Some("html\n  #text \"b\"\n".to_string());
    let (classes, hunks) = classify_page("p.html", true, Variant::Html5, &expected, &actual);
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
