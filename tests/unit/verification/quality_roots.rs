use super::*;

#[test]
fn root_quality_discovery_reuses_providers_without_repeating_filesystem_scans() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname='root-quality'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn answer() -> u8 { 42 }\n",
    )
    .unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let expected = registry(&workspace, None).unwrap();
    let expected_rust = expected
        .languages
        .iter()
        .find(|status| status.language == SemanticLanguage::Rust)
        .unwrap();
    crate::stage_executor::REGISTRY_CALLS.with(|count| count.set(0));
    // Duplicate root declarations must neither repeat I/O nor duplicate providers.
    let project_roots = vec![
        (".".to_owned(), vec!["rust".to_owned()]),
        (".".to_owned(), vec!["rust".to_owned()]),
    ];
    let actual = registry_for_project_roots(&workspace, None, &project_roots).unwrap();
    assert_eq!(
        crate::stage_executor::REGISTRY_CALLS.with(|count| count.get()),
        1
    );
    let actual_rust = actual
        .languages
        .iter()
        .find(|status| status.language == SemanticLanguage::Rust)
        .unwrap();
    assert_eq!(actual.detected_languages, expected.detected_languages);
    assert_eq!(actual.truncated, expected.truncated);
    assert_eq!(actual_rust.detected_files, expected_rust.detected_files);
    assert_eq!(actual_rust.gaps, expected_rust.gaps);
    assert_eq!(actual_rust.providers.len(), expected_rust.providers.len());
    for provider in &expected_rust.providers {
        let found = actual_rust
            .providers
            .iter()
            .find(|value| value.id == provider.id)
            .unwrap();
        assert_eq!(
            serde_json::to_value(found).unwrap(),
            serde_json::to_value(provider).unwrap()
        );
    }
}
