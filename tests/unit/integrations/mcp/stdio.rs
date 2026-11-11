use super::*;
use crate::mcp::{request_protocol, validate_modern_request};

#[test]
fn protocol_selection_supports_modern_stdio_and_legacy_sessions() {
    let modern = json!({
        "jsonrpc":"2.0",
        "id":1,
        "method":"tools/list",
        "params":{"_meta":{
            "io.modelcontextprotocol/protocolVersion": MODERN_PROTOCOL_VERSION,
            "io.modelcontextprotocol/clientCapabilities": {}
        }}
    });
    assert_eq!(
        protocol_for_payload(&modern, DEFAULT_LEGACY_PROTOCOL),
        MODERN_PROTOCOL_VERSION
    );
    let discover = json!({"jsonrpc":"2.0","id":2,"method":"server/discover"});
    assert_eq!(
        protocol_for_payload(&discover, DEFAULT_LEGACY_PROTOCOL),
        MODERN_PROTOCOL_VERSION
    );
    assert_eq!(
        validate_modern_payload(&discover),
        Err("missing 2026 request _meta envelope")
    );
    let legacy = json!({"jsonrpc":"2.0","id":3,"method":"tools/list","params":{}});
    assert_eq!(protocol_for_payload(&legacy, "2025-06-18"), "2025-06-18");

    let unsupported_modern = json!({
        "jsonrpc":"2.0","id":7,"method":"tools/list",
        "params":{"_meta":{"io.modelcontextprotocol/protocolVersion":"2099-01-01"}}
    });
    assert_eq!(
        protocol_for_payload(&unsupported_modern, DEFAULT_LEGACY_PROTOCOL),
        "2099-01-01"
    );

    let http_discover = json!({"jsonrpc":"2.0","id":8,"method":"server/discover"});
    assert_eq!(
        request_protocol(&axum::http::HeaderMap::new(), &http_discover),
        MODERN_PROTOCOL_VERSION
    );
    let modern_payload = json!({
        "jsonrpc":"2.0","id":9,"method":"tools/list",
        "params":{"_meta":{
            "io.modelcontextprotocol/protocolVersion": MODERN_PROTOCOL_VERSION,
            "io.modelcontextprotocol/clientCapabilities": {}
        }}
    });
    let mut conflicting_headers = axum::http::HeaderMap::new();
    conflicting_headers.insert("mcp-protocol-version", "2025-11-25".parse().unwrap());
    conflicting_headers.insert("mcp-method", "tools/list".parse().unwrap());
    assert_eq!(
        request_protocol(&conflicting_headers, &modern_payload),
        MODERN_PROTOCOL_VERSION
    );
    assert_eq!(
        validate_modern_request(&conflicting_headers, &modern_payload),
        Err("MCP-Protocol-Version header does not match request _meta protocolVersion")
    );

    let capable = json!({
        "jsonrpc":"2.0",
        "id":4,
        "method":"initialize",
        "params":{"protocolVersion":"2025-11-25","capabilities":{"elicitation":{}}}
    });
    assert!(legacy_client_supports_elicitation(&capable));
    let incapable = json!({
        "jsonrpc":"2.0",
        "id":5,
        "method":"initialize",
        "params":{"protocolVersion":"2025-11-25","capabilities":{}}
    });
    assert!(!legacy_client_supports_elicitation(&incapable));
    let url_only = json!({
        "jsonrpc":"2.0",
        "id":6,
        "method":"initialize",
        "params":{"protocolVersion":"2025-11-25","capabilities":{"elicitation":{"url":{}}}}
    });
    assert!(!legacy_client_supports_elicitation(&url_only));
}

fn human_decision_state() -> (Arc<AppState>, tempfile::TempDir) {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("lib.rs"), "pub fn baseline() {}\n").unwrap();
    let workspaces = Workspaces::new_with_security(
        [root.path()],
        true,
        true,
        crate::workspace::WorkspaceSecurity {
            allow_risky_exec: true,
            allow_unrestricted_commands: true,
            ..Default::default()
        },
    )
    .unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = Arc::new(AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1".into())),
        workspaces,
        harness: ToolHarness::new(2).unwrap(),
        monitor: TaskMonitor::new([workspace_id.clone()]),
        tasks: TaskRuntime::default(),
    });
    let (_, workspace) = state.workspaces.select(None).unwrap();
    let revision = state.harness.current_revision(&workspace).unwrap();
    let mut verification = crate::verification::VerificationState::default();
    let plan = verification
        .create_plan(
            "VP-human-bound".into(),
            workspace_id.clone(),
            "change:human-bound".into(),
            crate::verification::VerificationPlanBinding {
                revision,
                required_checks: None,
                stage_targets: vec![],
                automation_gaps: vec![],
            },
            crate::risk::RiskLevel::Critical,
            (0..32).map(|id| format!("VJ-human-{id}")),
        )
        .unwrap();
    assert!(plan.require_human_approval);
    crate::verification_store::persist(&workspace, &verification).unwrap();
    let reconciliation = crate::reconcile::ReconciliationPlan {
        id: "RP-human-bound".into(),
        workspace: workspace_id,
        risk_level: crate::risk::RiskLevel::Critical,
        design_changes: vec![],
        drift_ids: vec![],
        impacted_components: vec![],
        impacted_symbols: vec![],
        impacted_tests: vec![],
        impacted_acceptance: vec![],
        implementation_tasks: vec![],
        change_intents: vec![],
        worklist_item_ids: Vec::new(),
        verification_plan: plan,
    };
    crate::reconciliation_store::persist(&workspace, &reconciliation).unwrap();
    (state, root)
}

