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

#[tokio::test]
async fn legacy_stdio_elicitation_approves_and_retries_the_original_tool_call() {
    let dir = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([dir.path()], false, true).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    workspaces
        .revoke_command(Some(&workspace_id), "cargo")
        .unwrap();
    let state = Arc::new(AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1".to_owned())),
        workspaces,
        harness: ToolHarness::new(2).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    });
    let owner = "a".repeat(64);
    let protocol = "2025-11-25";
    let original = json!({
        "jsonrpc":"2.0",
        "id":9,
        "method":"tools/call",
        "params":{
            "name":"run_command",
            "arguments":{"program":"cargo","args":["--version"],"cwd":".","timeout_seconds":30}
        }
    });
    let initial = dispatch_mcp_payload(state.clone(), original.clone(), protocol, &owner).await;
    let request = authorization_request_from_tool_result(
        &state,
        initial.as_ref().unwrap().get("result").unwrap(),
    )
    .unwrap();
    let request_state = authorization_request_state(&state, &request, &owner).unwrap();
    let answer = format!(
        "{}\n",
        json!({
            "jsonrpc":"2.0",
            "id":request_state,
            "result":{"action":"accept","content":{"approved":true}}
        })
    );
    let mut lines = BufReader::new(answer.as_bytes()).lines();
    let mut sink = tokio::io::sink();
    let completed = drive_legacy_authorization(
        state,
        &mut lines,
        &mut sink,
        LegacyAuthorizationTurn {
            original,
            protocol: protocol.to_owned(),
            owner,
            response: initial,
            elicitation_supported: true,
        },
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(completed["result"]["isError"], false);
    assert_eq!(completed["result"]["structuredContent"]["success"], true);
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
async fn human_decisions_require_operator_grants_and_ignore_client_approver_authority() {
    for (tool, plan) in [
        ("verification_approve", "VP-human-bound"),
        ("reconciliation_approve", "RP-human-bound"),
    ] {
        let (state, _root) = human_decision_state();
        state
            .workspaces
            .set_all_commands_authorized(None, true)
            .unwrap();
        let initial = human_decision_call(&state, tool, plan, "I reviewed this exact plan.").await;
        assert_eq!(initial["isError"], true, "{initial}");
        let request = authorization_request_from_tool_result(&state, &initial).unwrap();
        assert_eq!(
            request.kind,
            crate::authorization::AuthorizationKind::HumanDecision
        );
        let owner = "a".repeat(64);
        let request_state = authorization_request_state(&state, &request, &owner).unwrap();
        assert_eq!(
            authorization_elicitation_params(&request)["requestedSchema"]["properties"]["scope"]
                ["enum"],
            json!(["deny"])
        );
        for content in [
            json!({"scope":"exact"}),
            json!({"scope":"all_commands"}),
            json!({"approved":true}),
        ] {
            let error = apply_authorization_response(
                &state,
                &owner,
                &request_state,
                &json!({"action":"accept","content":content}),
            )
            .unwrap_err();
            assert!(
                error.contains("cannot establish human authority"),
                "{error}"
            );
        }
        assert_eq!(
            state
                .workspaces
                .authorization_request(&request.id)
                .unwrap()
                .status,
            crate::authorization::AuthorizationStatus::Pending
        );
        assert!(state.workspaces.approve_authorization_session(&request.id));
        let other_client = crate::mcp::call_tool_owned(&state, json!({"name":tool,"arguments":{
            "plan_id":plan,"approver":"client-forged-admin","statement":"I reviewed this exact plan.","confirmed":true,
        }}), &"b".repeat(64)).await.unwrap();
        assert_eq!(other_client["isError"], true, "{other_client}");
        assert_ne!(
            other_client["structuredContent"]["authorization_required"]["fingerprint"],
            request.fingerprint
        );
        let changed = human_decision_call(&state, tool, plan, "A different decision.").await;
        assert_eq!(changed["isError"], true, "{changed}");
        assert_ne!(
            changed["structuredContent"]["authorization_required"]["id"],
            request.id
        );
        let approved = human_decision_call(&state, tool, plan, "I reviewed this exact plan.").await;
        assert_eq!(approved["isError"], false, "{approved}");
        if tool == "verification_approve" {
            assert_eq!(approved["structuredContent"]["authority"], "local_operator");
        }
        let receipt = &approved["structuredContent"]["operator_authorization"];
        assert_eq!(receipt["id"], request.id);
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
        assert!(serialized.contains("human:local_operator:"), "{serialized}");
        assert!(
            serialized.contains("\"authority\":\"local_operator\""),
            "{serialized}"
        );
        assert!(
            !serialized.contains("human:client-forged-admin"),
            "{serialized}"
        );
        assert!(
            serialized.contains(receipt["fingerprint"].as_str().unwrap()),
            "{serialized}"
        );
        let replay = human_decision_call(&state, tool, plan, "I reviewed this exact plan.").await;
        assert_eq!(replay["isError"], true, "{replay}");
        assert!(!state.workspaces.approve_authorization_session(&request.id));
    }
}

#[tokio::test]
async fn human_decision_server_binding_rejects_restart_and_revision_changes() {
    let (state, root) = human_decision_state();
    let initial = human_decision_call(
        &state,
        "verification_approve",
        "VP-human-bound",
        "Reviewed.",
    )
    .await;
    let request = authorization_request_from_tool_result(&state, &initial).unwrap();
    assert!(state.workspaces.approve_authorization_session(&request.id));
    let restarted = Arc::new(AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1".into())),
        workspaces: state.workspaces.clone(),
        harness: ToolHarness::new(2).unwrap(),
        monitor: state.monitor.clone(),
        tasks: TaskRuntime::default(),
    });
    let after_restart = human_decision_call(
        &restarted,
        "verification_approve",
        "VP-human-bound",
        "Reviewed.",
    )
    .await;
    assert_eq!(after_restart["isError"], true, "{after_restart}");
    assert_ne!(
        after_restart["structuredContent"]["authorization_required"]["fingerprint"],
        request.fingerprint
    );
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
    assert!(
        !state
            .harness
            .verification_status(state.workspaces.default_id(), &workspace, "VP-human-bound")
            .unwrap()
            .human_approval
    );
}
