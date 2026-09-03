use super::test_support::*;
use super::*;
use std::fs;

#[tokio::test]
async fn prompt_and_resource_catalog_flow_through_modern_mcp() {
    let dir = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = Arc::new(AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    });
    let owner = "a".repeat(64);

    let prompts = handle_message(
        state.clone(),
        modern_request("prompts/list", json!({})),
        MODERN_PROTOCOL_VERSION,
        &owner,
    )
    .await
    .unwrap();
    assert_eq!(prompts["result"]["prompts"].as_array().unwrap().len(), 3);

    let resource = handle_message(
        state,
        modern_request("resources/read", json!({"uri":"wcode://runtime/security"})),
        MODERN_PROTOCOL_VERSION,
        &owner,
    )
    .await
    .unwrap();
    assert!(resource["result"]["contents"][0]["text"]
        .as_str()
        .unwrap()
        .contains("workspace-scoped"));
    assert_eq!(resource["result"]["cacheScope"], "private");
    assert_eq!(resource["result"]["ttlMs"], 300_000);
}

#[tokio::test]
async fn routes_tools_to_selected_workspace() {
    let root = tempfile::tempdir().unwrap();
    let api = root.path().join("api");
    let web = root.path().join("web");
    fs::create_dir_all(&api).unwrap();
    fs::create_dir_all(&web).unwrap();
    fs::write(api.join("name.txt"), "api\n").unwrap();
    fs::write(web.join("name.txt"), "web\n").unwrap();

    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces: Workspaces::new([&api, &web], true, false).unwrap(),
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new(["api".to_owned(), "web".to_owned()]),
        tasks: TaskRuntime::default(),
    };
    let result = call_tool(
        &state,
        json!({
            "name": "read_file",
            "arguments": {"workspace": "web", "path": "name.txt"}
        }),
    )
    .await
    .unwrap();
    assert_eq!(result["structuredContent"]["workspace"], "web");
    assert_eq!(result["structuredContent"]["content"], "web");

    let sha = result["structuredContent"]["sha256"].as_str().unwrap();
    let edit = call_tool(
        &state,
        json!({
            "name": "replace_text",
            "arguments": {
                "workspace": "web",
                "path": "name.txt",
                "old_text": "web",
                "new_text": "frontend",
                "expected_sha256": sha
            }
        }),
    )
    .await
    .unwrap();
    assert_eq!(edit["structuredContent"]["workspace"], "web");
    assert_eq!(
        fs::read_to_string(web.join("name.txt")).unwrap(),
        "frontend\n"
    );
    assert_eq!(fs::read_to_string(api.join("name.txt")).unwrap(), "api\n");
}

#[tokio::test]
async fn workspace_info_exposes_parallel_scheduling_guidance() {
    let dir = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(16).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    };

    let result = call_tool(&state, json!({"name": "workspace_info", "arguments": {}}))
        .await
        .unwrap();
    let info = &result["structuredContent"];
    assert_eq!(info["scheduling"]["max_parallel"], 16);
    assert_eq!(info["scheduling"]["semantics"], "global-cap-not-target");
    assert_eq!(info["scheduling"]["fanout_tool"], "parallel_tools");
    assert!(info["mcp"]["transports"]
        .as_array()
        .unwrap()
        .iter()
        .any(|transport| transport == "stdio"));
    assert!(info["mcp"]["transports"]
        .as_array()
        .unwrap()
        .iter()
        .any(|transport| transport == "legacy-sse"));
    assert!(info["mcp"]["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .any(|capability| capability == "prompts"));
    assert_eq!(info["harness"]["phased_parallel_verification"], true);
    assert_eq!(
        info["harness"]["verification_exec_without_risky_flag"],
        true
    );
}

#[tokio::test]
async fn product_scope_status_flows_through_mcp() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir_all(dir.path().join("src/runtime")).unwrap();
    fs::write(
        dir.path().join("src/runtime/control.rs"),
        "pub fn control() {}\n",
    )
    .unwrap();
    let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    };

    let response = call_tool(&state, json!({"name": "scope_status", "arguments": {}}))
        .await
        .unwrap();
    assert_eq!(response["isError"], false);
    assert_eq!(response["structuredContent"]["source_files"], 1);
    assert_eq!(response["structuredContent"]["mapped_files"], 1);
    assert_eq!(response["structuredContent"]["counts"]["runtime"], 1);
}

