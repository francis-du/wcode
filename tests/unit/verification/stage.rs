use super::*;

#[test]
fn executor_config_can_cover_languages_without_builtin_frameworks() {
    let yaml = r#"
schema_version: 1
executors:
  - id: custom-css-property
    stage: property
    languages: [css]
    program: make
    args: [check]
"#;
    let config: ExecutorConfig = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(config.executors.len(), 1);
    assert_eq!(config.executors[0].languages, vec![SemanticLanguage::Css]);
}

#[test]
fn every_language_is_representable_in_executor_config() {
    let represented = SemanticLanguage::ALL.into_iter().collect::<BTreeSet<_>>();
    assert_eq!(represented.len(), 22);
}

#[test]
fn property_discovery_requires_framework_use_in_language_source() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname='property-fixture'\nversion='0.1.0'\n[dev-dependencies]\nproptest='1'\n",
    )
    .unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("src/lib.rs"),
        "pub fn value() -> u8 { 1 }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    assert!(!registry(&workspace)
        .unwrap()
        .executors
        .iter()
        .any(|entry| entry.spec.id == "builtin-rust-property"));

    std::fs::write(
        root.path().join("src/lib.rs"),
        "#[cfg(test)] mod tests { use proptest::prelude::*; proptest! { #[test] fn stable(v in 0u8..=9) { prop_assert!(v <= 9); } } }\n",
    )
    .unwrap();
    assert!(registry(&workspace)
        .unwrap()
        .executors
        .iter()
        .any(|entry| entry.spec.id == "builtin-rust-property"));
}

#[test]
fn js_property_uses_fixed_runner_and_ignores_arbitrary_test_scripts() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("package.json"),
        r#"{"scripts":{"test":"echo not-a-property-test","mutation":"echo fake"},"devDependencies":{"fast-check":"latest","vitest":"latest","@stryker-mutator/core":"latest"}}"#,
    )
    .unwrap();
    std::fs::write(
        root.path().join("property.test.js"),
        "import fc from 'fast-check'; fc.assert(fc.property(fc.integer(), () => true));\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let registry = registry(&workspace).unwrap();
    let property = registry
        .executors
        .iter()
        .find(|entry| entry.spec.id == "builtin-js-property")
        .expect("fast-check source use plus Vitest must expose a property executor");
    assert!(property.spec.program.ends_with("vitest"));
    assert_eq!(property.spec.args, vec!["run"]);
    assert!(!registry
        .executors
        .iter()
        .any(|entry| entry.spec.id == "builtin-js-mutation"));
}

#[test]
fn r_property_executor_uses_vanilla_startup() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("DESCRIPTION"),
        "Package: propertyfixture\nSuggests: quickcheck, testthat\n",
    )
    .unwrap();
    std::fs::create_dir_all(root.path().join("tests/testthat")).unwrap();
    std::fs::write(
        root.path().join("tests/testthat/property.R"),
        "quickcheck::for_all(a = quickcheck::integer_, property = function(a) TRUE)\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let registry = registry(&workspace).unwrap();
    let property = registry
        .executors
        .iter()
        .find(|entry| entry.spec.id == "builtin-r-property")
        .expect("declared and used quickcheck must expose the R property executor");
    assert_eq!(
        property.spec.args,
        vec!["--vanilla", "-e", "testthat::test_dir('tests/testthat')"]
    );
}

#[cfg(unix)]
#[test]
fn java_property_falls_back_from_non_executable_gradle_wrapper() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("build.gradle"),
        "dependencies { testImplementation 'net.jqwik:jqwik:1.9.3' }\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("PropertyTest.java"),
        "import net.jqwik.api.Property; class PropertyTest { @Property void works() {} }\n",
    )
    .unwrap();
    std::fs::write(root.path().join("gradlew"), "#!/bin/sh\nexit 0\n").unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let registry = registry(&workspace).unwrap();
    let property = registry
        .executors
        .iter()
        .find(|entry| entry.spec.id == "builtin-java-property")
        .expect("declared and used jqwik must expose Java property execution");
    assert_eq!(property.spec.program, "gradle");
}

#[tokio::test]
async fn safe_repository_test_executor_runs_without_authorization() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("Makefile"), "check:\n\t@true\n").unwrap();
    let workspaces = crate::workspace::Workspaces::new([root.path()], false, true).unwrap();
    let (_, workspace) = workspaces.select(None).unwrap();
    let executor = StageExecutorSpec {
        id: "repo-check".into(),
        stage: VerificationStage::Property,
        languages: vec![],
        program: "make".into(),
        args: vec!["check".into()],
        cwd: ".".into(),
        timeout_seconds: 10,
        builtin: false,
    };
    let result = execute(&workspace, &executor)
        .await
        .expect("bounded repository test executor should be autonomous");
    assert!(result.command.success, "{}", result.command.stderr);
    assert!(workspaces.authorization_requests(10).is_empty());
}
