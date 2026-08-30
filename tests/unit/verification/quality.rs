use super::*;
use std::fs;

#[test]
fn registry_represents_every_indexed_language_without_a_fake_support_boolean() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let registry = registry(&workspace).unwrap();
    assert_eq!(registry.languages.len(), SemanticLanguage::ALL.len());
    assert_eq!(registry.detected_languages, 1);
    let rust = registry
        .languages
        .iter()
        .find(|status| status.language == SemanticLanguage::Rust)
        .unwrap();
    assert_eq!(rust.detected_files, 1);
    assert!(rust.syntax_available);
    assert!(rust
        .providers
        .iter()
        .any(|provider| provider.id == "rustfmt" && provider.declared));
    assert!(registry
        .dimensions
        .iter()
        .any(|dimension| dimension.dimension == "fuzz"));
}

#[test]
fn repository_scripts_are_first_class_quality_providers() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
            dir.path().join("package.json"),
            r#"{"scripts":{"lint":"eslint .","typecheck":"tsc --noEmit","test":"vitest run"},"devDependencies":{"typescript":"latest"}}"#,
        )
        .unwrap();
    fs::write(dir.path().join("tsconfig.json"), "{}\n").unwrap();
    fs::write(dir.path().join("index.ts"), "export const x: number = 1;\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let registry = registry(&workspace).unwrap();
    let typescript = registry
        .languages
        .iter()
        .find(|status| status.language == SemanticLanguage::TypeScript)
        .unwrap();
    assert!(typescript
        .providers
        .iter()
        .any(|provider| provider.id == "package-lint" && provider.declared));
    assert!(typescript
        .providers
        .iter()
        .any(|provider| provider.id == "package-typecheck" && provider.declared));
    let package_lint = typescript
        .providers
        .iter()
        .find(|provider| provider.id == "package-lint")
        .unwrap();
    assert!(package_lint.declared);
    assert!(!package_lint.check_only);
    assert!(package_lint.reason.contains("discovery only"));
}

#[tokio::test]
async fn arbitrary_repository_scripts_cannot_enter_the_strict_check_only_lane() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("package.json"),
        r#"{"scripts":{"lint":"node mutate-repository.js"}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("index.js"), "export const value = 1;\n").unwrap();
    let workspace = Workspace::new(dir.path(), false, true).unwrap();
    let error = execute(&workspace, SemanticLanguage::JavaScript, "package-lint", 30)
        .await
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("not statically guaranteed check-only"));
}
