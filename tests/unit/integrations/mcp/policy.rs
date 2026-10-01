use super::*;
use crate::authorization::{AuthorizationKind, AuthorizationRequest, AuthorizationStatus};
use crate::design::{AcceptancePolicy, PolicyLevel, PolicyRequirements, ProjectDesign};
use crate::mcp::{
    apply_authorization_response, authorization_elicitation_params,
    authorization_request_from_tool_result, authorization_request_state,
};
use crate::workspace::{WorkspaceSecurity, Workspaces};
use std::sync::Arc;

fn app(root: &std::path::Path) -> AppState {
    let workspaces = Workspaces::new_with_security(
        [root],
        true,
        true,
        WorkspaceSecurity {
            allow_risky_exec: true,
            allow_unrestricted_commands: true,
            ..Default::default()
        },
    )
    .unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".into())),
        workspaces,
        harness: ToolHarness::new(2).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    }
}

fn fixture() -> (tempfile::TempDir, AppState) {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("src")).unwrap();
    std::fs::write(
        root.path().join("Cargo.toml"),
        "[package]\nname = \"policy_fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::write(root.path().join("src/lib.rs"), "pub fn baseline() {}\n").unwrap();
    let state = app(root.path());
    let workspace_id = state.workspaces.default_id().to_owned();
    let (_, workspace) = state.workspaces.select(None).unwrap();
    state
        .harness
        .design_init(
            &workspace_id,
            &workspace,
            "Policy fixture",
            "Transport fixture",
        )
        .unwrap();
    let project = ProjectDesign {
        schema_version: 1,
        name: "Policy fixture".into(),
        description: "Native transport fixture".into(),
        acceptance_policy: Some(AcceptancePolicy {
            schema_version: 1,
            id: "mcp-policy".into(),
            version: 1,
            requirements: PolicyRequirements {
                minimum_level: PolicyLevel::Quick,
                checks: vec!["rust-check".into()],
                stages: Vec::new(),
                reviewers: Vec::new(),
                human_approval: false,
                human_approval_min_risk: None,
            },
            docs_only: None,
            rules: Vec::new(),
        }),
    };
    std::fs::write(
        root.path().join(crate::design::PROJECT_FILE),
        serde_yaml::to_string(&project).unwrap(),
    )
    .unwrap();
    (root, state)
}

async fn invoke(state: &AppState, args: Value, owner: char) -> Value {
    crate::mcp::call_tool_owned(
        state,
        json!({"name":"acceptance_policy","arguments":args}),
        &owner.to_string().repeat(64),
    )
    .await
    .unwrap()
}

async fn activation_args(state: &AppState) -> Value {
    let preview = invoke(state, json!({"action":"preview"}), 'a').await;
    assert_eq!(preview["isError"], false, "{preview}");
    json!({
        "action":"activate", "expected_generation":0,
        "snapshot_digest":preview["structuredContent"]["snapshot_digest"],
    })
}

fn pending(state: &AppState, result: &Value) -> AuthorizationRequest {
    assert_eq!(result["isError"], true, "{result}");
    let request = authorization_request_from_tool_result(state, result).unwrap();
    assert_eq!(request.kind, AuthorizationKind::HumanDecision);
    request
}

async fn activate_fixture(state: &AppState) -> Value {
    let args = activation_args(state).await;
    let request = pending(state, &invoke(state, args.clone(), 'a').await);
    assert!(state.workspaces.approve_authorization_session(&request.id));
    let approved = invoke(state, args, 'a').await;
    assert_eq!(approved["isError"], false, "{approved}");
    assert_eq!(approved["structuredContent"]["authority"], "local_operator");
    assert_eq!(approved["structuredContent"]["acceptance_record"], false);
    approved
}

