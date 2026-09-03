use super::test_support::*;
use super::*;
use crate::task_store;
use crate::workspace::WorkspaceSecurity;

#[tokio::test]
async fn isolated_message_turns_panics_and_cancellation_into_jsonrpc_errors() {
    let panic_task = tokio::spawn(async move {
        panic!("synthetic MCP child panic");
        #[allow(unreachable_code)]
        None::<Value>
    });
    let panic_response = join_message_task(Some(json!(41)), panic_task)
        .await
        .unwrap();
    assert_eq!(panic_response["error"]["code"], -32603);
    assert!(panic_response["error"]["message"]
        .as_str()
        .unwrap()
        .contains("panicked"));

    let cancelled_task = tokio::spawn(async move { std::future::pending::<Option<Value>>().await });
    cancelled_task.abort();
    let cancelled_response = join_message_task(Some(json!(42)), cancelled_task)
        .await
        .unwrap();
    assert_eq!(cancelled_response["error"]["code"], -32603);
    assert!(cancelled_response["error"]["message"]
        .as_str()
        .unwrap()
        .contains("cancelled"));

    let dir = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = Arc::new(AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(2).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    });
    let response = handle_message_isolated(
        state,
        modern_request(
            "tools/call",
            json!({"name":"workspace_info","arguments":{}}),
        ),
        MODERN_PROTOCOL_VERSION,
        &"a".repeat(64),
    )
    .await
    .unwrap();
    assert!(response.get("result").is_some());
}

#[test]
fn protocol_detection_supports_modern_and_legacy_clients() {
    let modern = modern_headers("tools/list", None);
    assert_eq!(
        request_protocol(&modern, &json!({})),
        MODERN_PROTOCOL_VERSION
    );

    let legacy = json!({"params": {"protocolVersion": "2024-11-05"}});
    assert_eq!(request_protocol(&HeaderMap::new(), &legacy), "2024-11-05");
    assert_eq!(
        request_protocol(&HeaderMap::new(), &json!({})),
        "2025-03-26"
    );
    assert_eq!(
        request_protocol(&HeaderMap::new(), &modern_request("tools/list", json!({}))),
        MODERN_PROTOCOL_VERSION
    );
}

#[test]
fn origin_validation_accepts_same_origin_and_rejects_cross_origin() {
    let mut headers = HeaderMap::new();
    assert!(origin_allowed("https://example.com/gateway", &headers));

    headers.insert("origin", "https://example.com".parse().unwrap());
    assert!(origin_allowed("https://example.com/gateway", &headers));

    headers.insert("origin", "https://grok.com".parse().unwrap());
    assert!(!origin_allowed("https://example.com/gateway", &headers));
}

#[test]
fn modern_requests_require_routing_headers_and_metadata() {
    let request = modern_request("tools/call", json!({"name": "workspace_info"}));
    let headers = modern_headers("tools/call", Some("workspace_info"));
    assert!(validate_modern_request(&headers, &request).is_ok());

    let mut wrong_name = headers.clone();
    wrong_name.insert("mcp-name", "read_file".parse().unwrap());
    assert_eq!(
        validate_modern_request(&wrong_name, &request),
        Err("Mcp-Name header does not match request body value")
    );

    let missing_meta = json!({"jsonrpc": "2.0", "id": 1, "method": "tools/list", "params": {}});
    assert!(validate_modern_request(&modern_headers("tools/list", None), &missing_meta).is_err());

    let bootstrap = json!({"jsonrpc": "2.0", "id": 2, "method": "server/discover"});
    assert!(validate_modern_request(&modern_headers("server/discover", None), &bootstrap).is_ok());
}

