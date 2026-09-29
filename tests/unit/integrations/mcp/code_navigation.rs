use super::*;
use crate::workspace::ChangeLayer;
use axum::extract::Query;

#[tokio::test]
async fn code_source_is_node_resolved_snapshot_bound_and_stale_safe() {
    let (state, root) = origin_test_state();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(
        root.path().join("src/lib.rs"),
        "fn callee() {}\nfn target_feature() { callee(); }\nfn caller() { target_feature(); }\n",
    )
    .unwrap();
    let workspace_id = state.workspaces.default_id().to_owned();
    let (_, workspace) = state.workspaces.select(Some(&workspace_id)).unwrap();
    let graph = state
        .harness
        .software_graph(workspace_id.clone(), &workspace, "src", 100, 500)
        .unwrap();
    let target_node_id = graph
        .graph
        .nodes
        .values()
        .find(|node| node.label.ends_with("target_feature"))
        .map(|node| node.id.clone())
        .expect("target_feature graph node");
    let snapshot_id = state.harness.graph_history(&workspace, 1).unwrap()[0]
        .id
        .clone();

    let unauthorized = intelligence_web_code_source(
        State(state.clone()),
        HeaderMap::new(),
        Query(IntelligenceCodeSourceQuery {
            node_id: target_node_id.clone(),
            snapshot_id: snapshot_id.clone(),
            context_lines: Some(1),
        }),
    )
    .await;
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let response = intelligence_web_code_source(
        State(state.clone()),
        ui_headers(&state, &workspace_id),
        Query(IntelligenceCodeSourceQuery {
            node_id: target_node_id.clone(),
            snapshot_id: snapshot_id.clone(),
            context_lines: Some(1),
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let body = response_json(response).await;
    assert_eq!(body["workspace"], workspace_id);
    assert_eq!(body["source"]["snapshot_id"], snapshot_id);
    assert_eq!(body["source"]["node_id"], target_node_id);
    assert_eq!(body["source"]["path"], "src/lib.rs");
    assert_eq!(body["source"]["precision"], "syntax");
    assert!(body["source"]["content"]
        .as_str()
        .unwrap()
        .contains("target_feature"));
    assert_eq!(
        body["source"]["source_revision"]
            .as_str()
            .unwrap()
            .strip_prefix("sha256:")
            .unwrap(),
        body["source"]["current_sha256"].as_str().unwrap()
    );

    fs::write(
        root.path().join("src/lib.rs"),
        "fn callee() {}\nfn target_feature() { changed(); }\nfn changed() {}\n",
    )
    .unwrap();
    let stale = intelligence_web_code_source(
        State(state.clone()),
        ui_headers(&state, &workspace_id),
        Query(IntelligenceCodeSourceQuery {
            node_id: target_node_id,
            snapshot_id,
            context_lines: Some(1),
        }),
    )
    .await;
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let stale = response_json(stale).await;
    assert_eq!(stale["code"], "stale_source");
    assert!(stale.get("source").is_none());
    assert!(!stale.to_string().contains("changed()"));
}

#[tokio::test]
async fn current_change_symbol_refreshes_stale_graph_before_exact_drilldown() {
    let (mut state, root) = origin_test_state();
    fs::create_dir_all(root.path().join("src")).unwrap();
    let git = |args: &[&str]| {
        let output = std::process::Command::new("git")
            .current_dir(root.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    git(&["init", "-q"]);
    fs::write(root.path().join("src/lib.rs"), "fn original() {}\n").unwrap();
    git(&["add", "src/lib.rs"]);
    git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "-c",
        "commit.gpgSign=false",
        "commit",
        "-qm",
        "fixture",
    ]);
    Arc::get_mut(&mut state).unwrap().workspaces =
        Workspaces::new([root.path()], false, true).unwrap();

    let workspace_id = state.workspaces.default_id().to_owned();
    let (_, workspace) = state.workspaces.select(Some(&workspace_id)).unwrap();
    state
        .harness
        .software_graph(workspace_id.clone(), &workspace, ".", 100, 500)
        .unwrap();
    let stale_snapshot = state.harness.graph_history(&workspace, 1).unwrap()[0]
        .id
        .clone();

    fs::write(
        root.path().join("src/lib.rs"),
        "fn original() {}\nfn added_after_snapshot() {}\n",
    )
    .unwrap();
    let detail = intelligence_web_change_detail(
        State(state.clone()),
        ui_headers(&state, &workspace_id),
        Query(IntelligenceChangeQuery {
            path: "src/lib.rs".into(),
            layer: ChangeLayer::Working,
            expected_snapshot: None,
        }),
    )
    .await;
    assert_eq!(detail.status(), StatusCode::OK);
    let detail = response_json(detail).await;
    let repository_revision = detail["repository_revision"].clone();
    let node_id = detail["symbol_impact"]["after_symbols"]
        .as_array()
        .unwrap()
        .iter()
        .find(|symbol| symbol["name"] == "added_after_snapshot")
        .and_then(|symbol| symbol["node_id"].as_str())
        .unwrap()
        .to_owned();
    let expected_code_revision = repository_revision["code"].as_str().unwrap().to_owned();
    let expected_design_revision = repository_revision["design"].as_str().map(str::to_owned);

    let focus = intelligence_web_code_graph(
        State(state.clone()),
        ui_headers(&state, &workspace_id),
        Query(IntelligenceCodeGraphQuery {
            view: Some("focus".into()),
            q: None,
            node_id: Some(node_id.clone()),
            snapshot_id: None,
            depth: Some(1),
            limit: Some(40),
            mode: Some(GraphChainMode::Calls),
            expected_code_revision: Some(expected_code_revision.clone()),
            expected_design_revision: expected_design_revision.clone(),
        }),
    )
    .await;
    assert_eq!(focus.status(), StatusCode::OK);
    let focus = response_json(focus).await;
    assert_eq!(focus["repository_revision"], repository_revision);
    assert_ne!(focus["graph"]["snapshot_id"], stale_snapshot);
    assert!(focus["graph"]["root_ids"]
        .as_array()
        .unwrap()
        .iter()
        .any(|id| id == &node_id));

    fs::write(
        root.path().join("src/other.rs"),
        "fn concurrent_change() {}\n",
    )
    .unwrap();
    let stale = intelligence_web_code_graph(
        State(state.clone()),
        ui_headers(&state, &workspace_id),
        Query(IntelligenceCodeGraphQuery {
            view: Some("focus".into()),
            q: None,
            node_id: Some(node_id),
            snapshot_id: None,
            depth: Some(1),
            limit: Some(40),
            mode: Some(GraphChainMode::Calls),
            expected_code_revision: Some(expected_code_revision),
            expected_design_revision,
        }),
    )
    .await;
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    assert_eq!(response_json(stale).await["code"], "stale_change");
}
