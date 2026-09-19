use super::*;

#[test]
fn context_packing_source_backed_metadata_is_revision_bound() {
    let base = json!({
        "provenance_defaults":{"targets":{"provider":"tree-sitter","precision":"syntax"}},
        "targets":[{"id":"ts:a","path":"src/a.rs","qualified_name":"target","kind":"function","language":"rust",
            "start_line":1,"end_line":1,"provider":"tree-sitter","precision":"syntax"}],
        "files":[{"path":"src/a.rs","sha256":"a".repeat(64),"readonly":false}],
        "hot_source":[{"id":"ts:a","path":"src/a.rs","qualified_name":"target","sha256":"a".repeat(64),
            "body":{"content":"fn target() {}","start_line":1,"end_line":1,"truncated":false,"redacted":false}}]
    });
    for (pointer, replacement) in [
        ("/hot_source/0/id", json!("ts:wrong")),
        ("/hot_source/0/path", json!("src/wrong.rs")),
        ("/hot_source/0/sha256", json!("b".repeat(64))),
        ("/hot_source/0/body/redacted", json!(true)),
        ("/hot_source/0/body/truncated", json!(true)),
        ("/hot_source/0/body/start_line", json!(2)),
        ("/hot_source/0/body/content", json!("")),
    ] {
        let mut pack = base.clone();
        *pack.pointer_mut(pointer).unwrap() = replacement;
        assert!(
            !context_budget::compact_source_backed_targets(&mut pack),
            "{pointer}"
        );
        assert_eq!(pack["targets"], base["targets"], "{pointer}");
    }
    let mut valid = base.clone();
    assert!(context_budget::compact_source_backed_targets(&mut valid));
    assert_eq!(valid["targets"][0]["id"], "ts:a");
    assert_eq!(
        valid["hot_source"][0]["body"],
        base["hot_source"][0]["body"]
    );
    assert_eq!(valid["files"], base["files"]);
    assert_eq!(
        valid["provenance_defaults"]["targets"]["provider"],
        "tree-sitter"
    );
    assert!(!context_budget::compact_source_backed_targets(&mut valid));
    let mut special = base;
    special["targets"][0]["provider"] = json!("language-server");
    special["targets"][0]["precision"] = json!("semantic");
    context_budget::compact_source_backed_targets(&mut special);
    assert_eq!(special["targets"][0]["provider"], "language-server");
    assert_eq!(special["targets"][0]["precision"], "semantic");
}

#[test]
fn context_packing_evicted_body_restores_followup_coordinates() {
    let mut pack = json!({"truncated":false,"query":"a_fn b_fn",
    "targets":[{"id":"ts:a","path":"src/a.rs","qualified_name":"a_fn"},{"id":"ts:b","path":"src/b.rs","qualified_name":"b_fn"}],
    "files":[{"path":"src/a.rs","sha256":"a".repeat(64),"readonly":false},{"path":"src/b.rs","sha256":"b".repeat(64),"readonly":false}],
    "hot_source":[
        {"id":"ts:a","path":"src/a.rs","sha256":"a".repeat(64),"body":{"content":"fn a() {}","start_line":1,"end_line":1,"redacted":false,"truncated":false}},
        {"id":"ts:b","path":"src/b.rs","sha256":"b".repeat(64),"body":{"content":"x".repeat(4000),"start_line":20,"end_line":20,"redacted":false,"truncated":false}}
    ]});
    context_budget::trim_agent_context(&mut pack, 400).unwrap();
    assert!(serialized_json_bytes(&pack).unwrap().div_ceil(4) <= 400);
    assert_eq!(pack["hot_source"].as_array().unwrap().len(), 1);
    assert_eq!(pack["targets"][1]["start_line"], 20);
    assert_eq!(pack["targets"][1]["end_line"], 20);
}

#[test]
fn context_packing_duplicate_signatures_require_matching_identity_and_revision() {
    for mode in ["matching", "wrong-id", "stale"] {
        let mut pack = json!({
            "targets":[{"id":"ts:target","path":"src/a.rs","qualified_name":"overload", "signature":"fn overload(x: usize)"}],
            "files":[{"path":"src/a.rs","sha256":"a".repeat(64),"readonly":false}],
            "hot_source":[{"id":if mode == "wrong-id" {"ts:other"} else {"ts:target"},
                "path":"src/a.rs","qualified_name":"overload",
                "sha256":if mode == "stale" {"b".repeat(64)} else {"a".repeat(64)},
                "body":{"content":"fn overload(x: usize) {}","redacted":false,"truncated":false}}]
        });
        context_budget::compact_duplicate_symbol_metadata(&mut pack);
        assert_eq!(
            pack["targets"][0].get("signature").is_none(),
            mode == "matching",
            "{mode}"
        );
        assert_eq!(pack["targets"][0]["id"], "ts:target");
        assert_eq!(pack["files"][0]["readonly"], false);
    }
}

