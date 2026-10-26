use html5ever::local_name;

use super::*;

fn tag(count: usize) -> String {
    let attrs: String = (0..count).map(|i| format!(" a{i}")).collect();
    format!("<x{attrs}>")
}

fn too_large() -> ExtractError {
    ExtractError::Rejected(Rejection::TreeTooLarge)
}

#[test]
fn pauses_after_raw_text_start_tags() {
    let html = "<p><SCRIPT type=x>rest";
    let mut scanner = TagScanner::new();
    assert_eq!(
        scanner.scan(html, 0, html.len()).unwrap(),
        (18, Pause::RawTextTag(local_name!("script")))
    );
    for html in ["<scripts>", "</script>", "<p>"] {
        assert_eq!(
            TagScanner::new().scan(html, 0, html.len()).unwrap(),
            (html.len(), Pause::End),
            "{html}"
        );
    }
}

#[test]
fn pauses_after_the_bang_of_cdata() {
    let html = "ab<![CDATA[";
    let mut scanner = TagScanner::new();
    assert_eq!(
        scanner.scan(html, 0, html.len()).unwrap(),
        (4, Pause::Cdata)
    );
    scanner.resume_at_cdata(true);
    assert_eq!(
        scanner.scan(html, 4, html.len()).unwrap(),
        (html.len(), Pause::End)
    );

    // A piece that ends between `<` and `!` pauses in the next one, still just past the `!`.
    let mut scanner = TagScanner::new();
    assert_eq!(scanner.scan(html, 0, 3).unwrap(), (3, Pause::End));
    assert_eq!(
        scanner.scan(html, 3, html.len()).unwrap(),
        (4, Pause::Cdata)
    );

    let scan_after_cdata = |html: &str, foreign: bool| {
        let mut scanner = TagScanner::new();
        let (cut, pause) = scanner.scan(html, 0, html.len()).unwrap();
        assert_eq!((cut, pause), (2, Pause::Cdata));
        scanner.resume_at_cdata(foreign);
        scanner.scan(html, cut, html.len())
    };
    let big = tag(MAX_TAG_ATTRIBUTES + 1);
    assert!(scan_after_cdata(&format!("<![CDATA[{big}]]>"), true).is_ok());
    assert_eq!(
        scan_after_cdata(&format!("<![CDATA[]]>{big}"), true).unwrap_err(),
        too_large()
    );
    assert_eq!(
        scan_after_cdata(&format!("<![CDATA[>{big}"), false).unwrap_err(),
        too_large()
    );
}

#[test]
fn a_pause_includes_the_bom_that_follows() {
    let bom = char::from_u32(0xFEFF).unwrap();
    let html = format!("<title>{bom}{bom}x");
    let mut scanner = TagScanner::new();
    assert_eq!(
        scanner.scan(&html, 0, html.len()).unwrap(),
        (7 + 6, Pause::RawTextTag(local_name!("title")))
    );
}

#[test]
fn raw_text_ends_at_its_own_end_tag_only() {
    let big = tag(MAX_TAG_ATTRIBUTES + 1);
    let scan = |html: &str| {
        let mut scanner = TagScanner::new();
        let (cut, pause) = scanner.scan(html, 0, html.len())?;
        assert!(matches!(pause, Pause::RawTextTag(_)));
        scanner.resume_after_tag(true);
        scanner.scan(html, cut, html.len())
    };
    assert!(scan(&format!("<style></styles>{big}")).is_ok());
    assert!(scan(&format!("<style></STYLE{big}")).is_ok());
    assert_eq!(
        scan(&format!("<style></STYLE >{big}")).unwrap_err(),
        too_large()
    );
    assert_eq!(
        scan(&format!("<style></style/>{big}")).unwrap_err(),
        too_large()
    );
}

#[test]
fn unswitched_raw_text_tag_stays_in_data() {
    let big = tag(MAX_TAG_ATTRIBUTES + 1);
    let html = format!("<title>{big}");
    let mut scanner = TagScanner::new();
    let (cut, _) = scanner.scan(&html, 0, html.len()).unwrap();
    scanner.resume_after_tag(false);
    assert_eq!(
        scanner.scan(&html, cut, html.len()).unwrap_err(),
        too_large()
    );
}