#[tokio::test]
async fn bulk_read_isolates_missing_files() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("one.txt"), "one\n").unwrap();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces: Workspaces::new([dir.path()], false, false).unwrap(),
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string()]),
        tasks: TaskRuntime::default(),
    };
    let result = call_tool(
        &state,
        json!({
            "name": "read_files",
            "arguments": {"paths": ["one.txt", "missing.txt"]}
        }),
    )
    .await
    .unwrap();
    let files = result["structuredContent"]["files"].as_array().unwrap();
    assert_eq!(files[0]["ok"], true);
    assert_eq!(files[1]["ok"], false);
}

#[tokio::test]
async fn parallel_fanout_uses_child_slots_without_parent_deadlock() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("one.txt"), "one\n").unwrap();
    fs::write(dir.path().join("two.txt"), "two\n").unwrap();
    let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(1).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    };

    let response = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        call_tool(
            &state,
            json!({
                "name": "parallel_tools",
                "arguments": {
                    "tasks": [
                        {"id": "one", "tool": "read_file", "arguments": {"path": "one.txt"}},
                        {"id": "missing", "tool": "read_file", "arguments": {"path": "missing.txt"}},
                        {"id": "two", "tool": "read_file", "arguments": {"path": "two.txt"}}
                    ]
                }
            }),
        ),
    )
    .await
    .expect("fan-out must not deadlock with one semaphore slot")
    .unwrap();

    let result = &response["structuredContent"];
    assert_eq!(result["execution"], "parallel-fanout");
    assert_eq!(result["tasks"], 3);
    assert_eq!(result["succeeded"], 2);
    assert_eq!(result["failed"], 1);
}

#[tokio::test]
async fn parallel_fanout_inherits_the_parent_workspace() {
    let root = tempfile::tempdir().unwrap();
    let api = root.path().join("api");
    let web = root.path().join("web");
    fs::create_dir_all(&api).unwrap();
    fs::create_dir_all(&web).unwrap();
    fs::write(api.join("name.txt"), "api\n").unwrap();
    fs::write(web.join("name.txt"), "web\n").unwrap();
    let workspaces = Workspaces::new([&api, &web], false, false).unwrap();
    let default_id = workspaces.default_id().to_owned();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([default_id]),
        tasks: TaskRuntime::default(),
    };

    let response = call_tool(
        &state,
        json!({
            "name":"parallel_tools",
            "arguments":{
                "workspace":"web",
                "tasks":[
                    {"id":"a","tool":"read_file","arguments":{"path":"name.txt"}},
                    {"id":"b","tool":"path_info","arguments":{"path":"name.txt"}},
                    {"id":"api","tool":"read_file","arguments":{"workspace":"api","path":"name.txt"}}
                ]
            }
        }),
    )
    .await
    .unwrap();
    let items = response["structuredContent"]["items"].as_array().unwrap();
    assert_eq!(items[0]["result"]["content"], "web");
    assert_eq!(items[1]["result"]["path"], "name.txt");
    assert_eq!(items[2]["result"]["content"], "api");
}