#[test]
fn modern_name_headers_cover_tools_prompts_resources_and_base64_sentinel() {
    let prompt = modern_request("prompts/get", json!({"name":"wcode-review"}));
    assert!(validate_modern_request(
        &modern_headers("prompts/get", Some("wcode-review")),
        &prompt
    )
    .is_ok());
    assert_eq!(
        validate_modern_request(&modern_headers("prompts/get", None), &prompt),
        Err("missing Mcp-Name header")
    );

    let uri = "wcode://runtime/安全";
    let encoded = format!("=?base64?{}?=", STANDARD.encode(uri.as_bytes()));
    let resource = modern_request("resources/read", json!({"uri":uri}));
    assert!(
        validate_modern_request(&modern_headers("resources/read", Some(&encoded)), &resource)
            .is_ok()
    );
    assert_eq!(decode_mcp_header_value(&encoded).unwrap(), uri);
    assert!(decode_mcp_header_value("=?base64?not%%%?=").is_err());
    assert!(decode_mcp_header_value(" padded ").is_err());
    assert!(decode_mcp_header_value("a\tb").is_err());
    assert_eq!(
        decode_mcp_header_value("=?base64?literal").unwrap(),
        "=?base64?literal"
    );

    let mut missing_protocol = modern_headers("tools/list", None);
    missing_protocol.remove("mcp-protocol-version");
    assert_eq!(
        validate_modern_request(&missing_protocol, &modern_request("tools/list", json!({}))),
        Err("missing MCP-Protocol-Version header")
    );
}

#[test]
fn modern_validation_uses_final_2026_error_codes() {
    assert_eq!(
        modern_validation_error_code("Mcp-Method header does not match the JSON-RPC method"),
        -32020
    );
    assert_eq!(
        modern_validation_error_code("missing MCP-Protocol-Version header"),
        -32020
    );
    assert_eq!(
        modern_validation_error_code("invalid Mcp-Name base64 sentinel encoding"),
        -32020
    );
    assert_eq!(
        modern_validation_error_code("missing 2026 request _meta envelope"),
        -32602
    );
    assert_eq!(
        modern_validation_error_code("JSON-RPC batches are not supported"),
        -32600
    );
    assert_eq!(TaskRpcError::missing_capability().code(), -32021);
    assert_eq!(
        mcp_response_status(&json!({"error":{"code":-32021}}), true),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        mcp_response_status(&json!({"error":{"code":-32601}}), true),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        mcp_response_status(&json!({"error":{"code":-32021}}), false),
        StatusCode::OK
    );
}

#[test]
fn modern_results_include_server_identity_and_private_cache_hints() {
    let result = modern_cacheable_result(json!({"tools": []}));
    assert_eq!(result["resultType"], "complete");
    assert_eq!(result["cacheScope"], "private");
    assert_eq!(result["ttlMs"], 300_000);
    assert_eq!(
        result["_meta"]["io.modelcontextprotocol/serverInfo"]["name"],
        "wcode"
    );
}

#[tokio::test]
async fn modern_stdio_authorization_uses_human_elicitation_and_bound_retry_state() {
    let dir = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([dir.path()], false, true).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    workspaces
        .revoke_command(Some(&workspace_id), "cargo")
        .unwrap();
    let state = Arc::new(AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(2).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    });
    let owner = "a".repeat(64);
    let request = elicitation_capable(modern_request(
        "tools/call",
        json!({
            "name":"run_command",
            "arguments":{"program":"cargo","args":["--version"],"cwd":".","timeout_seconds":30}
        }),
    ));

    let first = handle_message(
        state.clone(),
        request.clone(),
        MODERN_PROTOCOL_VERSION,
        &owner,
    )
    .await
    .unwrap();
    assert_eq!(first["result"]["resultType"], "input_required");
    assert_eq!(
        first["result"]["inputRequests"][AUTHORIZATION_INPUT_KEY]["method"],
        "elicitation/create"
    );
    assert_eq!(
        first["result"]["inputRequests"][AUTHORIZATION_INPUT_KEY]["params"]["requestedSchema"]
            ["properties"]["approved"]["type"],
        "boolean"
    );
    let request_state = first["result"]["requestState"].as_str().unwrap().to_owned();

    let mut forged_parts = request_state.split(':').collect::<Vec<_>>();
    assert_eq!(forged_parts.len(), 4);
    forged_parts[2] = "forged-challenge";
    let mut forged = request.clone();
    forged["id"] = json!(2);
    forged["params"]["requestState"] = json!(forged_parts.join(":"));
    forged["params"]["inputResponses"] = json!({
        (AUTHORIZATION_INPUT_KEY): {"action":"accept","content":{"approved":true}}
    });
    let rejected = handle_message(state.clone(), forged, MODERN_PROTOCOL_VERSION, &owner)
        .await
        .unwrap();
    assert_eq!(rejected["error"]["code"], -32602);
    assert!(rejected["error"]["message"]
        .as_str()
        .unwrap()
        .contains("challenge does not match"));

    let mut wrong_owner = request.clone();
    wrong_owner["id"] = json!(2);
    wrong_owner["params"]["requestState"] = json!(request_state.clone());
    wrong_owner["params"]["inputResponses"] = json!({
        (AUTHORIZATION_INPUT_KEY): {"action":"accept","content":{"approved":true}}
    });
    let rejected = handle_message(
        state.clone(),
        wrong_owner,
        MODERN_PROTOCOL_VERSION,
        &"b".repeat(64),
    )
    .await
    .unwrap();
    assert_eq!(rejected["error"]["code"], -32602);
    assert!(rejected["error"]["message"]
        .as_str()
        .unwrap()
        .contains("different MCP client"));

    let mut retry = request;
    retry["id"] = json!(3);
    retry["params"]["requestState"] = json!(request_state);
    retry["params"]["inputResponses"] = json!({
        (AUTHORIZATION_INPUT_KEY): {"action":"accept","content":{"approved":true}}
    });
    let completed = handle_message(state, retry, MODERN_PROTOCOL_VERSION, &owner)
        .await
        .unwrap();
    assert_eq!(completed["result"]["resultType"], "complete");
    assert_eq!(completed["result"]["isError"], false);
    assert_eq!(completed["result"]["structuredContent"]["success"], true);
}

