use super::*;

#[test]
fn automatic_discovery_scans_beyond_the_index_output_limit() {
    assert_eq!(automatic_scan_limit(1), 32);
    assert_eq!(automatic_scan_limit(128), 4_096);
    assert_eq!(automatic_scan_limit(MAX_PROVIDER_FILES), 8_192);
    assert_eq!(automatic_scan_limit(usize::MAX), MAX_AUTO_DISCOVERY_FILES);
}

#[test]
fn provider_file_budget_preserves_rare_languages() {
    let rust = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "rust-analyzer")
        .unwrap();
    let swift = PROVIDERS
        .iter()
        .copied()
        .find(|provider| provider.id == "sourcekit-lsp")
        .unwrap();
    let mut assignments = BTreeMap::from([
        (
            rust.id.to_owned(),
            (
                rust,
                PathBuf::from("/tmp/rust-analyzer"),
                (0..200)
                    .map(|index| (format!("src/item_{index:03}.rs"), SemanticLanguage::Rust))
                    .collect(),
            ),
        ),
        (
            swift.id.to_owned(),
            (
                swift,
                PathBuf::from("/tmp/sourcekit-lsp"),
                vec![("tests/late.swift".to_owned(), SemanticLanguage::Swift)],
            ),
        ),
    ]);

    assert!(trim_provider_assignments(&mut assignments, 128));
    assert_eq!(
        assignments
            .values()
            .map(|(_, _, files)| files.len())
            .sum::<usize>(),
        128
    );
    assert_eq!(assignments[swift.id].2.len(), 1);
    assert_eq!(assignments[rust.id].2.len(), 127);
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
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("src")).unwrap();
    std::fs::create_dir_all(dir.path().join("web")).unwrap();
    let paths = ["src/one.rs", "src/two.rs", "web/app.ts", "README.md"];
    for path in paths {
        std::fs::write(dir.path().join(path), format!("// fixture {path}\n")).unwrap();
    }
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    // Use real stamps so platform-specific identity fields are preserved,
    // rather than manufacturing the obsolete (length, mtime) tuple.
    let inputs = paths
        .iter()
        .map(|path| {
            (
                (*path).to_owned(),
                workspace.source_metadata_stamp(path).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let expected = inputs[..2].to_vec();
    let groups = automatic_source_groups(inputs);

    assert_eq!(groups.len(), 1);
    assert_eq!(groups.get(&SemanticLanguage::Rust), Some(&expected));
}
