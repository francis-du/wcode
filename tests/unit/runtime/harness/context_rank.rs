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

#[test]
fn mixed_cjk_prose_extracts_embedded_snake_case_symbol() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/context.rs"),
        "pub fn agent_context_fast_path() -> usize { 11 }\npub fn unrelated_context() -> usize { 3 }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let pack = harness
        .agent_context(
            "demo",
            &workspace,
            "继续优化agent_context_fast_path让它更快",
            0,
            &[],
        )
        .unwrap();

    assert_eq!(
        pack["targets"][0]["qualified_name"],
        "agent_context_fast_path"
    );
    assert_eq!(
        pack["hot_source"][0]["qualified_name"],
        "agent_context_fast_path"
    );
    assert_eq!(pack["files"][0]["path"], "src/context.rs");
}

#[test]
fn mixed_cjk_prose_extracts_embedded_camel_case_symbol() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/context.rs"),
        "pub struct AgentContextPlanner;\npub struct ContextPlannerNoise;\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let pack = harness
        .agent_context(
            "demo",
            &workspace,
            "请检查AgentContextPlanner调用链",
            0,
            &[],
        )
        .unwrap();

    assert_eq!(pack["targets"][0]["qualified_name"], "AgentContextPlanner");
    assert_eq!(
        pack["hot_source"][0]["qualified_name"],
        "AgentContextPlanner"
    );
}

#[test]
fn natural_language_prefers_symbols_covering_more_query_terms() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/workspace.rs"),
        "pub fn workspace_aaa_metrics() {}\npub fn workspace_command_guard() {}\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let pack = harness
        .agent_context(
            "demo",
            &workspace,
            "find workspace command guard behavior",
            0,
            &[],
        )
        .unwrap();

    assert_eq!(
        pack["targets"][0]["qualified_name"],
        "workspace_command_guard"
    );
    assert_eq!(
        pack["hot_source"][0]["qualified_name"],
        "workspace_command_guard"
    );
}

#[test]
fn natural_language_can_retrieve_business_terms_from_symbol_signature() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/guard.rs"),
        "pub fn enforce(workspace_command_policy: usize) -> bool { workspace_command_policy > 0 }\npub fn workspace_noise() {}\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let pack = harness
        .agent_context(
            "demo",
            &workspace,
            "where is workspace command policy enforced",
            0,
            &[],
        )
        .unwrap();

    assert_eq!(pack["targets"][0]["qualified_name"], "enforce");
    assert_eq!(pack["hot_source"][0]["qualified_name"], "enforce");
}

#[test]
fn task_words_do_not_outrank_domain_terms_during_symbol_retrieval() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/context.rs"),
        "pub fn behavior_find_where_check() {}\npub fn workspace_command_guard() {}\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let pack = harness
        .agent_context(
            "demo",
            &workspace,
            "find where check workspace command behavior",
            0,
            &[],
        )
        .unwrap();

    assert_eq!(
        pack["targets"][0]["qualified_name"],
        "workspace_command_guard"
    );
    assert_eq!(
        pack["hot_source"][0]["qualified_name"],
        "workspace_command_guard"
    );
}

#[test]
fn natural_language_behavior_lookup_prefers_production_over_test_name_overlap() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::create_dir_all(root.path().join("tests")).unwrap();
    fs::write(
        root.path().join("src/session.rs"),
        "pub fn cleanup_if_owner(old: u64, current: u64) -> bool { old == current }\npub fn refresh_session(old: u64, current: u64) -> bool { cleanup_if_owner(old, current) }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("tests/session.rs"),
        "#[test]\nfn replacement_keeps_new_owner() { assert!(true); }\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let pack = harness
        .agent_context(
            "demo",
            &workspace,
            "Find the ownership check that prevents an old session from cleaning up its replacement",
            4_000,
            &[],
        )
        .unwrap();

    let targets = pack["targets"].as_array().unwrap();
    assert_eq!(targets[0]["path"], "src/session.rs");
    assert!(
        targets
            .iter()
            .any(|target| target["qualified_name"] == "cleanup_if_owner"),
        "targets={targets:#?}"
    );
    assert!(
        targets
            .iter()
            .any(|target| target["qualified_name"] == "refresh_session"),
        "targets={targets:#?}"
    );
}

#[test]
fn relationship_queries_expand_the_top_graph_neighbor_body() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/target.rs"),
        "pub fn target_feature() -> usize { 7 }\n",
    )
    .unwrap();
    fs::write(
        root.path().join("src/caller.rs"),
        "pub fn invoke_target() -> usize {\n    target_feature()\n}\n",
    )
    .unwrap();
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    let pack = harness
        .agent_context(
            "demo",
            &workspace,
            "show callers of target_feature",
            4_000,
            &[],
        )
        .unwrap();

    let hot = pack["hot_source"].as_array().unwrap();
    assert_eq!(hot[0]["qualified_name"], "target_feature");
    assert!(hot
        .iter()
        .any(|source| source["qualified_name"] == "invoke_target"));
    assert!(
        pack["repo_map"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["reason"] == "caller_of_direct"),
        "repo_map={}",
        pack["repo_map"]
    );
}
