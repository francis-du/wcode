use super::*;

#[test]
fn exact_symbol_outranks_helpers_across_scope_roots() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src/app")).unwrap();
    fs::create_dir_all(root.path().join("src/runtime")).unwrap();
    let helpers = (0..24)
        .map(|index| format!("pub fn parallel_tools_helper_{index}() {{}}\n"))
        .collect::<String>();
    fs::write(root.path().join("src/app/helpers.rs"), helpers).unwrap();
    fs::write(
        root.path().join("src/runtime/target.rs"),
        "pub fn parallel_tools() -> usize { 7 }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    for _ in 0..2 {
        let pack = harness
            .agent_context("demo", &workspace, "parallel_tools", 0, &["runtime".into()])
            .unwrap();
        assert_eq!(pack["targets"][0]["qualified_name"], "parallel_tools");
        assert_eq!(pack["hot_source"][0]["qualified_name"], "parallel_tools");
        assert_eq!(pack["files"][0]["path"], "src/runtime/target.rs");
        assert!(pack["hot_source"][0]["body"]["content"]
            .as_str()
            .unwrap()
            .contains("7"));
    }
}

#[test]
fn verified_experience_boosts_edit_ripple_without_outranking_exact_target() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/target.rs"),
        "pub fn target_feature() -> usize { 1 }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("src/ripple.rs"),
        "pub fn ripple_support() -> usize { 2 }\n",
    )
    .unwrap();
    let distractors = (0..24)
        .map(|index| format!("pub fn unrelated_{index}() -> usize {{ {index} }}\n"))
        .collect::<String>();
    fs::write(root.path().join("src/distractors.rs"), distractors).unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    crate::experience_store::persist_verified_change(
        &workspace,
        &crate::evidence::Revision {
            code: "sha256:verified-fixture".into(),
            design: None,
        },
        "full",
        &["src/target.rs".into(), "src/ripple.rs".into()],
    )
    .unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let pack = harness
        .agent_context("demo", &workspace, "target_feature impact", 4_000, &[])
        .unwrap();
    let items = pack["repo_map"]["items"].as_array().unwrap();
    let target_index = items
        .iter()
        .position(|item| item["qualified_name"] == "target_feature")
        .unwrap();
    let ripple_index = items
        .iter()
        .position(|item| item["qualified_name"] == "ripple_support")
        .unwrap();
    assert!(
        target_index < ripple_index,
        "exact target must remain strongest"
    );
    assert_eq!(items[target_index]["reason"], "direct_match");
    assert_eq!(items[ripple_index]["reason"], "verified_experience");
    assert_eq!(
        items[ripple_index]["historical_relevance"]["precision"],
        "heuristic"
    );
    assert_eq!(items[ripple_index]["historical_relevance"]["weight"], 500);
    assert_eq!(
        items[ripple_index]["historical_relevance"]["model"],
        "verified-context-cochange-v3"
    );
    assert_eq!(pack["repo_map"]["experience"]["matched_records"], 1);
}

#[test]
fn code_to_test_routing_prefers_tests_without_outranking_the_exact_source_target() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::create_dir_all(root.path().join("tests")).unwrap();
    fs::write(
        root.path().join("src/target.rs"),
        "pub fn target_feature() -> usize { 1 }\npub fn target_feature_helper() -> usize { 2 }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("tests/target.rs"),
        "#[test]\nfn target_feature_regression() { assert_eq!(1, 1); }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let pack = harness
        .agent_context(
            "demo",
            &workspace,
            "which tests verify target_feature",
            4_000,
            &[],
        )
        .unwrap();
    assert_eq!(pack["repo_map"]["routing"]["intent"], "code_to_test");
    assert_eq!(pack["repo_map"]["routing"]["specialized"], true);
    let items = pack["repo_map"]["items"].as_array().unwrap();
    let target_index = items
        .iter()
        .position(|item| item["qualified_name"] == "target_feature")
        .unwrap();
    let test_index = items
        .iter()
        .position(|item| item["qualified_name"] == "target_feature_regression")
        .unwrap();
    let helper_index = items
        .iter()
        .position(|item| item["qualified_name"] == "target_feature_helper")
        .unwrap();
    assert!(
        target_index < test_index,
        "exact target must remain strongest"
    );
    assert!(
        test_index < helper_index,
        "test intent should prefer the regression test over a lexical helper"
    );
}

#[test]
fn qualified_symbol_in_prose_is_kept_whole() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        "pub fn cancellation() {}\npub fn parallel_item_error() {}\npub struct Worker;\nimpl Worker { pub fn parallel_tools(&self) {} }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let pack = harness
        .agent_context(
            "demo",
            &workspace,
            "inspect Worker::parallel_tools cancellation",
            0,
            &[],
        )
        .unwrap();
    assert_eq!(
        pack["targets"][0]["qualified_name"],
        "Worker::parallel_tools"
    );
    assert_eq!(
        pack["hot_source"][0]["qualified_name"],
        "Worker::parallel_tools"
    );
}
