use super::*;

#[test]
fn output_report_lists_port_bugs_first() {
    let known: &[&str] = &["known-metadata"];
    let mut rows = vec![known; 25];
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
        source: "page.html",
        expected,
        upstream_error,
        actual,
        known_values: &[],
    };
    let class = |case: &OutputCase| classify_output(case).0;

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

    assert_eq!(class(&case(None, true, &invalid)), ["upstream-error"]);
    assert_eq!(class(&case(None, true, &too_short)), ["port-bug"]);
    assert_eq!(class(&case(None, true, &text)), ["port-bug"]);

    // An invalid-name rejection while upstream extracted text is a port bug: nothing drops the element.
    let dropped = case(Some("a"), false, &invalid);
    assert_eq!(class(&dropped), ["port-bug"]);

    let changed: Result<String, ExtractError> = Ok("a\nx\nc".to_string());
    let (classes, hunks) = classify_output(&case(Some("a\nb\nc"), false, &changed));
    assert_eq!(classes, ["port-bug"]);
    assert_eq!(hunks.len(), 1);
}

#[test]
fn classify_page_none_side_is_port_bug() {
    let dump = Some("html\n".to_string());
    let page = |e: &Option<String>, a: &Option<String>| classify_page("p.html", e, a);
    assert_eq!(page(&dump, &None), (vec!["port-bug"], vec![]));
    assert_eq!(page(&None, &dump), (vec!["port-bug"], vec![]));
    assert_eq!(page(&None, &None), (vec!["match"], vec![]));
}

#[test]
fn classify_page_names_every_differing_hunk_a_port_bug() {
    let expected = Some(
        "html\n  p\n    #text \"bliżej\"\n  q\n  select\n    option\n      #text \"1 Person\"\n"
            .to_string(),
    );
    let actual = Some(
        "html\n  p\n    #text \"bli¿ej\"\n  q\n  select\n    option\n      #text \"1 \"\n      span\n        #text \"Person\"\n"
            .to_string(),
    );
    let (classes, hunks) = classify_page("p.html", &expected, &actual);
    assert_eq!(classes, ["port-bug"]);
    assert_eq!(hunks.len(), 2);
}

#[test]
fn classify_page_cell_space_difference_is_port_bug() {
    let expected = Some("html\n  td\n    p\n      #text \" was\"\n".to_string());
    let actual =
        Some("html\n  td\n    p\n      a\n        #text \" \"\n      #text \"was\"\n".to_string());
    let (classes, _) = classify_page("p.html", &expected, &actual);
    assert_eq!(classes, ["port-bug"]);
}

#[test]
fn tree_upstream_error_reads_lxml_errors() {
    let fixture = json!({"lxml": {"tree": null, "errors": {"tree": "ValueError"}}});
    let actual = Some("html\n".to_string());
    let page = tree_page("p".to_string(), "p.html", true, &fixture, &actual);
    assert_eq!(page.classes, ["upstream-error"]);
}

#[test]
fn expected_output_is_the_lxml_text() {
    let fixture = json!({"lxml": {"formats": {"xml": "<doc xmlns:ns0=\"x\"/>"}}});
    assert_eq!(
        expected_output("xml", &fixture).as_deref(),
        Some("<doc xmlns:ns0=\"x\"/>")
    );
}

#[test]
fn classify_page_ascii_change_is_port_bug() {
    let expected = Some("html\n  #text \"a\"\n".to_string());
    let actual = Some("html\n  #text \"b\"\n".to_string());
    let (classes, hunks) = classify_page("p.html", &expected, &actual);
    assert_eq!(classes, ["port-bug"]);
    assert_eq!(hunks.len(), 1);
}

#[test]
fn hunk_diff_puts_port_bug_hunks_first() {
    let lines: Vec<String> = (0..31).map(|i| format!("line {i}")).collect();
    let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
    let classed: Vec<(Hunk, &'static str)> = (0..31)
        .map(|i| {
            let class = if i == 30 {
                "port-bug"
            } else {
                "known-metadata"
            };
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
