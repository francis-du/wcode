use super::*;

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
        protocol_for_message(&modern, DEFAULT_LEGACY_PROTOCOL),
        MODERN_PROTOCOL_VERSION
    );
    let discover = json!({"jsonrpc":"2.0","id":2,"method":"server/discover"});
    assert_eq!(
        protocol_for_message(&discover, DEFAULT_LEGACY_PROTOCOL),
        MODERN_PROTOCOL_VERSION
    );
    let legacy = json!({"jsonrpc":"2.0","id":3,"method":"tools/list","params":{}});
    assert_eq!(protocol_for_message(&legacy, "2025-06-18"), "2025-06-18");

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
