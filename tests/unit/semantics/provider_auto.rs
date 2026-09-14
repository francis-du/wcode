use super::*;

#[test]
fn automatic_discovery_scans_beyond_the_index_output_limit() {
    assert_eq!(automatic_scan_limit(1), 32);
    assert_eq!(automatic_scan_limit(128), 4_096);
    assert_eq!(automatic_scan_limit(MAX_PROVIDER_FILES), 8_192);
    assert_eq!(automatic_scan_limit(usize::MAX), MAX_AUTO_DISCOVERY_FILES);
}

#[test]
fn automatic_fingerprint_tracks_workspace_configuration_inputs() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.1.0'\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let first = automatic_configuration_stamps(&workspace);
    assert!(first.iter().any(|(path, _)| path == "Cargo.toml"));
    std::thread::sleep(std::time::Duration::from_millis(2));
    std::fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname='demo'\nversion='0.2.0'\n",
    )
    .unwrap();
    let second = automatic_configuration_stamps(&workspace);
    assert_ne!(first, second);
}

#[test]
fn automatic_sources_are_grouped_before_provider_resolution() {
    let groups = automatic_source_groups(vec![
        ("src/one.rs".to_owned(), (10, 100)),
        ("src/two.rs".to_owned(), (20, 200)),
        ("web/app.ts".to_owned(), (30, 300)),
        ("README.md".to_owned(), (40, 400)),
    ]);

    assert_eq!(groups.len(), 1);
    assert_eq!(
        groups.get(&SemanticLanguage::Rust),
        Some(&vec![
            ("src/one.rs".to_owned(), (10, 100)),
            ("src/two.rs".to_owned(), (20, 200)),
        ])
    );
}