async fn human_decision_call(
    state: &Arc<AppState>,
    tool: &str,
    plan: &str,
    statement: &str,
) -> Value {
    crate::mcp::call_tool_owned(
        state,
        json!({"name":tool,"arguments":{
            "plan_id":plan,"approver":"client-forged-admin","statement":statement,"confirmed":true,
        }}),
        &"a".repeat(64),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn human_decisions_are_permissive_by_default_and_ignore_client_approver_authority() {
    for (tool, plan) in [
        ("verification_approve", "VP-human-bound"),
        ("reconciliation_approve", "RP-human-bound"),
    ] {
        let (state, _root) = human_decision_state();
        state
            .workspaces
            .set_all_commands_authorized(None, true)
            .unwrap();
        let approved = human_decision_call(&state, tool, plan, "I reviewed this exact plan.").await;
        assert_eq!(approved["isError"], false, "{approved}");
        let receipt = &approved["structuredContent"]["operator_authorization"];
        // The permissive default is a policy authorization, never an operator
        // decision, even though the MCP client supplied a display name.
        assert_eq!(receipt["kind"], "policy_auto_approved");
        assert_eq!(receipt["status"], "approved_once");
        assert!(receipt["id"].as_str().unwrap().starts_with("AUTH-"));
        assert!(state.workspaces.latest_pending_authorization().is_none());
        assert_eq!(
            approved["structuredContent"]["approval_source"],
            "policy_auto_approved"
        );
        if tool == "verification_approve" {
            assert_eq!(
                approved["structuredContent"]["authority"],
                "policy_auto_approved"
            );
        }
        assert_eq!(
            approved["structuredContent"]["display_approver"],
            "client-forged-admin"
        );
        let (_, workspace) = state.workspaces.select(None).unwrap();
        let evidence = serde_json::to_value(
            state
                .harness
                .evidence_status(state.workspaces.default_id(), &workspace, None, 16)
                .unwrap(),
        )
        .unwrap();
        let serialized = evidence.to_string();
        assert!(
            serialized.contains("policy:policy_auto_approved:"),
            "{serialized}"
        );
        assert!(
            serialized.contains("\"authority\":\"policy_auto_approved\""),
            "{serialized}"
        );
        assert!(
            !serialized.contains("human:client-forged-admin"),
            "{serialized}"
        );
        assert!(
            !serialized.contains("\"kind\":\"human_approval\""),
            "a policy default must not be recorded as a human approval: {serialized}"
        );
        assert!(
            serialized.contains(receipt["fingerprint"].as_str().unwrap()),
            "{serialized}"
        );
    }
}

#[tokio::test]
async fn human_decision_revision_drift_still_requires_replanning() {
    let (state, root) = human_decision_state();
    let initial = human_decision_call(
        &state,
        "verification_approve",
        "VP-human-bound",
        "Reviewed.",
    )
    .await;
    assert_eq!(initial["isError"], false, "{initial}");
    assert!(state.workspaces.latest_pending_authorization().is_none());
    std::fs::write(root.path().join("lib.rs"), "pub fn changed() {}\n").unwrap();
    let stale = human_decision_call(
        &state,
        "verification_approve",
        "VP-human-bound",
        "Reviewed.",
    )
    .await;
    assert_eq!(stale["isError"], true, "{stale}");
    assert!(
        stale["structuredContent"]["error"]
            .as_str()
            .unwrap()
            .contains("replan_required"),
        "{stale}"
    );
    let (_, workspace) = state.workspaces.select(None).unwrap();
    // The permissive default records a policy authorization, so the plan still
    // lacks a real human approval after the stale-revision rejection.
    assert!(
        !state
            .harness
            .verification_status(
                state.workspaces.default_id(),
                &workspace,
                "VP-human-bound",
                None
            )
            .unwrap()
            .human_approval
    );
    assert!(state
        .harness
        .verification_status(
            state.workspaces.default_id(),
            &workspace,
            "VP-human-bound",
            None
        )
        .unwrap()
        .blockers
        .contains(&"human-approval-required".to_owned()));
}
