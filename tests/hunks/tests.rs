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

fn whole(expected: &[&str], actual: &[&str]) -> Hunk {
    Hunk {
        expected: 0..expected.len(),
        actual: 0..actual.len(),
    }
}

#[test]
fn encoding_hunk_accepts_non_ascii_only_change() {
    let (e, a) = (["#text \"bliżej\""], ["#text \"bli¿ej\""]);
    assert!(is_encoding_hunk(&e, &a, &whole(&e, &a)));
}

#[test]
fn encoding_hunk_rejects_ascii_change() {
    let e = ["#text \"bliżej\"", "#text \"abc\""];
    let a = ["#text \"bli¿ej\"", "#text \"abd\""];
    assert!(!is_encoding_hunk(&e, &a, &whole(&e, &a)));
    let insert = Hunk {
        expected: 0..0,
        actual: 0..1,
    };
    assert!(!is_encoding_hunk(&[], &["#text \"x\""], &insert));
}

#[test]
fn encoding_text_hunk_accepts_whitespace_from_decoding() {
    let (e, a) = (["Idealen ( ), hinter"], ["Idealen (…), hinter"]);
    let hunk = whole(&e, &a);
    assert!(is_encoding_text_hunk(&e, &a, &hunk));
    assert!(!is_encoding_hunk(&e, &a, &hunk));
    let ascii = ["Idealen (x), hinter"];
    let hunk = whole(&e, &ascii);
    assert!(!is_encoding_text_hunk(&e, &ascii, &hunk));
    assert!(!is_encoding_hunk(&e, &ascii, &hunk));
}

const SELECT: [&str; 2] = ["    select", "      option value=\"1\""];