#[tokio::test]
async fn parallel_fanout_runs_writes_and_coalesces_same_file_edits() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("shared.txt"), "same\nmiddle\nsame\n").unwrap();
    let workspaces = Workspaces::new([dir.path()], true, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    };
    let read = call_tool(
        &state,
        json!({"name":"read_file","arguments":{"path":"shared.txt"}}),
    )
    .await
    .unwrap();
    let sha = read["structuredContent"]["sha256"].as_str().unwrap();

    let response = call_tool(
        &state,
        json!({
            "name":"parallel_tools",
            "arguments":{"tasks":[
                {"id":"create-a","tool":"create_file","arguments":{"path":"a.txt","content":"a\n"}},
                {"id":"create-b","tool":"create_file","arguments":{"path":"b.txt","content":"b\n"}},
                {"id":"edit-first","tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":sha,"edits":[{"old_text":"same","new_text":"FIRST","start_line":1,"end_line":1}]}},
                {"id":"edit-last","tool":"apply_edits","arguments":{"path":"shared.txt","expected_sha256":sha,"edits":[{"old_text":"same","new_text":"LAST","start_line":3,"end_line":3}]}}
            ]}
        }),
    )
    .await
    .unwrap();

    let result = &response["structuredContent"];
    assert_eq!(result["succeeded"], 4);
    assert_eq!(result["failed"], 0);
    assert_eq!(result["coalesced_same_file_edits"], 1);
    assert_eq!(
        fs::read_to_string(dir.path().join("shared.txt")).unwrap(),
        "FIRST\nmiddle\nLAST\n"
    );
    assert_eq!(fs::read_to_string(dir.path().join("a.txt")).unwrap(), "a\n");
    assert_eq!(fs::read_to_string(dir.path().join("b.txt")).unwrap(), "b\n");
}

#[tokio::test]
async fn parallel_fanout_orders_real_directory_dependencies() {
    let dir = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([dir.path()], true, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    };

    let response = call_tool(
        &state,
        json!({
            "name":"parallel_tools",
            "arguments":{"tasks":[
                {"id":"child","tool":"create_file","arguments":{"path":"src/domain/model.rs","content":"pub struct Model;\n"}},
                {"id":"parent","tool":"create_directory","arguments":{"path":"src/domain"}}
            ]}
        }),
    )
    .await
    .unwrap();

    let result = &response["structuredContent"];
    assert_eq!(result["scheduler"], "dependency-graph");
    assert_eq!(result["dependency_edges"], 1);
    assert_eq!(result["dependency_layers"], 2);
    assert_eq!(result["succeeded"], 2);
    assert!(dir.path().join("src/domain/model.rs").is_file());
}

#[tokio::test]
async fn syntax_index_tools_flow_through_mcp() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("service.rs"),
        "pub struct Service;\nimpl Service { pub fn run(&self) { helper(); } }\nfn helper() {}\n",
    )
    .unwrap();
    let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([workspace_id]),
        tasks: TaskRuntime::default(),
    };

    let outline = call_tool(
        &state,
        json!({
            "name": "file_outline",
            "arguments": {"path": "service.rs"}
        }),
    )
    .await
    .unwrap();
    assert_eq!(outline["isError"], false);
    assert_eq!(outline["structuredContent"]["language"], "rust");
    assert!(outline["structuredContent"]["symbols"]
        .as_array()
        .unwrap()
        .iter()
        .any(|symbol| symbol["qualified_name"] == "Service::run"));

    let search = call_tool(
        &state,
        json!({
            "name": "find_symbol",
            "arguments": {"query": "Service::run"}
        }),
    )
    .await
    .unwrap();
    let symbol_id = search["structuredContent"]["results"][0]["id"]
        .as_str()
        .unwrap();
    let context = call_tool(
        &state,
        json!({
            "name": "symbol_context",
            "arguments": {"symbol_id": symbol_id, "max_body_lines": 50}
        }),
    )
    .await
    .unwrap();
    assert_eq!(context["isError"], false);
    assert!(context["structuredContent"]["body"]["content"]
        .as_str()
        .unwrap()
        .contains("pub fn run"));
    assert!(context["structuredContent"]["syntax_calls"]
        .as_array()
        .unwrap()
        .iter()
        .any(|call| call["name"] == "helper"));

    let navigation = call_tool(
        &state,
        json!({
            "name": "semantic_navigation",
            "arguments": {"path": "service.rs", "symbol": "Service::run", "intent": "calls"}
        }),
    )
    .await
    .unwrap();
    assert_eq!(navigation["isError"], false);
    assert_eq!(navigation["structuredContent"]["precision"], "syntax");
    assert_eq!(
        navigation["structuredContent"]["routing"],
        "tree_sitter_fallback"
    );
    assert!(
        navigation["structuredContent"]["syntax_context"]["body"]["content"]
            .as_str()
            .unwrap()
            .contains("pub fn run")
    );
}