#[test]
fn context_packing_compacts_body_metadata_before_losing_sha() {
    let sources: Vec<_> = ["decode_frame", "serve_packet"]
        .iter()
        .map(|name| {
            json!({"id":format!("ts:{name}"), "path":format!("src/{name}.rs"),
            "qualified_name":name, "sha256":"a".repeat(64),
            "signature":"redundant signature ".repeat(250),
            "body":{"content":format!("fn {name}() {{}}"), "start_line":1,
                "end_line":1, "redacted":false, "truncated":false}})
        })
        .collect();
    let files: Vec<_> = sources
        .iter()
        .map(|source| json!({"path":source["path"], "sha256":source["sha256"], "readonly":false}))
        .collect();
    let mut pack = json!({"hot_source":sources, "files":files, "truncated":false});
    context_budget::trim_agent_context(&mut pack, 1_000).unwrap();
    assert!(serialized_json_bytes(&pack).unwrap().div_ceil(4) <= 1_000);
    assert_eq!(pack["hot_source"].as_array().unwrap().len(), 2);
    assert_eq!(pack["files"].as_array().unwrap().len(), 2);
    for source in pack["hot_source"].as_array().unwrap() {
        assert_eq!(source["body"]["truncated"], false);
        assert_eq!(source["sha256"], "a".repeat(64));
    }
}

#[test]
fn tight_context_drops_advisory_decision_before_original_risk_evidence() {
    let risk_summary = "risk-evidence-".repeat(64);
    let mut pack = json!({
        "truncated": false,
        "decision_plane": {"advisory": "derived-signal-".repeat(192)},
        "risks": [{
            "level": "high",
            "category": "runtime",
            "summary": risk_summary,
        }],
    });

    context_budget::trim_agent_context(&mut pack, 350).unwrap();

    assert!(pack.get("decision_plane").is_none());
    assert_eq!(pack["risks"].as_array().unwrap().len(), 1);
    assert_eq!(pack["risks"][0]["summary"], risk_summary);
    assert!(serialized_json_bytes(&pack).unwrap().div_ceil(4) <= 350);
}

#[test]
fn tight_context_compacts_provider_prose_before_source() {
    let original = "pub fn target() -> bool {\n    true\n}";
    let mut pack = json!({
        "hot_source": [{"id":"ts:target", "path":"src/target.rs", "qualified_name":"target", "sha256":"a".repeat(64),
            "body":{"content":original,"start_line":1,"end_line":3,"redacted":false,"truncated":false}}],
        "semantic_provider_hints": [{"language":"rust", "provider":"rust-analyzer", "action":"authorize_lsp",
            "discovery":"available", "precision":"syntax", "reason":"Explanatory provider details. ".repeat(200)}]
    });
    context_budget::trim_agent_context(&mut pack, 1_000).unwrap();
    assert!(serialized_json_bytes(&pack).unwrap().div_ceil(4) <= 1_000);
    assert_eq!(pack["hot_source"][0]["body"]["content"], original);
    assert_eq!(pack["hot_source"][0]["body"]["redacted"], false);
    assert_eq!(pack["hot_source"][0]["sha256"], "a".repeat(64));
    let hint = &pack["semantic_provider_hints"][0];
    assert_eq!(hint["language"], "rust");
    assert_eq!(hint["provider"], "rust-analyzer");
    assert_eq!(hint["action"], "authorize_lsp");
    assert_eq!(hint["discovery"], "available");
    assert_eq!(hint["precision"], "syntax");
    assert!(hint.get("reason").is_none());
    let once = pack.clone();
    context_budget::trim_agent_context(&mut pack, 1_000).unwrap();
    assert_eq!(pack, once);
}

