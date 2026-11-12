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