#[tokio::test]
async fn positive_harness_tools_flow_through_mcp() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(
        dir.path().join("Cargo.toml"),
        "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("AGENTS.md"),
        "# Guidance\nRead tests before editing.\n",
    )
    .unwrap();
    fs::create_dir_all(dir.path().join("src")).unwrap();
    fs::write(
        dir.path().join("src/large_module.rs"),
        "// fixture\n".repeat(2_001),
    )
    .unwrap();
    let workspaces = Workspaces::new([dir.path()], false, false).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let state = AppState {
        auth: Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned())),
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
        monitor: TaskMonitor::new([workspace_id.clone()]),
        tasks: TaskRuntime::default(),
    };

    let context = call_tool(&state, json!({"name": "project_context", "arguments": {}}))
        .await
        .unwrap();
    assert_eq!(context["isError"], false);
    assert_eq!(context["structuredContent"]["workspace"], workspace_id);
    assert!(context["structuredContent"]["project_types"]
        .as_array()
        .unwrap()
        .iter()
        .any(|kind| kind == "rust"));
    assert_eq!(
        context["structuredContent"]["guidance"][0]["path"],
        "AGENTS.md"
    );
    assert!(context["structuredContent"]["conventions"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "oversized-source-module"));

    let agent = call_tool(
        &state,
        json!({
            "name": "agent_context",
            "arguments": {"query": "large module", "budget": 1000}
        }),
    )
    .await
    .unwrap();
    assert_eq!(agent["isError"], false);
    assert!(agent["structuredContent"].get("estimated_tokens").is_none());
    assert!(agent["structuredContent"].get("timing").is_none());
    assert!(agent["structuredContent"]
        .get("context_bytes_avoided")
        .is_none());
    let telemetry = &agent["_meta"]["dev.wcode/agentContextTelemetry"];
    assert!(telemetry["model_estimated_tokens"]
        .as_u64()
        .is_some_and(|tokens| tokens <= 1_000));
    assert!(telemetry["context_bytes_avoided"].as_u64().unwrap() > 0);
    assert!(telemetry["timing"].is_object());
    assert!(agent["content"][0]["text"]
        .as_str()
        .is_some_and(|text| !text.contains("context_reduction_percent")));
    assert!(agent["structuredContent"]["checks"]
        .as_array()
        .is_some_and(|checks| !checks.is_empty()));

    let convention = call_tool(
        &state,
        json!({"name": "convention_status", "arguments": {}}),
    )
    .await
    .unwrap();
    assert_eq!(convention["isError"], false);
    assert_eq!(
        convention["structuredContent"]["policies"]
            .as_array()
            .unwrap()
            .len(),
        22
    );
    assert!(convention["structuredContent"]["findings"]
        .as_array()
        .unwrap()
        .iter()
        .any(|finding| finding["code"] == "oversized-source-module"));

    let verification = call_tool(
        &state,
        json!({"name": "verify_project", "arguments": {"level": "quick"}}),
    )
    .await
    .unwrap();
    assert_eq!(verification["isError"], true);
    assert!(verification["structuredContent"]["error"]
        .as_str()
        .unwrap()
        .contains("requires command execution"));
}

#[test]
fn parallel_fanout_rejects_oversized_child_results() {
    let response = mcp_tools::tool_result(
        json!({"content": "x".repeat(MAX_PARALLEL_FANOUT_ITEM_BYTES + 1)}),
        false,
    );
    let (item, bytes) =
        parallel_item_from_response("large".to_owned(), "read_file".to_owned(), response);
    assert_eq!(item["ok"], false);
    assert!(item["error"]
        .as_str()
        .unwrap()
        .contains("fan-out item limit"));
    assert!(bytes < MAX_PARALLEL_FANOUT_ITEM_BYTES);
}

