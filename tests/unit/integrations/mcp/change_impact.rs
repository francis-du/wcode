//! Change symbol impact and relation normalization: which changed symbol a
//! relation belongs to, and how capped or contradictory provenance is reported.
use super::change_view::{change_query, change_state};
use super::*;
use crate::workspace::ChangeLayer;
use axum::extract::Query;

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