#[test]
fn tight_context_keeps_large_relation_query_within_budget() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname='fitness-fixture'\nversion='0.1.0'\nedition='2021'\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("src/lib.rs"),
        "mod session;\npub use session::*;\n",
    )
    .unwrap();
    std::fs::write(root.path().join("src/session.rs"), "pub fn cleanup_if_owner(old: u64, current: u64) -> bool {\n    old == current\n}\npub fn refresh_session(old: u64, current: u64) -> bool {\n    cleanup_if_owner(old, current)\n}\npub fn observe_epoch(epoch: u64) -> bool {\n    refresh_session(epoch, epoch)\n}\n").unwrap();
    for n in 0..640 {
        std::fs::write(
            root.path().join(format!("src/noise_{n:04}.rs")),
            format!("pub fn unrelated_{n}() {{}}\n"),
        )
        .unwrap();
    }
    let workspace = Workspace::new(root.path(), true, false).unwrap();
    let harness = ToolHarness::new(4).unwrap();
    for _ in 0..2 {
        let pack = harness
            .agent_context(
                "fitness",
                &workspace,
                "Find callers of cleanup_if_owner and refresh_session",
                1_000,
                &[],
            )
            .unwrap();
        assert!(serialized_json_bytes(&pack).unwrap().div_ceil(4) <= 1_000);
        for name in ["cleanup_if_owner", "refresh_session"] {
            assert!(pack["targets"]
                .as_array()
                .unwrap()
                .iter()
                .any(|target| target["qualified_name"] == name));
        }
        assert!(pack.get("query").is_none());
        assert_eq!(pack["repo_map"]["truncated"], true);
    }
}

#[test]
fn diagnostic_context_compaction_preserves_edit_identity_and_redaction() {
    let mut pack = json!({"hot_source": [{
        "id": "ts:target", "path": "src/target.rs", "qualified_name": "target",
        "sha256": "a".repeat(64), "provider": "workspace", "precision": "deterministic",
        "signature": "explanatory metadata ".repeat(100),
        "body": {"start_line": 7, "end_line": 9, "content": "line7\nline8\nline9", "redacted": true, "truncated": true}
    }]});
    context_budget::trim_agent_context(&mut pack, 180).unwrap();
    let source = &pack["hot_source"][0];
    assert_eq!(source["path"], "src/target.rs");
    assert_eq!(source["id"], "ts:target");
    assert_eq!(source["precision"], "deterministic");
    assert_eq!(source["body"]["redacted"], true);
    assert_eq!(source["body"]["end_line"], 9);
    assert_eq!(source["sha256"], "a".repeat(64));
    assert!(serde_json::to_vec(&pack).unwrap().len().div_ceil(4) <= 180);
}

#[test]
fn diagnostic_context_readiness_rejects_unusable_target_bodies() {
    let base = json!({
        "project": {"write_enabled": true},
        "targets": [{"id": "ts:target", "path": "src/target.rs", "qualified_name": "target"}],
        "files": [{"path": "src/target.rs", "sha256": "a".repeat(64), "readonly": false}],
        "hot_source": [{"id": "ts:target", "path": "src/target.rs", "qualified_name": "target", "sha256": "a".repeat(64),
            "body": {"start_line": 1, "end_line": 1, "content": "fn target() {}", "redacted": false, "truncated": false}}]
    });
    let mut valid = base.clone();
    update_agent_readiness(&mut valid);
    assert_eq!(valid["readiness"]["edit"], "ready");
    for (pointer, replacement) in [
        ("/hot_source/0/sha256", json!("b".repeat(64))),
        ("/hot_source/0/path", json!("src/other.rs")),
        ("/hot_source/0/id", json!("ts:other")),
        ("/hot_source/0/body/content", json!("")),
        ("/hot_source/0/body/redacted", json!(true)),
    ] {
        let mut pack = base.clone();
        *pack.pointer_mut(pointer).unwrap() = replacement;
        update_agent_readiness(&mut pack);
        assert_eq!(pack["readiness"]["edit"], "needs_source", "{pointer}");
        assert!(pack["readiness"]["next_actions"]
            .as_array()
            .unwrap()
            .contains(&json!("symbol_context")));
    }
}

