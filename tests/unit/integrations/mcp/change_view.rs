use super::*;
use crate::workspace::ChangeLayer;
use axum::extract::Query;

pub(super) fn change_query(
    path: &str,
    layer: ChangeLayer,
    snapshot: Option<String>,
) -> Query<IntelligenceChangeQuery> {
    Query(IntelligenceChangeQuery {
        path: path.into(),
        layer,
        expected_snapshot: snapshot,
    })
}

pub(super) fn change_state() -> (Arc<AppState>, tempfile::TempDir) {
    let (mut state, root) = origin_test_state();
    let git = |args: &[&str]| {
        let result = std::process::Command::new("git")
            .current_dir(root.path())
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    };
    git(&["init", "-q"]);
    fs::write(root.path().join("main.rs"), "fn original() {}\n").unwrap();
    git(&["add", "main.rs"]);
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
    (state, root)
}

#[tokio::test]
async fn change_detail_http_requires_token_origin_and_selected_workspace() {
    let (state, _root) = origin_test_state();
    let id = state.workspaces.default_id();
    let mut missing = ui_headers(&state, id);
    missing.remove("x-wcode-ui-token");
    let response = intelligence_web_change_detail(
        State(state.clone()),
        missing,
        change_query("main.rs", ChangeLayer::Working, None),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let mut foreign_origin = ui_headers(&state, id);
    foreign_origin.insert("origin", "https://foreign.invalid".parse().unwrap());
    let response = intelligence_web_change_detail(
        State(state.clone()),
        foreign_origin,
        change_query("main.rs", ChangeLayer::Working, None),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let response = intelligence_web_change_detail(
        State(state.clone()),
        ui_headers(&state, "unknown-workspace"),
        change_query("main.rs", ChangeLayer::Working, None),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn change_detail_http_returns_real_diff_and_conflict_without_stale_source() {
    let (state, root) = change_state();
    let id = state.workspaces.default_id();
    fs::write(root.path().join("main.rs"), "fn changed() {}\n").unwrap();
    let response = intelligence_web_change_detail(
        State(state.clone()),
        ui_headers(&state, id),
        change_query("main.rs", ChangeLayer::Working, None),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let body = response_json(response).await;
    assert_eq!(body["workspace"], id);
    assert!(body["repository_revision"]["code"]
        .as_str()
        .is_some_and(|value| !value.is_empty()));
    assert!(body["repository_revision"].get("design").is_some());
    assert_eq!(body["change"]["layer"], "working");
    let patch = body["change"]["content"].as_str().unwrap();
    assert!(patch.contains("-fn original()") && patch.contains("+fn changed()"));
    assert_eq!(body["change"]["before_changed_ranges"][0]["start_line"], 1);
    assert_eq!(body["change"]["before_changed_ranges"][0]["end_line"], 1);
    assert_eq!(body["change"]["after_changed_ranges"][0]["start_line"], 1);
    assert_eq!(body["change"]["after_changed_ranges"][0]["end_line"], 1);
    assert_eq!(body["change"]["changed_ranges_truncated"], false);
    let snapshot = body["change"]["snapshot_id"].as_str().unwrap().to_owned();
    fs::write(root.path().join("main.rs"), "fn newer() {}\n").unwrap();
    let response = intelligence_web_change_detail(
        State(state.clone()),
        ui_headers(&state, id),
        change_query("main.rs", ChangeLayer::Unstaged, Some(snapshot)),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CONFLICT);
    let body = response_json(response).await;
    assert_eq!(body["code"], "stale_change");
    assert!(body.get("change").is_none());
    assert!(!body.to_string().contains("newer()"));
}

#[tokio::test]
async fn change_detail_http_maps_only_current_after_lines_to_syntax_symbols() {
    let (state, root) = change_state();
    let id = state.workspaces.default_id();
    fs::write(
        root.path().join("main.rs"),
        "fn original() {}\nfn added_symbol() {}\n",
    )
    .unwrap();
    let response = intelligence_web_change_detail(
        State(state.clone()),
        ui_headers(&state, id),
        change_query("main.rs", ChangeLayer::Working, None),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let impact = &body["symbol_impact"];
    assert_eq!(impact["snapshot_id"], body["change"]["snapshot_id"]);
    assert_eq!(impact["path"], "main.rs");
    assert_eq!(impact["precision"], "syntax");
    assert_eq!(impact["provider"], "tree-sitter");
    assert_eq!(impact["mapping"], "before_after_line_overlap");
    assert_eq!(impact["before_symbols_available"], true);
    assert_eq!(impact["before_source_state"], "head");
    assert!(impact["before_symbols"].as_array().unwrap().is_empty());
    assert_eq!(impact["partial"], false);
    assert_eq!(impact["after_symbols"].as_array().unwrap().len(), 1);
    let symbol = &impact["after_symbols"][0];
    assert_eq!(symbol["name"], "added_symbol");
    assert_eq!(symbol["start_line"], 2);
    assert_eq!(symbol["end_line"], 2);
    assert!(symbol["node_id"]
        .as_str()
        .unwrap()
        .starts_with("symbol:ts:"));
    assert_eq!(impact["source_sha256"], body["change"]["worktree_sha256"]);
}

#[tokio::test]
async fn change_detail_http_maps_before_and_after_symbols_from_exact_snapshots() {
    let (state, root) = change_state();
    let id = state.workspaces.default_id();
    fs::write(root.path().join("main.rs"), "fn changed() {}\n").unwrap();

    let response = intelligence_web_change_detail(
        State(state.clone()),
        ui_headers(&state, id),
        change_query("main.rs", ChangeLayer::Working, None),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let impact = &body["symbol_impact"];
    assert_eq!(impact["before_symbols_available"], true);
    assert_eq!(impact["before_source_state"], "head");
    assert_eq!(impact["before_symbols"].as_array().unwrap().len(), 1);
    assert_eq!(impact["before_symbols"][0]["name"], "original");
    assert_eq!(impact["after_symbols"].as_array().unwrap().len(), 1);
    assert_eq!(impact["after_symbols"][0]["name"], "changed");
    assert_eq!(impact["precision"], "syntax");
    assert_eq!(impact["provider"], "tree-sitter");
}

#[tokio::test]
async fn change_detail_http_maps_unstaged_before_from_exact_index_and_after_from_worktree() {
    let (state, root) = change_state();
    let id = state.workspaces.default_id();
    fs::write(root.path().join("main.rs"), "fn staged_symbol() {}\n").unwrap();
    let status = std::process::Command::new("git")
        .current_dir(root.path())
        .args(["add", "main.rs"])
        .status()
        .unwrap();
    assert!(status.success());
    fs::write(root.path().join("main.rs"), "fn worktree_symbol() {}\n").unwrap();

    let response = intelligence_web_change_detail(
        State(state.clone()),
        ui_headers(&state, id),
        change_query("main.rs", ChangeLayer::Unstaged, None),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let impact = &body["symbol_impact"];
    assert_eq!(impact["mapping"], "before_after_line_overlap");
    assert_eq!(impact["before_source_state"], "index");
    assert_eq!(impact["before_symbols"].as_array().unwrap().len(), 1);
    assert_eq!(impact["before_symbols"][0]["name"], "staged_symbol");
    assert_eq!(impact["source_state"], "worktree");
    assert_eq!(impact["after_symbols"].as_array().unwrap().len(), 1);
    assert_eq!(impact["after_symbols"][0]["name"], "worktree_symbol");
}

#[tokio::test]
async fn change_detail_http_pairs_addition_only_body_edit_with_exact_before_symbol() {
    let (state, root) = change_state();
    let id = state.workspaces.default_id();
    fs::write(
        root.path().join("main.rs"),
        "fn same() {\n    let before = 1;\n}\n",
    )
    .unwrap();
    let status = std::process::Command::new("git")
        .current_dir(root.path())
        .args(["add", "main.rs"])
        .status()
        .unwrap();
    assert!(status.success());
    let status = std::process::Command::new("git")
        .current_dir(root.path())
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "-qm",
            "body fixture",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    fs::write(
        root.path().join("main.rs"),
        "fn same() {\n    let before = 1;\n    let added = 2;\n}\n",
    )
    .unwrap();

    let response = intelligence_web_change_detail(
        State(state.clone()),
        ui_headers(&state, id),
        change_query("main.rs", ChangeLayer::Working, None),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let impact = &body["symbol_impact"];
    assert_eq!(impact["after_symbols"].as_array().unwrap().len(), 1);
    assert_eq!(impact["after_symbols"][0]["name"], "same");
    assert_eq!(impact["before_symbols"].as_array().unwrap().len(), 1);
    assert_eq!(impact["before_symbols"][0]["name"], "same");
    assert_eq!(impact["before_symbols"][0]["counterpart_only"], true);
    assert_eq!(impact["counterpart_basis"], "qualified_name_kind_syntax");
    assert_eq!(impact["before_symbols"][0]["signature_redacted"], false);
    assert_eq!(impact["after_symbols"][0]["signature_redacted"], false);
    assert_eq!(
        impact["before_symbols"][0]["signature"],
        impact["after_symbols"][0]["signature"]
    );
}

#[tokio::test]
async fn change_detail_http_pairs_deletion_only_body_edit_with_exact_after_symbol() {
    let (state, root) = change_state();
    let id = state.workspaces.default_id();
    fs::write(
        root.path().join("main.rs"),
        "fn same() {\n    let keep = 1;\n    let removed = 2;\n}\n",
    )
    .unwrap();
    let status = std::process::Command::new("git")
        .current_dir(root.path())
        .args(["add", "main.rs"])
        .status()
        .unwrap();
    assert!(status.success());
    let status = std::process::Command::new("git")
        .current_dir(root.path())
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "-qm",
            "deletion fixture",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    fs::write(
        root.path().join("main.rs"),
        "fn same() {\n    let keep = 1;\n}\n",
    )
    .unwrap();

    let response = intelligence_web_change_detail(
        State(state.clone()),
        ui_headers(&state, id),
        change_query("main.rs", ChangeLayer::Working, None),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let impact = &body["symbol_impact"];
    assert_eq!(impact["before_symbols"].as_array().unwrap().len(), 1);
    assert_eq!(impact["before_symbols"][0]["name"], "same");
    assert_eq!(impact["before_symbols"][0]["counterpart_only"], false);
    assert_eq!(impact["after_symbols"].as_array().unwrap().len(), 1);
    assert_eq!(impact["after_symbols"][0]["name"], "same");
    assert_eq!(impact["after_symbols"][0]["counterpart_only"], true);
    assert_eq!(impact["counterpart_basis"], "qualified_name_kind_syntax");
}

#[tokio::test]
async fn change_detail_http_reports_path_local_definition_add_remove_without_rename_inference() {
    let (state, root) = change_state();
    let id = state.workspaces.default_id();
    fs::write(root.path().join("main.rs"), "fn old_name() {}\n").unwrap();
    let status = std::process::Command::new("git")
        .current_dir(root.path())
        .args(["add", "main.rs"])
        .status()
        .unwrap();
    assert!(status.success());
    let status = std::process::Command::new("git")
        .current_dir(root.path())
        .args([
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@example.invalid",
            "-c",
            "commit.gpgSign=false",
            "commit",
            "-qm",
            "rename-like fixture",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    fs::write(root.path().join("main.rs"), "fn new_name() {}\n").unwrap();

    let response = intelligence_web_change_detail(
        State(state.clone()),
        ui_headers(&state, id),
        change_query("main.rs", ChangeLayer::Working, None),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let impact = &body["symbol_impact"];
    assert_eq!(
        impact["definition_change_basis"],
        "qualified_name_kind_complete_syntax_outlines"
    );
    assert_eq!(impact["before_symbols"].as_array().unwrap().len(), 1);
    assert_eq!(impact["before_symbols"][0]["name"], "old_name");
    assert_eq!(impact["before_symbols"][0]["definition_change"], "removed");
    assert_eq!(impact["after_symbols"].as_array().unwrap().len(), 1);
    assert_eq!(impact["after_symbols"][0]["name"], "new_name");
    assert_eq!(impact["after_symbols"][0]["definition_change"], "added");
    assert!(impact["before_symbols"][0]["counterpart_only"] == false);
    assert!(impact["after_symbols"][0]["counterpart_only"] == false);
}

#[tokio::test]
async fn change_detail_http_never_maps_staged_ranges_onto_a_newer_unstaged_worktree() {
    let (state, root) = change_state();
    let id = state.workspaces.default_id();
    fs::write(root.path().join("main.rs"), "fn staged_symbol() {}\n").unwrap();
    let status = std::process::Command::new("git")
        .current_dir(root.path())
        .args(["add", "main.rs"])
        .status()
        .unwrap();
    assert!(status.success());
    fs::write(root.path().join("main.rs"), "fn worktree_symbol() {}\n").unwrap();

    let response = intelligence_web_change_detail(
        State(state.clone()),
        ui_headers(&state, id),
        change_query("main.rs", ChangeLayer::Staged, None),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = response_json(response).await;
    let impact = &body["symbol_impact"];
    assert_eq!(body["change"]["after_source_matches_worktree"], false);
    assert_eq!(impact["source_state"], "index");
    assert!(impact["unavailable_reason"].is_null());
    assert_eq!(impact["after_symbols"].as_array().unwrap().len(), 1);
    assert_eq!(impact["after_symbols"][0]["name"], "staged_symbol");
    assert_ne!(impact["source_sha256"], body["change"]["worktree_sha256"]);
    assert!(!body.to_string().contains("worktree_symbol"));
}

#[tokio::test]
async fn change_detail_http_allows_protected_paths_and_keeps_path_invariants() {
    let (state, root) = change_state();
    let id = state.workspaces.default_id();
    fs::write(root.path().join(".env"), "private-file-contents").unwrap();
    for (path, allowed) in [(".env", true), ("../outside.txt", false)] {
        let response = intelligence_web_change_detail(
            State(state.clone()),
            ui_headers(&state, id),
            change_query(path, ChangeLayer::Working, None),
        )
        .await;
        assert_eq!(response.status() == StatusCode::OK, allowed, "{path}");
        let body = response_json(response).await;
        assert_eq!(body.get("change").is_some(), allowed, "{path}");
        if !allowed {
            assert!(!body.to_string().contains("private-file-contents"));
        }
    }
}
