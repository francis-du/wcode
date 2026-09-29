use super::*;
use crate::workspace::ChangeLayer;
use axum::extract::Query;

fn change_query(
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

fn change_state() -> (Arc<AppState>, tempfile::TempDir) {
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
async fn change_symbol_impact_is_bound_to_the_selected_changed_symbol_and_snapshot() {
    let (state, root) = change_state();
    let id = state.workspaces.default_id();
    fs::write(
        root.path().join("main.rs"),
        "fn original() { helper(); }\nfn helper() {}\n",
    )
    .unwrap();
    fs::write(
        root.path().join("caller.rs"),
        "fn caller() { original(); }\n",
    )
    .unwrap();

    let detail = intelligence_web_change_detail(
        State(state.clone()),
        ui_headers(&state, id),
        change_query("main.rs", ChangeLayer::Working, None),
    )
    .await;
    assert_eq!(detail.status(), StatusCode::OK);
    let detail = response_json(detail).await;
    let repository_revision = detail["repository_revision"].clone();
    let expected_code_revision = repository_revision["code"].as_str().unwrap().to_owned();
    let expected_design_revision = repository_revision["design"].as_str().map(str::to_owned);
    let snapshot = detail["change"]["snapshot_id"].as_str().unwrap().to_owned();
    let source_sha256 = detail["change"]["worktree_sha256"]
        .as_str()
        .unwrap()
        .to_owned();
    let node_id = detail["symbol_impact"]["after_symbols"]
        .as_array()
        .unwrap()
        .iter()
        .find(|symbol| symbol["name"] == "original")
        .and_then(|symbol| symbol["node_id"].as_str())
        .unwrap()
        .to_owned();

    let stale_revision = intelligence_web_change_impact(
        State(state.clone()),
        ui_headers(&state, id),
        Query(IntelligenceChangeImpactQuery {
            path: "main.rs".into(),
            layer: ChangeLayer::Working,
            expected_snapshot: snapshot.clone(),
            expected_code_revision: "sha256:stale".into(),
            expected_design_revision: expected_design_revision.clone(),
            node_id: node_id.clone(),
        }),
    )
    .await;
    assert_eq!(stale_revision.status(), StatusCode::CONFLICT);
    let stale_revision = response_json(stale_revision).await;
    assert_eq!(stale_revision["code"], "stale_change");
    assert!(stale_revision.get("impact").is_none());

    let response = intelligence_web_change_impact(
        State(state.clone()),
        ui_headers(&state, id),
        Query(IntelligenceChangeImpactQuery {
            path: "main.rs".into(),
            layer: ChangeLayer::Working,
            expected_snapshot: snapshot.clone(),
            expected_code_revision: expected_code_revision.clone(),
            expected_design_revision: expected_design_revision.clone(),
            node_id: node_id.clone(),
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let body = response_json(response).await;
    let impact = &body["impact"];
    assert_eq!(body["workspace"], id);
    assert_eq!(body["repository_revision"], repository_revision);
    assert_eq!(impact["snapshot_id"], snapshot);
    assert_eq!(impact["path"], "main.rs");
    assert_eq!(impact["node_id"], node_id);
    assert_eq!(impact["source_sha256"], source_sha256);
    assert!(matches!(
        impact["precision"].as_str(),
        Some("syntax") | Some("semantic")
    ));
    assert!(impact["incoming_calls"].is_array());
    assert!(impact["references"].is_array());
    assert!(impact["implementations"].is_array());
    assert!(impact["search_matches"].is_array());
    if impact["precision"] == "syntax" {
        assert_eq!(impact["degraded"], true);
        assert_eq!(impact["degraded_from"], "lsp");
        assert_eq!(impact["provider"], "tree-sitter+search");
        let caller = impact["incoming_calls"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["path"] == "caller.rs")
            .expect("syntax caller should remain navigable");
        assert!(caller["node_id"]
            .as_str()
            .is_some_and(|node_id| node_id.starts_with("symbol:ts:")));
        assert!(impact["search_matches"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| {
                item["path"] == "caller.rs"
                    && item["line"] == 1
                    && item["text"]
                        .as_str()
                        .is_some_and(|text| text.contains("original"))
            }));
    }

    fs::write(root.path().join("main.rs"), "fn original() { newer(); }\n").unwrap();
    let stale = intelligence_web_change_impact(
        State(state.clone()),
        ui_headers(&state, id),
        Query(IntelligenceChangeImpactQuery {
            path: "main.rs".into(),
            layer: ChangeLayer::Working,
            expected_snapshot: detail["change"]["snapshot_id"].as_str().unwrap().to_owned(),
            expected_code_revision,
            expected_design_revision,
            node_id: impact["node_id"].as_str().unwrap().to_owned(),
        }),
    )
    .await;
    assert_eq!(stale.status(), StatusCode::CONFLICT);
    let stale = response_json(stale).await;
    assert_eq!(stale["code"], "stale_change");
    assert!(stale.get("impact").is_none());
}

#[test]
fn change_relation_normalization_marks_capped_or_invalid_locations_partial() {
    let source_sha256 = "a".repeat(64);
    let expected_revision = format!("sha256:{source_sha256}");
    let incoming_calls = (1..=25)
        .map(|line| {
            json!({
                "path": format!("caller-{line}.rs"),
                "line": line,
                "character": 1,
                "name": format!("caller_{line}"),
            })
        })
        .collect::<Vec<_>>();
    let capped = crate::mcp::web::web_change_impact::normalize_change_relation_impact(
        "main.rs",
        "snapshot",
        "symbol:ts:selected",
        &source_sha256,
        1,
        json!({
            "path": "main.rs", "line": 1, "character": 1, "selector": {"revision": expected_revision, "name": "selected", "line": 1, "character": 1},
            "degraded": false,
            "provider": "lsp:test",
            "precision": "semantic",
            "routing": "cross_file_semantic",
            "incoming_calls": incoming_calls,
            "references": [],
            "implementations": [],
            "truncated": false,
        }),
    )
    .unwrap();
    assert_eq!(capped["incoming_calls"].as_array().unwrap().len(), 24);
    assert_eq!(capped["partial"], true);
    assert_eq!(capped["routing"], "lsp");
    let invalid = crate::mcp::web::web_change_impact::normalize_change_relation_impact(
        "main.rs",
        "snapshot",
        "symbol:ts:selected",
        &source_sha256,
        1,
        json!({
            "path": "main.rs", "line": 1, "character": 1, "selector": {"revision": format!("sha256:{source_sha256}"), "name": "selected", "line": 1, "character": 1},
            "degraded": false,
            "provider": "lsp:test",
            "precision": "semantic",
            "routing": "cross_file_semantic",
            "incoming_calls": [
                {"path":"caller.rs","line":2,"character":1,"name":"caller"},
                {"path":"zero-line.rs","line":0,"character":1,"name":"bad_line"},
                {"path":"zero-character.rs","line":3,"character":0,"name":"bad_character"}
            ],
            "references": [],
            "implementations": [],
            "truncated": false,
        }),
    )
    .unwrap();
    assert_eq!(invalid["incoming_calls"].as_array().unwrap().len(), 1);
    assert_eq!(invalid["partial"], true);
    let invalid_search = crate::mcp::web::web_change_impact::normalize_change_relation_impact(
        "main.rs",
        "snapshot",
        "symbol:ts:selected",
        &source_sha256,
        1,
        json!({
            "path": "main.rs", "selector": {"revision": format!("sha256:{source_sha256}"), "name": "selected", "line": 1},
            "degraded": true,
            "provider": "tree-sitter+search",
            "precision": "syntax",
            "routing": "degraded_syntax_and_keyword_search",
            "degraded_from": "lsp",
            "syntax_calls": {"incoming_calls": [], "truncated": false},
            "keyword_matches": [
                {"path":"caller.rs","line":2,"sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb","text":"selected()"},
                {"path":"","line":3,"sha256":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","text":"selected()"},
                {"path":"zero.rs","line":0,"sha256":"dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd","text":"selected()"},
                {"path":"bad-sha.rs","line":4,"sha256":"not-a-sha","text":"selected()"}
            ],
            "truncated": false,
        }),
    )
    .unwrap();
    assert_eq!(
        invalid_search["search_matches"].as_array().unwrap().len(),
        1
    );
    assert_eq!(invalid_search["partial"], true);
    assert_eq!(invalid_search["routing"], "syntax-degraded");
    let malformed_syntax_identity =
        crate::mcp::web::web_change_impact::normalize_change_relation_impact(
            "main.rs",
            "snapshot",
            "symbol:ts:selected",
            &source_sha256,
            1,
            json!({
                "path": "main.rs", "selector": {"revision": format!("sha256:{source_sha256}"), "name": "selected", "line": 1},
                "degraded": true,
                "provider": "tree-sitter+search",
                "precision": "syntax",
                "routing": "degraded_syntax_and_keyword_search",
                "degraded_from": "lsp",
                "syntax_calls": {
                    "incoming_calls": [
                        {"path":"caller.rs","line":2,"character":1,"name":"caller","node_id":"not-a-syntax-node"}
                    ],
                    "truncated": false
                },
                "keyword_matches": [],
                "truncated": false,
            }),
        )
        .unwrap();
    assert_eq!(
        malformed_syntax_identity["incoming_calls"][0]["node_id"],
        Value::Null
    );
    assert_eq!(malformed_syntax_identity["partial"], true);
    let unavailable_syntax_calls =
        crate::mcp::web::web_change_impact::normalize_change_relation_impact(
            "main.rs",
            "snapshot",
            "symbol:ts:selected",
            &source_sha256,
            1,
            json!({
                "path": "main.rs", "selector": {"revision": format!("sha256:{source_sha256}"), "name": "selected", "line": 1},
                "degraded": true,
                "provider": "tree-sitter+search",
                "precision": "syntax",
                "routing": "degraded_syntax_and_keyword_search",
                "degraded_from": "lsp",
                "syntax_calls": null,
                "keyword_matches": [],
                "truncated": false,
            }),
        )
        .unwrap();
    assert!(unavailable_syntax_calls["incoming_calls"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(unavailable_syntax_calls["partial"], true);
    let malformed_collections =
        crate::mcp::web::web_change_impact::normalize_change_relation_impact(
            "main.rs",
            "snapshot",
            "symbol:ts:selected",
            &source_sha256,
            1,
            json!({
                "path": "main.rs", "line": 1, "character": 1, "selector": {"revision": format!("sha256:{source_sha256}"), "name": "selected", "line": 1, "character": 1},
                "degraded": false,
                "provider": "lsp:test",
                "precision": "semantic",
                "routing": "cross_file_semantic",
                "incoming_calls": [],
                "truncated": false,
            }),
        )
        .unwrap();
    assert!(malformed_collections["incoming_calls"]
        .as_array()
        .unwrap()
        .is_empty());
    assert!(malformed_collections["references"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(malformed_collections["partial"], true);
    let mut semantic = json!({
        "path": "main.rs", "line": 1, "character": 1, "selector": {"revision": format!("sha256:{source_sha256}"), "name": "selected", "line": 1, "character": 1},
        "degraded": false, "provider": "lsp:test", "precision": "semantic", "routing": "cross_file_semantic",
        "incoming_calls": [], "references": [], "implementations": [],
        "queried": ["references", "implementations", "calls"], "truncated": false,
    });
    let normalize = |navigation| {
        crate::mcp::web::web_change_impact::normalize_change_relation_impact(
            "main.rs",
            "snapshot",
            "symbol:ts:selected",
            &source_sha256,
            1,
            navigation,
        )
        .unwrap()
    };
    assert_eq!(normalize(semantic.clone())["partial"], true);
    semantic["unsupported"] = json!([]);
    semantic["failures"] = json!([]);
    semantic["queried"] = json!(["references", "implementations"]);
    assert_eq!(normalize(semantic)["partial"], true);
}

#[test]
fn change_relation_normalization_rejects_precision_provenance_contradictions() {
    let source_sha256 = "a".repeat(64);
    let normalize = |navigation| {
        crate::mcp::web::web_change_impact::normalize_change_relation_impact_for_symbol(
            "main.rs",
            "snapshot",
            "symbol:ts:selected",
            &source_sha256,
            1,
            "selected",
            navigation,
        )
    };
    let revision = || format!("sha256:{source_sha256}");
    assert!(normalize(json!({
        "path": "main.rs", "line": 1, "character": 1, "selector": {"revision": revision(), "name": "selected", "line": 1, "character": 1},
        "degraded": false,
        "provider": "lsp:test",
        "precision": "semantic",
        "routing": "cross_file_semantic",
        "incoming_calls": [{"path":"caller.rs","line":2,"character":1,"node_id":"symbol:ts:caller"}],
    }))
    .is_err());
    assert!(normalize(json!({
        "path": "main.rs", "selector": {"revision": revision(), "name": "selected", "line": 1},
        "degraded": true,
        "provider": "tree-sitter+search",
        "precision": "syntax",
        "routing": "degraded_syntax_and_keyword_search",
        "degraded_from": "lsp",
        "syntax_calls": {"incoming_calls": [], "truncated": false},
        "keyword_matches": [],
        "references": [{"path":"ref.rs","line":3,"character":1}],
    }))
    .is_err());
    for (provider, selector_line, path, name, query_line) in [
        ("lsp:test", 1, "other.rs", "selected", 1),
        ("test-lsp", 1, "main.rs", "selected", 1),
        ("lsp:test", 2, "main.rs", "selected", 2),
        ("lsp:test", 1, "main.rs", "other", 1),
        ("lsp:test", 1, "main.rs", "selected", 2),
    ] {
        assert!(normalize(json!({
        "path": path, "line": query_line, "character": 1, "selector": {"revision": revision(), "name": name, "line": selector_line, "character": 1}, "degraded": false, "provider": provider,
        "precision": "semantic", "routing": "cross_file_semantic", "incoming_calls": [], "references": [], "implementations": []
    })).is_err());
    }
    assert!(normalize(json!({
        "path": "main.rs", "selector": {"revision": revision(), "name": "selected", "line": 1},
        "degraded": "false",
        "provider": "lsp:test",
        "precision": "semantic",
        "routing": "cross_file_semantic",
    }))
    .is_err());
}

#[test]
fn changed_symbol_mapping_marks_malformed_definition_partial() {
    let source_sha256 = "a".repeat(64);
    let impact = crate::mcp::web::changed_symbol_impact(
        "main.rs",
        "snapshot",
        &source_sha256,
        "worktree",
        &[(2, 2)],
        false,
        json!({
            "sha256": source_sha256,
            "provider": "tree-sitter",
            "precision": "syntax",
            "parse_errors": false,
            "truncated": false,
            "symbols": [
                {
                    "id": "ts:valid",
                    "name": "valid",
                    "qualified_name": "valid",
                    "kind": "function",
                    "is_definition": true,
                    "range": {"start_line": 2, "end_line": 2},
                    "signature": "fn valid()",
                    "signature_redacted": false
                },
                {
                    "id": "ts:malformed",
                    "name": "malformed",
                    "qualified_name": "malformed",
                    "kind": "function",
                    "is_definition": true
                }
            ]
        }),
    )
    .unwrap();
    assert_eq!(impact["after_symbols"].as_array().unwrap().len(), 1);
    assert_eq!(impact["after_symbols"][0]["name"], "valid");
    assert_eq!(impact["partial"], true);
}

#[tokio::test]
async fn change_symbol_impact_rejects_counterpart_only_after_definition() {
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
            "counterpart-only fixture",
        ])
        .status()
        .unwrap();
    assert!(status.success());
    fs::write(
        root.path().join("main.rs"),
        "fn same() {\n    let keep = 1;\n}\n",
    )
    .unwrap();

    let detail = intelligence_web_change_detail(
        State(state.clone()),
        ui_headers(&state, id),
        change_query("main.rs", ChangeLayer::Working, None),
    )
    .await;
    assert_eq!(detail.status(), StatusCode::OK);
    let detail = response_json(detail).await;
    let paired = &detail["symbol_impact"]["after_symbols"][0];
    assert_eq!(paired["counterpart_only"], true);
    let node_id = paired["node_id"].as_str().unwrap().to_owned();

    let response = intelligence_web_change_impact(
        State(state.clone()),
        ui_headers(&state, id),
        Query(IntelligenceChangeImpactQuery {
            path: "main.rs".into(),
            layer: ChangeLayer::Working,
            expected_snapshot: detail["change"]["snapshot_id"].as_str().unwrap().to_owned(),
            expected_code_revision: detail["repository_revision"]["code"]
                .as_str()
                .unwrap()
                .to_owned(),
            expected_design_revision: detail["repository_revision"]["design"]
                .as_str()
                .map(str::to_owned),
            node_id,
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = response_json(response).await;
    assert_eq!(body["code"], "impact_unavailable");
    assert_eq!(
        body["error"],
        "The requested symbol is not one of the directly changed definitions in this snapshot."
    );
    assert!(body.get("impact").is_none());
}

#[tokio::test]
async fn change_symbol_impact_never_rebases_staged_symbols_onto_newer_worktree_bytes() {
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

    let detail = intelligence_web_change_detail(
        State(state.clone()),
        ui_headers(&state, id),
        change_query("main.rs", ChangeLayer::Staged, None),
    )
    .await;
    assert_eq!(detail.status(), StatusCode::OK);
    let detail = response_json(detail).await;
    let node_id = detail["symbol_impact"]["after_symbols"][0]["node_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let expected_code_revision = detail["repository_revision"]["code"]
        .as_str()
        .unwrap()
        .to_owned();
    let expected_design_revision = detail["repository_revision"]["design"]
        .as_str()
        .map(str::to_owned);
    assert_eq!(detail["symbol_impact"]["source_state"], "index");

    let response = intelligence_web_change_impact(
        State(state.clone()),
        ui_headers(&state, id),
        Query(IntelligenceChangeImpactQuery {
            path: "main.rs".into(),
            layer: ChangeLayer::Staged,
            expected_snapshot: detail["change"]["snapshot_id"].as_str().unwrap().to_owned(),
            expected_code_revision,
            expected_design_revision,
            node_id,
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let body = response_json(response).await;
    assert_eq!(body["code"], "impact_unavailable");
    assert!(body.get("impact").is_none());
    assert!(!body.to_string().contains("worktree_symbol"));
}

#[tokio::test]
async fn change_detail_http_never_exposes_protected_source_or_error_details() {
    let (state, root) = change_state();
    let id = state.workspaces.default_id();
    fs::write(root.path().join(".env"), "private-file-contents").unwrap();
    for path in [".env", ".git/config", "../outside.txt", "."] {
        let response = intelligence_web_change_detail(
            State(state.clone()),
            ui_headers(&state, id),
            change_query(path, ChangeLayer::Working, None),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
        let body = response_json(response).await;
        assert!(body.get("change").is_none());
        assert!(!body.to_string().contains("private-file-contents"));
        assert!(!body
            .to_string()
            .contains(&root.path().display().to_string()));
    }
}