#[test]
fn streaming_json_size_matches_real_serialization() {
    let value = json!({
        "text": "unicode-你好-🚀",
        "nested": [{"a": 1, "b": true}, null, [1, 2, 3]],
        "escaped": "quote: \" slash: \\ newline: \n"
    });
    assert_eq!(
        context_budget::serialized_json_bytes(&value).unwrap(),
        serde_json::to_vec(&value).unwrap().len()
    );
    assert_eq!(
        context_budget::estimated_json_tokens(&value).unwrap(),
        serde_json::to_vec(&value).unwrap().len().div_ceil(4)
    );
}

#[test]
fn streaming_json_size_matches_real_serialization_across_200_shapes() {
    for round in 0..200usize {
        let value = json!({
            "round": round,
            "ascii": "x".repeat(round % 113),
            "unicode": format!("你好🚀-{}-{}", round, "λ".repeat(round % 19)),
            "flags": [round % 2 == 0, round % 3 == 0, round % 5 == 0],
            "nested": {"left": round.saturating_mul(17), "right": null},
        });
        let encoded = serde_json::to_vec(&value).unwrap();
        assert_eq!(
            context_budget::serialized_json_bytes(&value).unwrap(),
            encoded.len(),
            "round {round}"
        );
        assert_eq!(
            context_budget::estimated_json_tokens(&value).unwrap(),
            encoded.len().div_ceil(4),
            "round {round}"
        );
    }
}

#[test]
fn trimming_drops_secondary_hot_source_before_shrinking_primary() {
    let primary = "primary-body-".repeat(64);
    let secondary = "secondary-body-".repeat(64);
    let mut value = json!({
        "truncated": false,
        "hot_source": [
            {"body": {"content": primary, "truncated": false}},
            {"body": {"content": secondary, "truncated": false}}
        ]
    });
    let mut one_item = value.clone();
    one_item["hot_source"].as_array_mut().unwrap().pop();
    let budget = context_budget::estimated_json_tokens(&one_item).unwrap();

    context_budget::trim_agent_context(&mut value, budget).unwrap();

    let hot_source = value["hot_source"].as_array().unwrap();
    assert_eq!(hot_source.len(), 1);
    assert_eq!(
        hot_source[0]["body"]["content"].as_str().unwrap(),
        "primary-body-".repeat(64)
    );
    assert_eq!(value["truncated"], true);
}

#[test]
fn trimming_preserves_multiple_explicit_targets_before_hot_source() {
    let mut value = json!({
        "query": "feature_entry batch_worker",
        "truncated": false,
        "targets": [
            {"qualified_name": "feature_entry", "path": "src/entry/mod.rs"},
            {"qualified_name": "batch_worker", "path": "src/worker/mod.rs"}
        ],
        "hot_source": [
            {"body": {"content": "primary-body-".repeat(80), "truncated": false}},
            {"body": {"content": "secondary-body-".repeat(80), "truncated": false}}
        ]
    });
    let mut one_target = value.clone();
    one_target["targets"].as_array_mut().unwrap().pop();
    let budget = context_budget::estimated_json_tokens(&one_target).unwrap();

    context_budget::trim_agent_context(&mut value, budget).unwrap();

    let targets = value["targets"].as_array().unwrap();
    assert_eq!(
        targets.len(),
        2,
        "explicit edit targets must survive trimming"
    );
    assert_eq!(value["truncated"], true);
    assert!(
        value["hot_source"].as_array().unwrap().len() < 2
            || value["hot_source"][0]["body"]["content"]
                .as_str()
                .is_some_and(|body| body.len() < "primary-body-".repeat(80).len()),
        "hot source should yield before an explicit target"
    );
}

#[test]
fn tight_context_budget_preserves_pending_execution_steering() {
    let directive = json!({
        "kind": "change_scope",
        "summary": "Expand the recovery work into the scheduler",
        "requested_by": "user:test",
        "scopes": ["runtime", "workspace"],
        "worklist_revision": 7,
        "repository_revision": {"code": "abc123", "design": "def456"},
        "reconciliation_plan_id": "RP-old",
        "requires_replan": true,
        "requested_at_ms": 1234
    });
    let lineage = json!({
        "parent_execution_id": "EX-parent",
        "handoff_count": 1,
        "requested_by": "user:test",
        "summary": "Continue cleanly",
        "handed_off_at_ms": 1200
    });
    let mut value = json!({
        "truncated": false,
        "execution": {
            "id": "EX-current",
            "revision": 9,
            "objective": "verbose-objective-".repeat(300),
            "phase": "blocked",
            "checkpoint": {
                "worklist_revision": 7,
                "repository_revision": {"code": "abc123", "design": "def456"},
                "reconciliation_plan_id": "RP-old",
                "verification_plan_id": "VP-old",
                "verification_ready": false,
                "blockers": ["steering_replan_required"]
            },
            "pending_directive": directive,
            "replan_required": true,
            "lineage": lineage
        }
    });
    let before = context_budget::estimated_json_tokens(&value).unwrap();
    context_budget::trim_agent_context(&mut value, 420).unwrap();

    assert!(before > 420);
    assert_eq!(value["truncated"], true);
    assert_eq!(value["execution"]["compacted"], true);
    assert_eq!(value["execution"]["pending_directive"], directive);
    assert_eq!(value["execution"]["replan_required"], true);
    assert_eq!(value["execution"]["lineage"], lineage);
    assert_eq!(
        value["execution"]["checkpoint"]["reconciliation_plan_id"],
        "RP-old"
    );
}

