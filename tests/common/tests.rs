#[test]
fn commit_is_read_from_the_upstream_table() {
    let manifest = r#"
[package]
version = "0.0.0"
commit = "wrong"

[package.metadata.other]
commit = "also wrong"

[package.metadata.upstream]
version = "1"
commits = "not the key"
commit = "abc123"

[dependencies]
commit = "late"
"#;
    assert_eq!(super::upstream_commit(manifest).as_deref(), Some("abc123"));
    assert_eq!(super::upstream_version(manifest).as_deref(), Some("1"));
    let no_table = "[package]
version = \"0.0.0\"
commit = \"x\"
";
    assert_eq!(super::upstream_commit(no_table), None);
    assert_eq!(super::upstream_version(no_table), None);
}