fn tail(lines: &[&'static str]) -> Vec<&'static str> {
    SELECT.iter().chain(lines).copied().collect()
}

fn tail_hunk(expected: &[&str], actual: &[&str]) -> Hunk {
    Hunk {
        expected: 2..expected.len(),
        actual: 2..actual.len(),
    }
}

#[test]
fn python_parser_hunk_accepts_kept_phrasing() {
    let e = tail(&["        #text \"1 Person\""]);
    let a = tail(&[
        "        #text \"1 \"",
        "        span",
        "          #text \"Person\"",
    ]);
    assert!(is_python_parser_hunk(&e, &a, &tail_hunk(&e, &a)));
}

#[test]
fn python_parser_hunk_rejects_changed_text() {
    let e = tail(&["        #text \"1 Person\""]);
    let a = tail(&[
        "        #text \"1 \"",
        "        span",
        "          #text \"Persons\"",
    ]);
    assert!(!is_python_parser_hunk(&e, &a, &tail_hunk(&e, &a)));
}

#[test]
fn python_parser_hunk_rejects_line_outside_subtree() {
    let e = tail(&["        #text \"1 Person\""]);
    let a = tail(&["        #text \"1 \"", "    span", "      #text \"Person\""]);
    assert!(!is_python_parser_hunk(&e, &a, &tail_hunk(&e, &a)));
}

#[test]
fn python_parser_hunk_rejects_dropped_empty_option() {
    let e = ["  select", "    option value=\"1\""];
    let a = ["  select"];
    let hunk = Hunk {
        expected: 1..2,
        actual: 1..1,
    };
    assert!(!is_python_parser_hunk(&e, &a, &hunk));
}

#[test]
fn python_parser_hunk_rejects_added_empty_option() {
    let e = ["  select"];
    let a = ["  select", "    option value=\"1\""];
    let hunk = Hunk {
        expected: 1..1,
        actual: 1..2,
    };
    assert!(!is_python_parser_hunk(&e, &a, &hunk));
}

#[test]
fn python_parser_hunk_rejects_changed_attribute() {
    let e = [
        "    select",
        "      option value=\"1\"",
        "        #text \"x\"",
    ];
    let a = ["    select", "      option", "        #text \"x\""];
    let hunk = Hunk {
        expected: 1..2,
        actual: 1..2,
    };
    assert!(!is_python_parser_hunk(&e, &a, &hunk));
}

#[test]
fn encoding_text_hunk_rejects_whitespace_only_change() {
    for (e, a) in [("Hello world", "Helloworld"), ("  - item", "- item")] {
        let (e, a) = ([e], [a]);
        assert!(
            !is_encoding_text_hunk(&e, &a, &whole(&e, &a)),
            "{e:?} {a:?}"
        );
    }
}

#[test]
fn encoding_text_hunk_accepts_dropped_non_ascii_inside_a_word() {
    let (e, a) = (["word„x"], ["wordx"]);
    assert!(is_encoding_text_hunk(&e, &a, &whole(&e, &a)));
}

#[test]
fn python_parser_hunk_rejects_dropped_parser_element() {
    let e = ["  div", "    select name=\"q\""];
    let a = ["  div"];
    let hunk = Hunk {
        expected: 1..2,
        actual: 1..1,
    };
    assert!(!is_python_parser_hunk(&e, &a, &hunk));
}

const CELL: [&str; 2] = ["    td", "      p"];

fn cell_lines(lines: &[&'static str]) -> Vec<&'static str> {
    CELL.iter().chain(lines).copied().collect()
}

fn cell_hunk(expected: &[&str], actual: &[&str]) -> Hunk {
    Hunk {
        expected: 2..expected.len(),
        actual: 2..actual.len(),
    }
}

#[test]
fn cell_space_hunk_accepts_unreconstructed_formatting_element() {
    let e = cell_lines(&["        #text \" was published\""]);
    let a = cell_lines(&[
        "        a href=\"x\"",
        "          #text \" \"",
        "        #text \"was published\"",
    ]);
    assert!(is_html5lib_cell_space_hunk(&e, &a, &cell_hunk(&e, &a)));
}

#[test]
fn cell_space_hunk_accepts_newline_whitespace() {
    let e = cell_lines(&["        #text \"\\n was\""]);
    let a = cell_lines(&[
        "        i",
        "          #text \"\\n \"",
        "        #text \"was\"",
    ]);
    assert!(is_html5lib_cell_space_hunk(&e, &a, &cell_hunk(&e, &a)));
}

#[test]
fn cell_space_hunk_rejects_changed_text() {
    let e = cell_lines(&["        #text \" was published\""]);
    let a = cell_lines(&[
        "        a href=\"x\"",
        "          #text \" \"",
        "        #text \"was publisher\"",
    ]);
    assert!(!is_html5lib_cell_space_hunk(&e, &a, &cell_hunk(&e, &a)));
}

#[test]
fn cell_space_hunk_rejects_non_whitespace_text_in_element() {
    let e = cell_lines(&["        #text \"x was\""]);
    let a = cell_lines(&[
        "        a href=\"x\"",
        "          #text \"x\"",
        "        #text \" was\"",
    ]);
    assert!(!is_html5lib_cell_space_hunk(&e, &a, &cell_hunk(&e, &a)));
}

#[test]
fn cell_space_hunk_rejects_non_formatting_element() {
    let e = cell_lines(&["        #text \" was\""]);
    let a = cell_lines(&[
        "        div",
        "          #text \" \"",
        "        #text \"was\"",
    ]);
    assert!(!is_html5lib_cell_space_hunk(&e, &a, &cell_hunk(&e, &a)));
}

#[test]
fn cell_space_hunk_rejects_text_outside_a_cell() {
    let e = ["    div", "      p", "        #text \" was\""];
    let a = [
        "    div",
        "      p",
        "        a href=\"x\"",
        "          #text \" \"",
        "        #text \"was\"",
    ];
    assert!(!is_html5lib_cell_space_hunk(&e, &a, &cell_hunk(&e, &a)));
}
