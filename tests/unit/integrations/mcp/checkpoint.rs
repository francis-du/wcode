use super::*;

#[test]
fn verification_file_uri_locations_keep_only_workspace_files() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("inside.ts"),
        "throw new Error('inside');\n",
    )
    .unwrap();
    std::fs::write(
        outside.path().join("outside.ts"),
        "throw new Error('outside');\n",
    )
    .unwrap();
    let workspace = crate::workspace::Workspace::new(root.path(), false, false).unwrap();
    let inside_uri = url::Url::from_file_path(root.path().join("inside.ts"))
        .unwrap()
        .to_string();
    let outside_uri = url::Url::from_file_path(outside.path().join("outside.ts"))
        .unwrap()
        .to_string();

    let diagnostic = format!("at inside ({inside_uri}:1:7)\nat outside ({outside_uri}:1:9)");
    let locations = verification_file_uri_locations(&workspace, &diagnostic);

    assert_eq!(locations, vec![("inside.ts".to_owned(), 1, Some(7))]);
}

#[test]
fn verification_file_uri_location_parser_requires_explicit_line() {
    assert_eq!(
        split_file_uri_location("file:///tmp/example.ts:12:4"),
        Some(("file:///tmp/example.ts", 12, Some(4)))
    );
    assert_eq!(
        split_file_uri_location("file:///tmp/example.ts:12"),
        Some(("file:///tmp/example.ts", 12, None))
    );
    assert!(split_file_uri_location("https://example.com/app.ts:12:4").is_none());
    assert!(split_file_uri_location("file:///tmp/example.ts").is_none());
}