#[test]
fn mcp_policy_catalog_registers_one_scoped_strict_tool() {
    let tools = crate::mcp::mcp_tools::tools();
    let matches = tools
        .iter()
        .filter(|tool| tool["name"] == "acceptance_policy")
        .collect::<Vec<_>>();
    assert_eq!(matches.len(), 1);
    let tool = matches[0];
    assert_eq!(tool["inputSchema"]["additionalProperties"], false);
    assert_eq!(
        tool["inputSchema"]["properties"]["action"]["enum"],
        json!(["status", "preview", "activate", "revoke"])
    );
    let properties = tool["inputSchema"]["properties"].as_object().unwrap();
    for name in [
        "confirmed",
        "approver",
        "snapshot",
        "command",
        "args",
        "authority",
    ] {
        assert!(!properties.contains_key(name), "{name}");
    }
    assert_eq!(tool["annotations"]["readOnlyHint"], false);
    assert_eq!(
        tool["_meta"]["dev.wcode/productScopes"],
        json!(["design", "verification", "workspace"])
    );
}

#[tokio::test]
async fn mcp_policy_status_and_native_preview_never_request_approval() {
    let (_root, state) = fixture();
    let status = invoke(&state, json!({}), 'a').await;
    assert_eq!(status["isError"], false, "{status}");
    assert_eq!(status["structuredContent"]["status"], "inactive");
    assert_eq!(status["structuredContent"]["generation"], 0);
    assert_eq!(status["structuredContent"]["authority"], "none");
    let preview = invoke(&state, json!({"action":"preview"}), 'a').await;
    assert_eq!(preview["isError"], false, "{preview}");
    let content = &preview["structuredContent"];
    let snapshot: PolicySnapshot = serde_json::from_value(content["snapshot"].clone()).unwrap();
    assert_eq!(content["snapshot_digest"], snapshot.digest().unwrap());
    assert_eq!(content["authority"], "none");
    assert_eq!(content["acceptance_record"], false);
    assert!(state.workspaces.authorization_requests(256).is_empty());
}

#[tokio::test]
async fn mcp_policy_activation_requires_operator_transport_grant_and_exact_retry() {
    let (_root, state) = fixture();
    state
        .workspaces
        .set_all_commands_authorized(None, true)
        .unwrap();
    let args = activation_args(&state).await;
    let initial = invoke(&state, args.clone(), 'a').await;
    let request = pending(&state, &initial);
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
        AuthorizationStatus::Pending
    );
    assert!(state.workspaces.approve_authorization_session(&request.id));

    let other_owner = pending(&state, &invoke(&state, args.clone(), 'b').await);
    assert_ne!(other_owner.fingerprint, request.fingerprint);
    let mut other_expiry = args.clone();
    other_expiry["expires_at_ms"] = json!(now_ms().unwrap() + 60_000);
    let other_expiry = pending(&state, &invoke(&state, other_expiry, 'a').await);
    assert_ne!(other_expiry.fingerprint, request.fingerprint);
    let approved = invoke(&state, args.clone(), 'a').await;
    assert_eq!(approved["isError"], false, "{approved}");
    assert_eq!(approved["structuredContent"]["authority"], "local_operator");
    assert_eq!(approved["structuredContent"]["record"]["generation"], 1);
    assert_eq!(
        approved["structuredContent"]["record"]["operator_receipt"]["request_id"],
        format!("{}:{}", state.auth.instance_id(), request.id)
    );
    assert_eq!(
        approved["structuredContent"]["requester_binding"],
        mcp_writer::owner_binding(&owner)
    );
    assert_eq!(
        state
            .workspaces
            .authorization_request(&request.id)
            .unwrap()
            .status,
        AuthorizationStatus::Consumed
    );
    let count = state.workspaces.authorization_requests(256).len();
    let replay = invoke(&state, args, 'a').await;
    assert_eq!(replay["isError"], true, "{replay}");
    assert!(replay["structuredContent"]["error"]
        .as_str()
        .unwrap()
        .contains("generation changed"));
    assert!(replay["structuredContent"]
        .get("authorization_required")
        .is_none());
    assert_eq!(state.workspaces.authorization_requests(256).len(), count);
    let status = invoke(&state, json!({}), 'a').await;
    assert_eq!(status["structuredContent"]["status"], "active", "{status}");
}

