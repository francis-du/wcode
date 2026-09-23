use super::*;

#[test]
fn capability_routing_does_not_treat_identifier_words_as_task_intent() {
    for query in [
        "inspect verify_packet",
        "inspect reviewChanges",
        "inspect src/verification.rs",
        "inspect SemanticRegistry::find_symbol",
        "查看 design_plan 和 verify_packet",
    ] {
        let pack = manifest(query, &[], None);
        assert_eq!(pack["profile"], "coding", "{query}");
        assert_eq!(
            pack["recommended_tools"],
            json!(crate::harness::default_coding_tools()),
            "{query}"
        );
    }
    for (query, expected) in [
        ("review verify_packet", "verification"),
        ("design the verify_packet API", "planning"),
        ("查看 verify_packet 的调用", "semantic_navigation"),
    ] {
        assert_eq!(manifest(query, &[], None)["profile"], expected, "{query}");
    }
}

#[test]
fn coding_manifest_keeps_mandatory_controls_and_defers_specialized_scopes() {
    let manifest = manifest("fix the parser bug", &[], None);
    assert_eq!(manifest["profile"], "coding");
    assert_eq!(manifest["disclosure"], "metadata_first");
    assert!(manifest["recommended_tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool == "agent_context"));
    assert!(manifest["recommended_tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool == "verify_project"));
    assert_eq!(
        manifest["recommended_tool_count"].as_u64().unwrap() as usize,
        manifest["recommended_tools"].as_array().unwrap().len()
    );
    assert!(manifest["recommended_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action["tool"] == "agent_context"
            && action["group"] == "context"
            && action["disclosure"] == "core"));
    for control in [
        "workspace_boundary",
        "authorization",
        "sha_preconditions",
        "verification_evidence",
    ] {
        assert!(manifest["mandatory_controls"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == control));
    }
    assert!(manifest["deferred_product_scopes"]
        .as_array()
        .unwrap()
        .iter()
        .any(|scope| scope == "semantics"));
}

#[test]
fn execution_phase_and_query_promote_only_relevant_specialized_tools() {
    let execution = serde_json::json!({
        "phase": "verifying",
        "checkpoint": {
            "reconciliation_plan_id": "RC-1",
            "verification_plan_id": "VP-1"
        }
    });
    let manifest = manifest(
        "review semantic callers before verification",
        &["semantics".to_owned()],
        Some(&execution),
    );
    assert_eq!(manifest["profile"], "verification");
    let tools = manifest["recommended_tools"].as_array().unwrap();
    for expected in [
        "execution_status",
        "execution_propose",
        "worklist_status",
        "reconciliation_execution_status",
        "verification_status",
        "semantic_navigation",
    ] {
        assert!(
            tools.iter().any(|tool| tool == expected),
            "missing recommended tool {expected}"
        );
    }
    assert!(
        tools.len() <= MAX_RECOMMENDED_TOOLS,
        "manifest must remain bounded"
    );
    assert!(manifest["recommended_actions"]
        .as_array()
        .unwrap()
        .iter()
        .any(|action| action["tool"] == "semantic_navigation"
            && action["group"] == "semantics"
            && action["disclosure"] == "on_demand"));
}

#[test]
fn pending_steering_promotes_authoritative_apply_and_replan_tools() {
    let execution = serde_json::json!({
        "phase": "blocked",
        "pending_directive": {
            "kind": "change_scope",
            "summary": "Expand scope"
        },
        "replan_required": true,
        "checkpoint": {
            "reconciliation_plan_id": "RC-old"
        }
    });
    let continuation = manifest("continue after scope change", &[], Some(&execution));
    let tools = continuation["recommended_tools"].as_array().unwrap();
    for expected in [
        "worklist_update",
        "reconciliation_plan",
        "reconciliation_status",
    ] {
        assert!(
            tools.iter().any(|tool| tool == expected),
            "missing {expected}"
        );
    }
    assert!(tools.len() <= MAX_RECOMMENDED_TOOLS);

    let steering = manifest("steer: change scope", &[], Some(&execution));
    assert!(steering["recommended_tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool == "execution_steer"));
    let handoff = manifest("handoff to a clean context", &[], Some(&execution));
    assert!(handoff["recommended_tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|tool| tool == "execution_handoff"));
}

#[test]
fn capability_manifest_matrix_keeps_task_selection_bounded_and_specific() {
    let cases = [
        (
            "fix the parser bug",
            vec![],
            vec!["apply_file_edits", "review_changes", "verify_project"],
            vec![
                "semantic_navigation",
                "design_status",
                "verification_history",
            ],
        ),
        (
            "show callers and implementations of Parser::parse",
            vec!["semantics".to_owned()],
            vec!["semantic_navigation", "find_symbol", "symbol_context"],
            vec!["reconciliation_plan", "verification_history"],
        ),
        (
            "review verification evidence before release",
            vec!["verification".to_owned()],
            vec![
                "verification_status",
                "evidence_status",
                "verification_history",
            ],
            vec!["semantic_navigation", "design_status"],
        ),
        (
            "inspect architecture requirement traceability",
            vec!["design".to_owned()],
            vec![
                "design_status",
                "traceability_status",
                "reconciliation_status",
            ],
            vec!["semantic_navigation", "verification_history"],
        ),
    ];

    for (query, scopes, expected, excluded) in cases {
        let manifest = manifest(query, &scopes, None);
        let tools = manifest["recommended_tools"].as_array().unwrap();
        assert!(tools.len() <= MAX_RECOMMENDED_TOOLS, "{query}: {tools:?}");
        for tool in expected {
            assert!(
                tools.iter().any(|value| value == tool),
                "{query}: missing {tool}"
            );
        }
        for tool in excluded {
            assert!(
                !tools.iter().any(|value| value == tool),
                "{query}: leaked {tool}"
            );
        }
        for tool in tools.iter().filter_map(Value::as_str) {
            assert_ne!(
                crate::harness::model_tool_group(tool),
                "other",
                "{query}: {tool}"
            );
        }
    }
}

#[test]
fn explicit_design_scope_promotes_planning_without_provider_specific_routing() {
    let manifest = manifest("inspect current state", &["design".to_owned()], None);
    assert_eq!(manifest["profile"], "planning");
    let encoded = serde_json::to_string(&manifest).unwrap();
    assert!(encoded.contains("design_status"));
    assert!(encoded.contains("traceability_status"));
    assert!(!encoded.to_ascii_lowercase().contains("claude"));
    assert!(!encoded.to_ascii_lowercase().contains("openai"));
    assert!(encoded.len() < 3_000);
}
fn pending_recovery_execution() -> Value {
    json!({
        "id": "EX-current",
        "phase": "blocked",
        "pending_directive": {"kind": "change_scope", "summary": "Apply the expanded scope"},
        "replan_required": true,
        "checkpoint": {
            "reconciliation_plan_id": "RC-current",
            "verification_plan_id": "VP-current"
        }
    })
}

#[test]
fn pending_recovery_survives_combined_specialist_manifest() {
    let execution = pending_recovery_execution();
    let manifest = manifest(
        "review architecture semantic callers",
        &[],
        Some(&execution),
    );
    let tools = manifest["recommended_tools"].as_array().unwrap();
    assert_eq!(tools.len(), MAX_RECOMMENDED_TOOLS);
    for name in [
        "worklist_update",
        "reconciliation_plan",
        "reconciliation_status",
    ] {
        assert!(tools.contains(&json!(name)), "missing {name}: {tools:?}");
    }
    assert_eq!(manifest["recommended_tool_count"], tools.len());
    assert_eq!(
        manifest["recommended_actions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|action| action["tool"].clone())
            .collect::<Vec<_>>(),
        *tools
    );
}

#[test]
fn finalized_context_keeps_pending_recovery_with_saturated_readiness() {
    let query = "review architecture semantic callers";
    let execution = pending_recovery_execution();
    let source = "pub fn entry() {}";
    for budget in [1_200, 1_600, 4_000] {
        let mut pack = json!({
            "query": query,
            "truncated": false,
            "project": {"write_enabled": true},
            "execution": execution,
            "capabilities": manifest(query, &[], Some(&execution)),
            "targets": [{"id":"ts:entry", "path":"src/lib.rs", "qualified_name":"entry",
                "start_line":1, "end_line":1}],
            "files": [{"path":"src/lib.rs", "sha256":"a".repeat(64), "readonly":false}],
            "hot_source": [{"id":"ts:entry", "path":"src/lib.rs", "qualified_name":"entry",
                "sha256":"a".repeat(64), "body":{"content":source, "start_line":1,
                "end_line":1, "redacted":false, "truncated":false}}],
            "repo_map": {"precision":"syntax", "items":[], "truncated":false},
            "semantic_provider_hints": [{"language":"rust", "action":"install_lsp"}],
            "tests": []
        });
        super::super::refresh_readiness_capabilities(&mut pack);
        super::super::finalize_agent_context(&mut pack, 20_000, budget).unwrap();
        let tools = pack["capabilities"]["recommended_tools"]
            .as_array()
            .unwrap();
        assert!(tools.len() <= MAX_RECOMMENDED_TOOLS);
        for name in [
            "worklist_update",
            "reconciliation_plan",
            "reconciliation_status",
            "semantic_navigation",
            "apply_edits",
            "review_changes",
            "verify_project",
        ] {
            assert!(
                tools.contains(&json!(name)),
                "{budget}: missing {name}: {tools:?}"
            );
        }
        assert_eq!(pack["capabilities"]["recommended_tool_count"], tools.len());
        assert_eq!(
            pack["execution"]["pending_directive"],
            execution["pending_directive"]
        );
        assert_eq!(pack["hot_source"][0]["body"]["content"], source);
        assert_eq!(pack["files"][0]["sha256"], pack["hot_source"][0]["sha256"]);
        assert!(serde_json::to_vec(&pack).unwrap().len().div_ceil(4) <= budget);
    }
}