#[tokio::test]
async fn modern_authorization_fails_closed_without_elicitation_capability() {
    let dir = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([dir.path()], false, true).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    workspaces
        .revoke_command(Some(&workspace_id), "cargo")
        .unwrap();
    let state = Arc::new(AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(2).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    });
    let response = handle_message(
        state.clone(),
        modern_request(
            "tools/call",
            json!({"name":"run_command","arguments":{"program":"cargo","args":["--version"]}}),
        ),
        MODERN_PROTOCOL_VERSION,
        &"a".repeat(64),
    )
    .await
    .unwrap();
    assert_eq!(response["error"]["code"], -32021);
    assert!(response["error"]["data"]["requiredCapabilities"]["elicitation"].is_object());

    let mut url_only = modern_request(
        "tools/call",
        json!({"name":"run_command","arguments":{"program":"cargo","args":["--version"]}}),
    );
    url_only["params"]["_meta"]["io.modelcontextprotocol/clientCapabilities"]["elicitation"] =
        json!({"url":{}});
    let response = handle_message(state, url_only, MODERN_PROTOCOL_VERSION, &"a".repeat(64))
        .await
        .unwrap();
    assert_eq!(response["error"]["code"], -32021);
}

#[tokio::test]
async fn task_capability_is_per_request_and_advertised_by_discovery() {
    let plain = modern_request(
        "tools/call",
        json!({"name":"semantic_provider_refresh","arguments":{}}),
    );
    assert!(!client_supports_tasks(&plain));
    let capable = task_capable(plain);
    assert!(client_supports_tasks(&capable));

    let dir = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = Arc::new(AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(2).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    });
    let response = handle_message(
        state,
        modern_request("server/discover", json!({})),
        MODERN_PROTOCOL_VERSION,
        &"a".repeat(64),
    )
    .await
    .unwrap();
    assert!(response["result"]["capabilities"]["extensions"][TASK_EXTENSION_ID].is_object());
    assert!(MEDIA_CONTENT_EXTENSION_ID.starts_with("run.francis.wcode/"));
    assert_eq!(
        response["result"]["capabilities"]["extensions"][MEDIA_CONTENT_EXTENSION_ID]
            ["contentTypes"],
        json!(["image", "audio"])
    );
    assert_eq!(
        response["result"]["capabilities"]["tools"]["listChanged"],
        false
    );
    assert_eq!(
        response["result"]["capabilities"]["prompts"]["listChanged"],
        false
    );
    assert_eq!(
        response["result"]["capabilities"]["resources"]["subscribe"],
        false
    );
}