#[tokio::test]
async fn mcp_policy_revision_and_native_definition_drift_reject_before_grant_consumption() {
    for (path, replacement) in [
        ("src/lib.rs", "pub fn changed() {}\n"),
        (
            "Cargo.toml",
            "[package]\nname = \"policy_fixture\"\nversion = \"0.1.1\"\nedition = \"2021\"\n",
        ),
    ] {
        let (root, state) = fixture();
        let args = activation_args(&state).await;
        let request = pending(&state, &invoke(&state, args.clone(), 'a').await);
        assert!(state.workspaces.approve_authorization_session(&request.id));
        std::fs::write(root.path().join(path), replacement).unwrap();
        let count = state.workspaces.authorization_requests(256).len();
        let stale = invoke(&state, args, 'a').await;
        assert_eq!(stale["isError"], true, "{stale}");
        assert!(
            stale["structuredContent"]["error"]
                .as_str()
                .unwrap()
                .contains("snapshot changed"),
            "{stale}"
        );
        assert!(stale["structuredContent"]
            .get("authorization_required")
            .is_none());
        assert_eq!(state.workspaces.authorization_requests(256).len(), count);
        assert_eq!(
            state
                .workspaces
                .authorization_request(&request.id)
                .unwrap()
                .status,
            AuthorizationStatus::ApprovedOnce
        );
        let fresh_args = activation_args(&state).await;
        let fresh_request = pending(&state, &invoke(&state, fresh_args, 'a').await);
        assert_ne!(fresh_request.fingerprint, request.fingerprint);
    }
}

#[tokio::test]
async fn mcp_policy_instance_binding_cannot_consume_another_server_grant() {
    let (_root, state) = fixture();
    let args = activation_args(&state).await;
    let request = pending(&state, &invoke(&state, args.clone(), 'a').await);
    assert!(state.workspaces.approve_authorization_session(&request.id));
    let other_instance = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".into())),
        workspaces: state.workspaces.clone(),
        harness: state.harness.clone(),
        monitor: state.monitor.clone(),
        tasks: TaskRuntime::default(),
    };
    assert_ne!(other_instance.auth.instance_id(), state.auth.instance_id());
    let other = pending(
        &other_instance,
        &invoke(&other_instance, args.clone(), 'a').await,
    );
    assert_ne!(other.fingerprint, request.fingerprint);
    assert_eq!(
        state
            .workspaces
            .authorization_request(&request.id)
            .unwrap()
            .status,
        AuthorizationStatus::ApprovedOnce
    );
    assert_eq!(invoke(&state, args, 'a').await["isError"], false);
}

#[tokio::test]
async fn mcp_policy_revocation_is_operator_bound_and_does_not_require_a_draft() {
    let (root, state) = fixture();
    activate_fixture(&state).await;
    std::fs::remove_file(root.path().join(crate::design::PROJECT_FILE)).unwrap();
    let preview = invoke(&state, json!({"action":"preview"}), 'a').await;
    assert_eq!(preview["isError"], true, "{preview}");
    let args = json!({"action":"revoke","expected_generation":1});
    let request = pending(&state, &invoke(&state, args.clone(), 'a').await);
    assert!(request.summary.contains("Policy revoke"));
    assert!(state.workspaces.approve_authorization_session(&request.id));
    let other = pending(&state, &invoke(&state, args.clone(), 'b').await);
    assert_ne!(other.fingerprint, request.fingerprint);
    let revoked = invoke(&state, args, 'a').await;
    assert_eq!(revoked["isError"], false, "{revoked}");
    assert_eq!(revoked["structuredContent"]["authority"], "local_operator");
    assert_eq!(revoked["structuredContent"]["record"]["action"], "revoke");
    assert_eq!(revoked["structuredContent"]["record"]["generation"], 2);
    let status = invoke(&state, json!({}), 'a').await;
    assert_eq!(status["structuredContent"]["status"], "revoked", "{status}");
}

