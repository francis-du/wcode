use super::*;

#[test]
fn automatic_discovery_scans_beyond_the_index_output_limit() {
    assert_eq!(automatic_scan_limit(1), 32);
    assert_eq!(automatic_scan_limit(128), 4_096);
    assert_eq!(automatic_scan_limit(MAX_PROVIDER_FILES), 8_192);
    assert_eq!(automatic_scan_limit(usize::MAX), MAX_AUTO_DISCOVERY_FILES);
}

#[test]
fn automatic_sources_are_grouped_before_provider_resolution() {
    let groups = automatic_source_groups(vec![
        "src/one.rs".to_owned(),
        "src/two.rs".to_owned(),
        "web/app.ts".to_owned(),
        "README.md".to_owned(),
    ]);

    assert_eq!(groups.len(), 1);
    assert_eq!(
        groups.get(&SemanticLanguage::Rust),
        Some(&vec!["src/one.rs".to_owned(), "src/two.rs".to_owned()])
    );
}
