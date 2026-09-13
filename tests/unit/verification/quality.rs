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
    let registry = registry(&workspace, None).unwrap();
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
        .any(|provider| { provider.id == "rustfmt" && provider.root == "." && provider.declared }));
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
    let registry = registry(&workspace, None).unwrap();
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

#[test]
fn polyglot_registry_scopes_nested_manifest_providers_to_their_islands() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(dir.path().join("src/lib.rs"), "pub fn demo() {}\n").unwrap();
    fs::create_dir_all(dir.path().join("mobile/lib")).unwrap();
    fs::write(
        dir.path().join("mobile/pubspec.yaml"),
        "name: mobile\nenvironment:\n  sdk: '>=3.0.0 <4.0.0'\n",
    )
    .unwrap();
    fs::write(dir.path().join("mobile/lib/app.dart"), "void main() {}\n").unwrap();
    fs::create_dir_all(dir.path().join("web/src")).unwrap();
    fs::write(
        dir.path().join("web/package.json"),
        r#"{"scripts":{"lint":"eslint ."},"devDependencies":{"eslint":"latest","typescript":"latest"}}"#,
    )
    .unwrap();
    fs::write(dir.path().join("web/tsconfig.json"), "{}\n").unwrap();
    fs::write(
        dir.path().join("web/src/app.ts"),
        "export const value = 1;\n",
    )
    .unwrap();

    let workspace = Workspace::new(dir.path(), false, false).unwrap();
    let project_roots = vec![
        (".".to_owned(), vec!["rust".to_owned()]),
        ("mobile".to_owned(), vec!["dart".to_owned()]),
        ("web".to_owned(), vec!["node".to_owned()]),
    ];
    let registry = registry_for_project_roots(&workspace, None, &project_roots).unwrap();
    assert_eq!(registry.provider, "wcode-language-quality-islands");
    let dart = registry
        .languages
        .iter()
        .find(|status| status.language == SemanticLanguage::Dart)
        .unwrap();
    assert!(dart.providers.iter().any(|provider| {
        provider.id == "mobile::dart-analyze" && provider.root == "mobile" && provider.declared
    }));
    let typescript = registry
        .languages
        .iter()
        .find(|status| status.language == SemanticLanguage::TypeScript)
        .unwrap();
    assert!(typescript.providers.iter().any(|provider| {
        provider.id == "web::package-lint" && provider.root == "web" && provider.declared
    }));
    let rust = registry
        .languages
        .iter()
        .find(|status| status.language == SemanticLanguage::Rust)
        .unwrap();
    assert!(rust
        .providers
        .iter()
        .any(|provider| provider.id == "rustfmt" && provider.root == "."));
    assert_eq!(
        scoped_provider_id("mobile::dart-analyze"),
        ("mobile", "dart-analyze")
    );
    assert_eq!(scoped_provider_id("rustfmt"), (".", "rustfmt"));
}

#[tokio::test]
async fn exact_quality_provider_shapes_reuse_the_autonomous_verification_lane() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"quality-lane\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    fs::create_dir(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/lib.rs"),
        "pub fn value() -> u8 {\n    1\n}\n",
    )
    .unwrap();
    let workspaces = crate::workspace::Workspaces::new([dir.path()], false, true).unwrap();
    let (_, workspace) = workspaces.select(None).unwrap();
    let status = registry(&workspace, None).unwrap();
    let rustfmt = status
        .languages
        .iter()
        .find(|status| status.language == SemanticLanguage::Rust)
        .and_then(|status| {
            status
                .providers
                .iter()
                .find(|provider| provider.id == "rustfmt")
        })
        .expect("rustfmt provider must be visible");
    assert!(rustfmt.runnable);
    assert!(!rustfmt.authorization_required);
    assert_eq!(
        rustfmt.execution_lane,
        QualityExecutionLane::AutonomousVerification
    );
    assert!(rustfmt
        .reason
        .contains("autonomous exact-shape verification"));

    let run = execute(&workspace, SemanticLanguage::Rust, "rustfmt", 30)
        .await
        .expect("exact check-only quality provider should reuse verification autonomy");
    assert!(run.success, "rustfmt failed: {}", run.command.stderr);
    assert_eq!(
        run.execution_lane,
        QualityExecutionLane::AutonomousVerification
    );
    assert!(workspaces.authorization_requests(10).is_empty());
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
