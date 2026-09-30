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
            start_line: None,
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
            start_line: None,
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
            start_line: None,
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

#[tokio::test]
async fn file_source_pages_are_bounded_complete_and_sha_guarded() {
    let (state, root) = origin_test_state();
    fs::create_dir_all(root.path().join("src")).unwrap();
    let source = (1..=501)
        .map(|line| format!("// line {line}\n"))
        .collect::<String>();
    fs::write(root.path().join("src/long.rs"), &source).unwrap();
    let workspace_id = state.workspaces.default_id().to_owned();
    let (_, workspace) = state.workspaces.select(Some(&workspace_id)).unwrap();
    let graph = state
        .harness
        .software_graph(workspace_id.clone(), &workspace, "src", 100, 500)
        .unwrap();
    let node_id = graph
        .graph
        .nodes
        .values()
        .find(|node| node.attributes.get("path").and_then(Value::as_str) == Some("src/long.rs"))
        .unwrap()
        .id
        .clone();
    let snapshot_id = state.harness.graph_history(&workspace, 1).unwrap()[0]
        .id
        .clone();
    let mut combined = String::new();
    for (start, end) in [(1, 240), (241, 480), (481, 501)] {
        let response = intelligence_web_code_source(
            State(state.clone()),
            ui_headers(&state, &workspace_id),
            Query(IntelligenceCodeSourceQuery {
                node_id: node_id.clone(),
                snapshot_id: snapshot_id.clone(),
                context_lines: Some(0),
                start_line: Some(start),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        let page = &body["source"];
        assert_eq!(page["start_line"], start);
        assert_eq!(page["end_line"], end);
        assert_eq!(page["total_lines"], 501);
        assert!(page["focus_start_line"].as_u64().unwrap() >= start as u64);
        assert!(page["focus_end_line"].as_u64().unwrap() <= end as u64);
        combined.push_str(page["content"].as_str().unwrap());
        if end != 501 {
            combined.push('\n');
        }
    }
    assert_eq!(combined.trim_end(), source.trim_end());
    let invalid = intelligence_web_code_source(
        State(state.clone()),
        ui_headers(&state, &workspace_id),
        Query(IntelligenceCodeSourceQuery {
            node_id: node_id.clone(),
            snapshot_id: snapshot_id.clone(),
            context_lines: None,
            start_line: Some(502),
        }),
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    fs::write(
        root.path().join("src/long.rs"),
        source.replace("line 501", "changed"),
    )
    .unwrap();
    let stale = intelligence_web_code_source(
        State(state.clone()),
        ui_headers(&state, &workspace_id),
        Query(IntelligenceCodeSourceQuery {
            node_id,
            snapshot_id,
            context_lines: None,
            start_line: Some(481),
        }),
    )
    .await;
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    assert_eq!(response_json(stale).await["code"], "stale_source");
}

#[tokio::test]
async fn symbol_source_page_without_intersection_has_no_focus() {
    let (state, root) = origin_test_state();
    fs::create_dir_all(root.path().join("src")).unwrap();
    let source = format!(
        "// header\nfn selected() {{}}\n{}",
        "// filler\n".repeat(500)
    );
    fs::write(root.path().join("src/paged.rs"), source).unwrap();
    let workspace_id = state.workspaces.default_id().to_owned();
    let (_, workspace) = state.workspaces.select(Some(&workspace_id)).unwrap();
    let graph = state
        .harness
        .software_graph(workspace_id.clone(), &workspace, "src", 100, 500)
        .unwrap();
    let node_id = graph
        .graph
        .nodes
        .values()
        .find(|node| {
            node.attributes.get("path").and_then(Value::as_str) == Some("src/paged.rs")
                && node.attributes.contains_key("range")
                && node.label.ends_with("selected")
        })
        .unwrap()
        .id
        .clone();
    let snapshot_id = state.harness.graph_history(&workspace, 1).unwrap()[0]
        .id
        .clone();
    for (start, expected_focus) in [(1, 2), (241, 0)] {
        let response = intelligence_web_code_source(
            State(state.clone()),
            ui_headers(&state, &workspace_id),
            Query(IntelligenceCodeSourceQuery {
                node_id: node_id.clone(),
                snapshot_id: snapshot_id.clone(),
                context_lines: None,
                start_line: Some(start),
            }),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["source"]["start_line"], start);
        assert_eq!(body["source"]["focus_start_line"], expected_focus);
        assert_eq!(body["source"]["focus_end_line"], expected_focus);
        assert_eq!(body["source"]["symbol_start_line"], 2);
        assert_eq!(body["source"]["symbol_end_line"], 2);
    }
}

#[tokio::test]
async fn empty_file_source_has_explicit_empty_range() {
    let (state, root) = origin_test_state();
    fs::create_dir_all(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/empty.rs"), "").unwrap();
    let workspace_id = state.workspaces.default_id().to_owned();
    let (_, workspace) = state.workspaces.select(Some(&workspace_id)).unwrap();
    let graph = state
        .harness
        .software_graph(workspace_id.clone(), &workspace, "src", 100, 500)
        .unwrap();
    let node_id = graph
        .graph
        .nodes
        .values()
        .find(|node| node.attributes.get("path").and_then(Value::as_str) == Some("src/empty.rs"))
        .unwrap()
        .id
        .clone();
    let snapshot_id = state.harness.graph_history(&workspace, 1).unwrap()[0]
        .id
        .clone();
    let response = intelligence_web_code_source(
        State(state.clone()),
        ui_headers(&state, &workspace_id),
        Query(IntelligenceCodeSourceQuery {
            node_id,
            snapshot_id,
            context_lines: None,
            start_line: None,
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    assert_eq!(body["source"]["total_lines"], 0);
    assert_eq!(body["source"]["end_line"], 0);
    assert_eq!(body["source"]["focus_start_line"], 0);
    assert_eq!(body["source"]["focus_end_line"], 0);
    assert_eq!(body["source"]["content"], "");
    assert_eq!(body["source"]["truncated"], false);
}
