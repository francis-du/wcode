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