#[test]
fn task_aware_retrieval_specializes_only_when_one_intent_is_clear() {
    use crate::harness::harness_retrieval::{classify_repo_map_intent, RepoMapIntent};

    let trace = classify_repo_map_intent("find the implementation for REQ-AUTH-001");
    assert_eq!(trace.intent, RepoMapIntent::TraceToCode);
    assert!(trace.specialized);

    let tests = classify_repo_map_intent("which tests verify token rotation");
    assert_eq!(tests.intent, RepoMapIntent::CodeToTest);
    assert!(tests.specialized);

    let comment = classify_repo_map_intent(
        "review comment: should this stay consistent with the existing implementation elsewhere?",
    );
    assert_eq!(comment.intent, RepoMapIntent::CommentToContext);
    assert!(comment.specialized);

    let failure = classify_repo_map_intent(
        "test worker failed: thread 'worker' panicked at src/runtime/worker.rs:120 assertion failed",
    );
    assert_eq!(failure.intent, RepoMapIntent::FailureTraceToCode);
    assert!(failure.specialized);
    assert_eq!(failure.reason, "reproduced_failure_trace_signal");

    let ripple = classify_repo_map_intent("show callers and rename impact");
    assert_eq!(ripple.intent, RepoMapIntent::EditToRipple);
    assert!(ripple.specialized);

    let generic = classify_repo_map_intent("optimize token rotation");
    assert_eq!(generic.intent, RepoMapIntent::Context);
    assert!(!generic.specialized);
    assert_eq!(generic.reason, "no_specific_retrieval_signal");

    let ambiguous =
        classify_repo_map_intent("review comment: which tests verify the same behavior elsewhere?");
    assert_eq!(ambiguous.intent, RepoMapIntent::Context);
    assert!(!ambiguous.specialized);
    assert_eq!(ambiguous.reason, "ambiguous_retrieval_signals");
}

#[test]
fn readiness_precision_requires_relationship_coverage_for_every_direct_target() {
    let targets = ["direct:a".to_owned(), "direct:b".to_owned()]
        .into_iter()
        .collect::<std::collections::BTreeSet<_>>();
    let mut relations = std::collections::HashMap::<String, Vec<Value>>::from([
        (
            "related:a".to_owned(),
            vec![json!({"direct": "direct:a", "precision": "semantic"})],
        ),
        (
            "broad-seed".to_owned(),
            vec![json!({"direct": "direct:broad-seed", "precision": "runtime"})],
        ),
    ]);
    assert_eq!(
        crate::harness::harness_repo_map::repo_map_covered_precision(&targets, &relations),
        "syntax",
        "one semantic target and one uncovered target must stay syntax"
    );

    relations.insert(
        "related:b".to_owned(),
        vec![json!({"direct": "direct:b", "precision": "semantic"})],
    );
    let semantic =
        crate::harness::harness_repo_map::repo_map_covered_precision(&targets, &relations);
    assert_eq!(semantic, "semantic");
    let value = json!({"repo_map": {"precision": semantic}});
    assert_eq!(covered_repo_map_precision(&value), "semantic");

    relations.insert(
        "runtime:a".to_owned(),
        vec![json!({"direct": "direct:a", "precision": "runtime"})],
    );
    assert_eq!(
        crate::harness::harness_repo_map::repo_map_covered_precision(&targets, &relations),
        "semantic",
        "one runtime-covered target must not upgrade a second semantic-only target"
    );
}