#[tokio::test]
async fn mcp_policy_rejects_forged_inputs_and_stale_generation_without_prompting() {
    let (_root, state) = fixture();
    let valid = activation_args(&state).await;
    let mut malformed = vec![
        Value::Null,
        json!({"action":null}),
        json!({"action":"unknown"}),
        json!({"action":"activate"}),
        json!({"action":"revoke"}),
        json!({"expected_generation":null}),
        json!({"action":"status","expected_generation":0}),
        json!({"action":"preview","expires_at_ms":1}),
        json!({"action":"revoke","expected_generation":0,"snapshot_digest":"x"}),
    ];
    for (key, value) in [
        ("confirmed", json!(true)),
        ("approver", json!("forged-human")),
        ("snapshot", json!({"authority":"local_operator"})),
        ("command", json!("arbitrary-shell")),
        ("expected_generation", json!(-1)),
        ("expected_generation", json!("0")),
        ("expected_generation", json!(1)),
        ("snapshot_digest", json!("sha256:abc")),
        (
            "snapshot_digest",
            json!(format!("sha256:{}", "A".repeat(64))),
        ),
        ("expires_at_ms", Value::Null),
        ("expires_at_ms", json!(0)),
        (
            "expires_at_ms",
            json!(now_ms().unwrap() + MAX_POLICY_LIFETIME_MS + 60_000),
        ),
        ("expires_at_ms", json!(u64::MAX)),
    ] {
        let mut args = valid.clone();
        args[key] = value;
        malformed.push(args);
    }
    for args in malformed {
        let result = crate::mcp::call_tool_owned(
            &state,
            json!({"name":"acceptance_policy","arguments":args}),
            &"a".repeat(64),
        )
        .await;
        if let Ok(result) = result {
            assert_eq!(result["isError"], true, "{result}");
            assert!(
                result["structuredContent"]
                    .get("authorization_required")
                    .is_none(),
                "{result}"
            );
        }
    }
    assert!(state.workspaces.authorization_requests(256).is_empty());
}

#[tokio::test]
async fn mcp_policy_restart_namespaces_receipts_when_authorization_ids_repeat() {
    let (root, state) = fixture();
    let original_instance = state.auth.instance_id().to_owned();
    let activated = activate_fixture(&state).await;
    let first_id = activated["structuredContent"]["operator_authorization"]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        activated["structuredContent"]["record"]["operator_receipt"]["request_id"],
        format!("{original_instance}:{first_id}")
    );
    drop(state);

    let restarted = app(root.path());
    assert_ne!(restarted.auth.instance_id(), original_instance);
    let status = invoke(&restarted, json!({}), 'a').await;
    assert_eq!(status["isError"], false, "{status}");
    assert_eq!(status["structuredContent"]["generation"], 1);
    let args = json!({"action":"revoke","expected_generation":1});
    let request = pending(&restarted, &invoke(&restarted, args.clone(), 'a').await);
    assert_eq!(
        request.id, first_id,
        "real AuthorizationManager numbering restarts"
    );
    assert!(restarted
        .workspaces
        .approve_authorization_session(&request.id));
    let revoked = invoke(&restarted, args, 'a').await;
    assert_eq!(revoked["isError"], false, "{revoked}");
    assert_eq!(revoked["structuredContent"]["record"]["generation"], 2);
    assert_eq!(
        revoked["structuredContent"]["record"]["operator_receipt"]["request_id"],
        format!("{}:{}", restarted.auth.instance_id(), request.id)
    );
    assert_eq!(
        invoke(&restarted, json!({}), 'a').await["structuredContent"]["status"],
        "revoked"
    );
}
