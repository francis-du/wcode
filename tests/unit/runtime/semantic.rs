use super::*;

#[tokio::test(flavor = "current_thread")]
async fn worker_panics_are_contained_for_restart() {
    assert!(worker_completed(async {}).await);
    assert!(!worker_completed(async { panic!("fixture panic") }).await);
}

#[test]
fn cached_provider_coverage_is_success_even_when_warm_session_is_degraded() {
    assert!(automatic_refresh_succeeded(1, 1));
    assert!(automatic_refresh_succeeded(2, 2));
    assert!(!automatic_refresh_succeeded(1, 0));
    assert!(!automatic_refresh_succeeded(2, 1));
    assert!(!automatic_refresh_succeeded(0, 0));
}

#[test]
fn retry_backoff_is_bounded() {
    assert_eq!(
        doubled_retry(Duration::from_secs(10)),
        Duration::from_secs(20)
    );
    assert_eq!(doubled_retry(Duration::from_secs(200)), MAX_RETRY);
    assert_eq!(doubled_retry(MAX_RETRY), MAX_RETRY);
}

#[test]
fn semantic_execution_is_default_on_and_can_be_disabled() {
    assert!(crate::workspace::WorkspaceSecurity::default().allow_semantic_exec);
    let dir = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new_with_security(
        [dir.path()],
        false,
        true,
        crate::workspace::WorkspaceSecurity {
            allow_semantic_exec: false,
            ..crate::workspace::WorkspaceSecurity::default()
        },
    )
    .unwrap();
    assert!(workspaces.semantic_workspaces().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn semantic_navigation_degrades_to_cross_file_syntax_calls() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("target.rs"), "pub fn target() {}\n").unwrap();
    std::fs::write(
        root.path().join("caller.rs"),
        "pub fn caller() {\n    target();\n}\n",
    )
    .unwrap();
    let workspace = crate::workspace::Workspace::new(root.path(), false, false).unwrap();
    let harness = crate::harness::ToolHarness::new(4).unwrap();
    let request = crate::harness::SemanticNavigationRequest {
        path: "target.rs".into(),
        symbol: Some("target".into()),
        line: None,
        character: None,
        intent: crate::semantic_provider::SemanticNavigationIntent::IncomingCalls,
        max_results: 10,
    };
    let result = harness
        .semantic_navigation("demo", &workspace, &request)
        .await
        .unwrap();
    assert_eq!(result["degraded"], true);
    assert_eq!(result["precision"], "syntax");
    assert!(result["fallback_capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|capability| capability == "syntax_call_graph"));
    assert!(result["syntax_calls"]["incoming_calls"]
        .as_array()
        .unwrap()
        .iter()
        .any(|caller| caller["name"] == "caller"));
}

#[tokio::test(flavor = "current_thread")]
async fn semantic_navigation_recovers_calls_beyond_repo_map_file_cap() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("a_noise")).unwrap();
    std::fs::create_dir_all(root.path().join("z_domain")).unwrap();
    for index in 0..650 {
        std::fs::write(
            root.path().join(format!("a_noise/item_{index:03}.rs")),
            format!("pub fn noise_{index}() {{}}\n"),
        )
        .unwrap();
    }
    std::fs::write(
        root.path().join("z_domain/target.rs"),
        "pub fn target_feature() {}\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("z_domain/caller.rs"),
        "pub fn invoke_target() {\n    target_feature();\n}\n",
    )
    .unwrap();
    let workspace = crate::workspace::Workspace::new(root.path(), false, false).unwrap();
    let harness = crate::harness::ToolHarness::new(4).unwrap();
    let request = crate::harness::SemanticNavigationRequest {
        path: "z_domain/target.rs".into(),
        symbol: Some("target_feature".into()),
        line: None,
        character: None,
        intent: crate::semantic_provider::SemanticNavigationIntent::IncomingCalls,
        max_results: 10,
    };
    let result = harness
        .semantic_navigation("demo", &workspace, &request)
        .await
        .unwrap();
    assert!(result["syntax_calls"]["incoming_calls"]
        .as_array()
        .unwrap()
        .iter()
        .any(|caller| caller["name"] == "invoke_target"));
}

#[test]
fn semantic_workers_skip_broad_parent_workspaces() {
    let root = tempfile::tempdir().unwrap();
    for (path, marker) in [("Rust/app", "Cargo.toml"), ("Web/app", "package.json")] {
        let project = root.path().join(path);
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(project.join(marker), "fixture\n").unwrap();
    }
    let workspaces = Workspaces::new([root.path()], false, true).unwrap();
    let semantic = workspaces.semantic_workspaces();
    assert_eq!(semantic.len(), 2);
    assert!(semantic
        .iter()
        .all(|(_, workspace)| workspace.root() != root.path()));
}
