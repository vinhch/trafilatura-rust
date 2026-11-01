use super::*;
use crate::error::Rejection;

fn titled(value: &str) -> Document {
    let mut document = Document::new();
    document.title = Some(value.to_owned());
    document
}

#[test]
fn clean_and_trim_truncates_by_code_points() {
    let mut over = titled(&"é".repeat(10_001));
    over.clean_and_trim().unwrap();
    // oracle: d = Document(); d.title = "é" * 10001; d.clean_and_trim() -> "é" * 9999 + "…"
    assert_eq!(
        over.title.as_deref(),
        Some(format!("{}…", "é".repeat(9_999)).as_str())
    );
    let mut exact = titled(&"é".repeat(10_000));
    exact.clean_and_trim().unwrap();
    assert_eq!(exact.title, Some("é".repeat(10_000)));
}

#[test]
fn clean_and_trim_unescapes_then_line_processes() {
    let mut document = titled(" a&amp;b \n");
    document.author = Some("&nbsp;".to_owned());
    document.tags = Some(vec![" &amp; ".to_owned()]);
    document.clean_and_trim().unwrap();
    // oracle: d.title = " a&amp;b \n"; d.author = "&nbsp;"; d.clean_and_trim()
    assert_eq!(document.title.as_deref(), Some("a&b"));
    assert_eq!(document.author, None);
    assert_eq!(document.tags, Some(vec![" &amp; ".to_owned()]));
}

#[test]
fn clean_and_trim_overlong_charref_fails() {
    let mut document = titled(&format!("&#{};", "1".repeat(4_301)));
    // oracle: html.unescape("&#" + "1" * 4301 + ";") raises ValueError
    assert_eq!(
        document.clean_and_trim(),
        Err(ExtractError::Rejected(Rejection::InvalidValue))
    );
}

#[test]
fn set_date_params_prefers_the_original_date() {
    // oracle: set_date_params(False)
    assert_eq!(
        set_date_params(false),
        DateParams {
            original_date: true,
            extensive_search: false,
            max_date: None,
        }
    );
    assert!(set_date_params(true).extensive_search);
}
