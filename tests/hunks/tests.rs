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
