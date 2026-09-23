use super::test_support::*;
use super::*;
use crate::task_store;
use crate::workspace::WorkspaceSecurity;

fn cancellation_test_state(root: &std::path::Path) -> Arc<AppState> {
    let workspaces = Workspaces::new([root], true, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    Arc::new(AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(1).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    })
}

async fn wait_for_task_queue(state: &AppState, count: usize) {
    tokio::time::timeout(std::time::Duration::from_secs(3), async {
        while state.monitor.connection_status().queued_tasks as usize != count {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap_or_else(|_| {
        panic!(
            "expected {count} queued tasks, observed {}",
            state.monitor.connection_status().queued_tasks
        )
    });
}

#[test]
fn durable_recovery_missing_worker_is_failed_not_left_working() {
    let root = tempfile::tempdir().unwrap();
    let state = cancellation_test_state(root.path());
    let (workspace_id, workspace) = state.workspaces.select(None).unwrap();
    let owner = "a".repeat(64);
    let record = task_store::TaskRecord::working(
        owner.clone(),
        workspace_id,
        "semantic_provider_refresh".into(),
        state.auth.instance_id().to_owned(),
    );
    task_store::persist(&workspace, &record).unwrap();
    assert!(get_task(&state, &record.task_id, &"b".repeat(64)).is_err());
    assert_eq!(
        task_store::load(&workspace, &record.task_id)
            .unwrap()
            .unwrap()
            .status,
        task_store::TaskStatus::Working
    );
    let recovered = get_task(&state, &record.task_id, &owner).unwrap();
    assert_eq!(recovered["status"], "failed");
    assert_eq!(recovered["error"]["code"], -32603);
    assert!(recovered["error"]["message"]
        .as_str()
        .unwrap()
        .contains("worker"));
    assert_eq!(
        get_task(&state, &record.task_id, &owner).unwrap()["status"],
        "failed"
    );
}

#[tokio::test]
async fn durable_recovery_subspace_tasks_keep_poll_and_cancel_ownership() {
    let root = tempfile::tempdir().unwrap();
    let child = root.path().join("nested/project");
    std::fs::create_dir_all(&child).unwrap();
    std::fs::write(
        child.join("Cargo.toml"),
        "[package]\nname = \"fixture\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    let state = cancellation_test_state(root.path());
    let (_, workspace) = state.workspaces.select(Some("nested/project")).unwrap();
    let permit = state.harness.acquire().await.unwrap();
    let owner = "a".repeat(64);
    let created = create_tool_task(
        state.clone(),
        json!({"name":"semantic_provider_refresh","arguments":{"workspace":"nested/project"}}),
        owner.clone(),
    )
    .await
    .unwrap();
    let id = created["taskId"].as_str().unwrap();
    wait_for_task_queue(&state, 1).await;
    assert!(task_store::load(&workspace, id).unwrap().is_some());
    assert_eq!(get_task(&state, id, &owner).unwrap()["status"], "working");
    assert!(cancel_task(&state, id, &"b".repeat(64)).is_err());
    cancel_task(&state, id, &owner).unwrap();
    wait_for_task_queue(&state, 0).await;
    drop(permit);
    let reloaded = cancellation_test_state(root.path());
    assert_eq!(
        get_task(&reloaded, id, &owner).unwrap()["status"],
        "cancelled"
    );
}

#[tokio::test]
async fn storage_failure_does_not_prevent_task_cancellation_or_expiry() {
    use sha2::{Digest, Sha256};
    for expired in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let state = cancellation_test_state(root.path());
        let permit = state.harness.acquire().await.unwrap();
        let owner = "a".repeat(64);
        let created = create_tool_task(
            state.clone(),
            json!({"name":"semantic_provider_refresh","arguments":{}}),
            owner.clone(),
        )
        .await
        .unwrap();
        let id = created["taskId"].as_str().unwrap();
        wait_for_task_queue(&state, 1).await;
        let (_, workspace, mut record) = task_store::find(&state.workspaces, id).unwrap().unwrap();
        // Pin the next timestamp without timing assumptions or global clock changes.
        record.updated_at_ms = u64::MAX - 2;
        if expired {
            record.created_at_ms = record.created_at_ms.saturating_sub(record.ttl_ms + 1);
        }
        task_store::persist(&workspace, &record).unwrap();
        let mut terminal = record.clone();
        if expired {
            terminal.fail(-32603, "task exceeded its durable TTL".to_owned());
        } else {
            terminal.cancel();
        }
        let bytes = serde_json::to_vec(&terminal).unwrap();
        let digest = format!("{:x}", Sha256::digest(&bytes));
        let blocked = crate::evidence_store::workspace_state_directory(&workspace)
            .unwrap()
            .join("mcp-tasks")
            .join(id)
            .join(format!(
                "{:020}-{}.json",
                terminal.updated_at_ms,
                &digest[..24]
            ));
        // A directory at the exact output path lets reads succeed but blocks saving.
        std::fs::create_dir(&blocked).unwrap();
        let result = if expired {
            get_task(&state, id, &owner)
        } else {
            cancel_task(&state, id, &owner)
        };
        assert_eq!(result.unwrap_err().code(), -32603);
        wait_for_task_queue(&state, 0).await;
        drop(permit);
        let _permit = state.harness.acquire().await.unwrap();
        std::fs::remove_dir(&blocked).unwrap();
        assert_eq!(get_task(&state, id, &owner).unwrap()["status"], "failed");
    }
}

#[tokio::test]
async fn durable_task_cancellation_aborts_its_queued_tool_worker() {
    let root = tempfile::tempdir().unwrap();
    let state = cancellation_test_state(root.path());
    let permit = state.harness.acquire().await.unwrap();
    let owner = "a".repeat(64);
    let created = create_tool_task(
        state.clone(),
        json!({"name":"semantic_provider_refresh","arguments":{}}),
        owner.clone(),
    )
    .await
    .unwrap();
    let task_id = created["taskId"].as_str().unwrap();
    wait_for_task_queue(&state, 1).await;

    assert!(cancel_task(&state, task_id, &"b".repeat(64)).is_err());
    assert_eq!(
        get_task(&state, task_id, &owner).unwrap()["status"],
        "working"
    );
    cancel_task(&state, task_id, &owner).unwrap();
    wait_for_task_queue(&state, 0).await;
    drop(permit);
    let _permit = state.harness.acquire().await.unwrap();
    assert_eq!(
        get_task(&state, task_id, &owner).unwrap()["status"],
        "cancelled"
    );
    cancel_task(&state, task_id, &owner).unwrap();
    assert_eq!(
        get_task(&state, task_id, &owner).unwrap()["status"],
        "cancelled"
    );
}

#[tokio::test]
async fn expired_durable_task_aborts_its_queued_tool_worker() {
    let root = tempfile::tempdir().unwrap();
    let state = cancellation_test_state(root.path());
    let permit = state.harness.acquire().await.unwrap();
    let owner = "a".repeat(64);
    let created = create_tool_task(
        state.clone(),
        json!({"name":"semantic_provider_refresh","arguments":{}}),
        owner.clone(),
    )
    .await
    .unwrap();
    let task_id = created["taskId"].as_str().unwrap();
    wait_for_task_queue(&state, 1).await;
    let (_, workspace, mut record) = task_store::find(&state.workspaces, task_id)
        .unwrap()
        .unwrap();
    record.created_at_ms = record.created_at_ms.saturating_sub(record.ttl_ms + 1);
    record.updated_at_ms += 1;
    task_store::persist(&workspace, &record).unwrap();
    let expired = get_task(&state, task_id, &owner).unwrap();
    assert_eq!(expired["status"], "failed");
    assert!(expired["error"]["message"]
        .as_str()
        .unwrap()
        .contains("TTL"));
    wait_for_task_queue(&state, 0).await;
    drop(permit);
    let _permit = state.harness.acquire().await.unwrap();
    assert_eq!(
        get_task(&state, task_id, &owner).unwrap()["status"],
        "failed"
    );
}

#[tokio::test]
async fn isolated_request_cancellation_aborts_its_queued_write() {
    for (params, queued) in [
        (
            json!({"name":"create_file","arguments":{"path":"cancelled.txt","content":"must not run"}}),
            1,
        ),
        (
            json!({"name":"parallel_tools","arguments":{"tasks":[
                {"tool":"create_file","arguments":{"path":"cancelled.txt","content":"must not run"}},
                {"tool":"create_file","arguments":{"path":"second.txt","content":"must not run"}}
            ]}}),
            2,
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        let state = cancellation_test_state(root.path());
        let permit = state.harness.acquire().await.unwrap();
        let child_state = state.clone();
        let request = tokio::spawn(async move {
            handle_message_isolated(
                child_state,
                modern_request("tools/call", params),
                MODERN_PROTOCOL_VERSION,
                &"a".repeat(64),
            )
            .await
        });
        wait_for_task_queue(&state, queued).await;
        request.abort();
        assert!(request.await.unwrap_err().is_cancelled());
        wait_for_task_queue(&state, 0).await;
        drop(permit);
        let _permit = state.harness.acquire().await.unwrap();
        assert!(!root.path().join("cancelled.txt").exists());
        assert!(!root.path().join("second.txt").exists());
    }
}

#[tokio::test]
async fn isolated_message_turns_panics_and_cancellation_into_jsonrpc_errors() {
    let mut panic_tasks = JoinSet::new();
    panic_tasks.spawn(async move {
        panic!("synthetic MCP child panic");
        #[allow(unreachable_code)]
        None::<Value>
    });
    let panic_response = join_message_task(Some(json!(41)), panic_tasks)
        .await
        .unwrap();
    assert_eq!(panic_response["error"]["code"], -32603);
    assert!(panic_response["error"]["message"]
        .as_str()
        .unwrap()
        .contains("panicked"));

    let mut cancelled_tasks = JoinSet::new();
    cancelled_tasks.spawn(async move { std::future::pending::<Option<Value>>().await });
    cancelled_tasks.abort_all();
    let cancelled_response = join_message_task(Some(json!(42)), cancelled_tasks)
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
    let auth = AuthState::new("https://example.com/gateway".to_owned());
    let mut headers = HeaderMap::new();
    assert!(origin_allowed(&auth, &headers));

    headers.insert("origin", "https://example.com".parse().unwrap());
    assert!(origin_allowed(&auth, &headers));

    headers.insert("origin", "https://grok.com".parse().unwrap());
    assert!(!origin_allowed(&auth, &headers));
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
    assert_eq!(
        validate_modern_request(&modern_headers("server/discover", None), &bootstrap),
        Err("missing 2026 request _meta envelope")
    );
}

#[tokio::test]
async fn modern_ping_is_not_a_supported_2026_method() {
    let root = tempfile::tempdir().unwrap();
    let state = cancellation_test_state(root.path());
    let response = handle_message(
        state,
        modern_request("ping", json!({})),
        MODERN_PROTOCOL_VERSION,
        &"a".repeat(64),
    )
    .await
    .unwrap();
    assert_eq!(response["error"]["code"], -32601);
    assert!(response["error"]["message"]
        .as_str()
        .unwrap()
        .contains("Method not found: ping"));
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
fn server_instructions_make_task_scoped_tool_selection_clear_up_front() {
    let prefix = SERVER_INSTRUCTIONS.chars().take(512).collect::<String>();
    assert!(prefix.contains("agent_context"));
    assert!(prefix.contains("capabilities.recommended_actions"));
    assert!(prefix.contains("complete tools/list is a compatibility catalog"));
    assert!(SERVER_INSTRUCTIONS.len() < 1_000);
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
async fn tools_list_reports_progressive_disclosure_catalog_metrics() {
    let dir = tempfile::tempdir().unwrap();
    let state = cancellation_test_state(dir.path());
    for protocol in LEGACY_PROTOCOL_VERSIONS
        .iter()
        .copied()
        .chain([MODERN_PROTOCOL_VERSION])
    {
        let response = handle_message(
            state.clone(),
            modern_request("tools/list", json!({})),
            protocol,
            &"a".repeat(64),
        )
        .await
        .unwrap();
        let catalog = response["result"]["tools"].as_array().unwrap();
        assert_eq!(catalog.as_slice(), mcp_tools::tools());
        assert!(response["result"].get("nextCursor").is_none());
        let metrics = &response["result"]["_meta"]["dev.wcode/catalog"];
        assert_eq!(metrics["tool_count"], catalog.len());
        assert_eq!(metrics["core_tool_count"], 4);
        assert!(metrics["on_demand_tool_count"].as_u64().unwrap() > 4);
        assert!(metrics["catalog_bytes"].as_u64().unwrap() > 0);
        assert!(metrics["input_schema_bytes"].as_u64().unwrap() > 0);
        assert!(
            metrics["preload_catalog_bytes"].as_u64().unwrap()
                < metrics["catalog_bytes"].as_u64().unwrap()
        );
        assert!(
            metrics["preload_catalog_reduction_percent"]
                .as_u64()
                .unwrap()
                >= 50
        );
        assert_eq!(metrics["task_manifest"], "agent_context.capabilities");
        assert_eq!(metrics["dynamic_tool_list"], false);
        assert_eq!(
            metrics["dynamic_tool_list_policy"],
            "task_independent_protocol_catalog"
        );

        let on_demand = catalog
            .iter()
            .find(|tool| tool["name"] == "path_info")
            .unwrap();
        assert!(on_demand["_meta"]
            .get("dev.wcode/preloadRecommended")
            .is_none());
        let called = handle_message(
            state.clone(),
            modern_request(
                "tools/call",
                json!({"name":"path_info","arguments":{"path":"."}}),
            ),
            protocol,
            &"a".repeat(64),
        )
        .await
        .unwrap();
        assert_eq!(called["result"]["isError"], false, "{called}");
        assert!(called["result"]["structuredContent"].is_object());
    }
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
async fn modern_stdio_authorization_can_grant_all_commands_for_the_workspace_session() {
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
        monitor: TaskMonitor::new([workspace_id.clone()]),
        tasks: TaskRuntime::default(),
    });
    let owner = "c".repeat(64);
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
    let schema = &first["result"]["inputRequests"][AUTHORIZATION_INPUT_KEY]["params"]
        ["requestedSchema"]["properties"]["scope"];
    assert_eq!(schema["type"], "string");
    assert!(schema["enum"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "all_commands"));
    let request_state = first["result"]["requestState"].as_str().unwrap().to_owned();

    let mut retry = request;
    retry["id"] = json!(2);
    retry["params"]["requestState"] = json!(request_state);
    retry["params"]["inputResponses"] = json!({
        (AUTHORIZATION_INPUT_KEY): {"action":"accept","content":{"scope":"all_commands"}}
    });
    let completed = handle_message(state.clone(), retry, MODERN_PROTOCOL_VERSION, &owner)
        .await
        .unwrap();
    assert_eq!(completed["result"]["resultType"], "complete");
    assert_eq!(completed["result"]["isError"], false);
    assert!(state
        .workspaces
        .all_commands_authorized(Some(&workspace_id))
        .unwrap());
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
    assert_eq!(response["error"]["data"]["approvalSurface"], "tui_or_webui");
    assert_eq!(
        response["error"]["data"]["nextAction"],
        "approve_then_retry_same_tool"
    );
    assert!(response["error"]["data"]["authorizationRequestId"]
        .as_str()
        .is_some_and(|id| id.starts_with("AUTH-")));

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
    assert!(response["result"]["capabilities"]["extensions"]
        .as_object()
        .unwrap()
        .keys()
        .all(|key| key != "run.francis.wcode/media-content"));
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
    let held_permits = [
        state.harness.acquire().await.unwrap(),
        state.harness.acquire().await.unwrap(),
    ];
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        handle_message_isolated(
            state.clone(),
            task_capable(modern_request(
                "tools/call",
                json!({
                    "name":"semantic_provider_refresh",
                    "arguments":{"workspace":workspace_id}
                }),
            )),
            MODERN_PROTOCOL_VERSION,
            &owner,
        ),
    )
    .await
    .unwrap()
    .unwrap();
    let created = &response["result"];
    assert_eq!(created["resultType"], "task");
    assert_eq!(created["status"], "working");
    let task_id = created["taskId"].as_str().unwrap().to_owned();
    let found = task_store::find(&state.workspaces, &task_id)
        .unwrap()
        .expect("task must be durable before its handle is returned");
    assert_eq!(found.2.owner, owner);
    wait_for_task_queue(&state, 1).await;
    drop(held_permits);

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