#[tokio::test]
async fn task_augmented_tool_is_durable_pollable_and_owner_scoped() {
    let dir = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new_with_security(
        [dir.path()],
        false,
        true,
        WorkspaceSecurity {
            allow_risky_exec: true,
            ..WorkspaceSecurity::default()
        },
    )
    .unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = Arc::new(AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(2).unwrap(),
        monitor: TaskMonitor::new([workspace_id.clone()]),
        tasks: TaskRuntime::default(),
    });
    let owner = "a".repeat(64);
    let created = create_tool_task(
        state.clone(),
        json!({
            "name":"semantic_provider_refresh",
            "arguments":{"workspace":workspace_id}
        }),
        owner.clone(),
    )
    .await
    .unwrap();
    assert_eq!(created["resultType"], "task");
    assert_eq!(created["status"], "working");
    let task_id = created["taskId"].as_str().unwrap().to_owned();
    let found = task_store::find(&state.workspaces, &task_id)
        .unwrap()
        .expect("task must be durable before its handle is returned");
    assert_eq!(found.2.owner, owner);

    let mut completed = None;
    for _ in 0..100 {
        let current = get_task(&state, &task_id, &owner).unwrap();
        if current["status"] == "completed" {
            completed = Some(current);
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    let completed = completed.expect("background semantic refresh should complete");
    assert_eq!(completed["resultType"], "complete");
    assert_eq!(completed["result"]["isError"], false);
    assert_eq!(
        completed["result"]["structuredContent"]["runs"]
            .as_array()
            .unwrap()
            .len(),
        0
    );

    let other_owner = "b".repeat(64);
    let error = get_task(&state, &task_id, &other_owner).unwrap_err();
    assert_eq!(error.code(), -32602);
}

#[tokio::test]
async fn task_methods_require_the_extension_on_each_request() {
    let dir = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = Arc::new(AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(2).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    });
    let response = handle_message(
        state,
        modern_request("tasks/get", json!({"taskId":"TASK-unknown"})),
        MODERN_PROTOCOL_VERSION,
        &"a".repeat(64),
    )
    .await
    .unwrap();
    assert_eq!(response["error"]["code"], -32021);
    assert!(
        response["error"]["data"]["requiredCapabilities"]["extensions"][TASK_EXTENSION_ID]
            .is_object()
    );
}

#[test]
fn task_details_explain_work_without_exposing_payloads() {
    let search = json!({"path": "src", "query": "private implementation text"});
    let detail = mcp_tools::task_detail("search_code", &search);
    assert!(detail.contains("src"));
    assert!(detail.contains("27 chars"));
    assert!(!detail.contains("private implementation text"));

    let command = json!({
        "program": "cargo",
        "args": ["test", "--token", "very-secret", "--locked"],
        "cwd": "crates/core"
    });
    let detail = mcp_tools::task_detail("run_command", &command);
    assert!(detail.contains("cargo test"));
    assert!(detail.contains("[REDACTED]"));
    assert!(!detail.contains("very-secret"));
    assert!(detail.contains("crates/core"));

    let parallel = json!({
        "tasks": [
            {"tool": "read_file", "arguments": {"path": "src/lib.rs"}},
            {"tool": "search_code", "arguments": {"query": "secret payload"}}
        ]
    });
    let detail = mcp_tools::task_detail("parallel_tools", &parallel);
    assert_eq!(detail, "2 independent tool requests");
    assert!(!detail.contains("secret payload"));

    for orchestration_tool in [
        "review_changes",
        "drift_status",
        "risk_status",
        "impact_analysis",
        "verification_plan",
        "reconciliation_plan",
    ] {
        assert_ne!(
            mcp_tools::task_detail(orchestration_tool, &json!({})),
            "unknown tool request"
        );
    }
}

#[test]
fn estimates_context_avoided_only_for_precision_tools() {
    let value = json!({"source_bytes": 10_000});
    assert_eq!(
        estimated_context_bytes_avoided("symbol_context", &value, 2_000),
        8_000
    );
    assert_eq!(
        estimated_context_bytes_avoided(
            "agent_context",
            &json!({"context_bytes_avoided": 24_000}),
            3_000
        ),
        24_000
    );
    assert_eq!(
        estimated_context_bytes_avoided("file_outline", &value, 12_000),
        0
    );
    assert_eq!(
        estimated_context_bytes_avoided("read_file", &value, 2_000),
        0
    );
}