#[test]
fn batch_limits_reject_empty_and_oversized_requests() {
    assert_eq!(
        mcp_tools::batch_validation_error(0).unwrap()["error"]["code"],
        -32600
    );
    assert!(mcp_tools::batch_validation_error(1).is_none());
    assert!(mcp_tools::batch_validation_error(MAX_BATCH_ITEMS).is_none());
    assert_eq!(
        mcp_tools::batch_validation_error(MAX_BATCH_ITEMS + 1).unwrap()["error"]["code"],
        -32600
    );
}

#[test]
fn tool_list_exposes_bulk_index_and_positive_harness_tools() {
    let catalog = mcp_tools::tools();
    let names = catalog
        .iter()
        .filter_map(|tool| tool["name"].as_str().map(str::to_owned))
        .collect::<Vec<_>>();
    let context_tool = catalog
        .iter()
        .find(|tool| tool["name"] == "software_context")
        .unwrap();
    let context_scopes = context_tool["_meta"]["dev.wcode/productScopes"]
        .as_array()
        .unwrap();
    for expected_scope in ["design", "graph", "semantics", "traceability", "risk"] {
        assert!(context_scopes.iter().any(|scope| scope == expected_scope));
    }
    let agent_tool = catalog
        .iter()
        .find(|tool| tool["name"] == "agent_context")
        .unwrap();
    let agent_scopes = agent_tool["_meta"]["dev.wcode/productScopes"]
        .as_array()
        .unwrap();
    for expected_scope in [
        "design",
        "graph",
        "traceability",
        "verification",
        "workspace",
    ] {
        assert!(agent_scopes.iter().any(|scope| scope == expected_scope));
    }
    assert!(SERVER_INSTRUCTIONS.contains("call agent_context first"));
    assert!(SERVER_INSTRUCTIONS.len() < 1_000);
    let read_tool = catalog
        .iter()
        .find(|tool| tool["name"] == "read_file")
        .unwrap();
    assert_eq!(
        read_tool["_meta"]["dev.wcode/productScopes"],
        json!(["workspace"])
    );
    for expected in [
        "design_status",
        "convention_status",
        "scope_status",
        "design_init",
        "software_graph",
        "graph_provider_import",
        "graph_provider_status",
        "semantic_provider_status",
        "language_quality_status",
        "language_quality_run",
        "semantic_provider_refresh",
        "semantic_navigation",
        "graph_history",
        "graph_query",
        "graph_diff",
        "traceability_status",
        "drift_status",
        "risk_status",
        "impact_analysis",
        "software_context",
        "agent_context",
        "semantic_status",
        "semantic_query",
        "semantic_record",
        "semantic_confirm",
        "semantic_retire",
        "verification_plan",
        "verification_claim",
        "verification_submit",
        "verification_executor_status",
        "verification_execute_stages",
        "verification_stage_submit",
        "verification_approve",
        "verification_status",
        "verification_history",
        "evidence_status",
        "reconciliation_plan",
        "reconciliation_status",
        "reconciliation_history",
        "reconciliation_execution_status",
        "reconciliation_claim",
        "reconciliation_submit",
        "reconciliation_retry",
        "read_files",
        "path_info",
        "replace_text",
        "apply_edits",
        "write_file",
        "create_directory",
        "create_file",
        "create_files",
        "apply_file_edits",
        "move_path",
        "move_paths",
        "delete_path",
        "search_many",
        "file_outline",
        "find_symbol",
        "symbol_context",
        "project_context",
        "review_changes",
        "verify_project",
        "parallel_tools",
    ] {
        assert!(
            names.contains(&expected.to_owned()),
            "missing MCP tool {expected}"
        );
    }
}
