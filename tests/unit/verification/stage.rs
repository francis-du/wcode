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
