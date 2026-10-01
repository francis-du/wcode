use super::*;
use std::collections::HashSet;

#[test]
fn internal_structured_results_do_not_duplicate_payload_as_text() {
    let payload = json!({"body": "x".repeat(32 * 1024), "count": 7});
    let external = tool_result(payload.clone(), false);
    let internal = structured_tool_result(payload.clone(), false);

    assert_eq!(internal["structuredContent"], payload);
    assert_eq!(internal["isError"], false);
    assert!(internal.get("content").is_none());
    assert!(
        serde_json::to_vec(&internal).unwrap().len() * 3
            < serde_json::to_vec(&external).unwrap().len() * 2,
        "structured-only child results should avoid roughly one full payload copy"
    );
}

#[tokio::test]
async fn cancelled_blocking_worker_retains_its_real_permit_until_finished() {
    use std::sync::Arc;
    use std::time::Duration;
    let slots = Arc::new(tokio::sync::Semaphore::new(1));
    let permit = Arc::new(slots.clone().acquire_owned().await.unwrap().into());
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let parent = tokio::spawn(BLOCKING_PERMIT.scope(
        permit,
        run_blocking(move || {
            let _ = started_tx.send(());
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(json!({"done": true}))
        }),
    ));
    tokio::time::timeout(Duration::from_secs(3), started_rx)
        .await
        .unwrap()
        .unwrap();
    parent.abort();
    assert!(parent.await.unwrap_err().is_cancelled());
    assert_eq!(
        slots.available_permits(),
        0,
        "running work still owns the slot"
    );
    release_tx.send(()).unwrap();
    let permit = tokio::time::timeout(Duration::from_secs(3), slots.clone().acquire_owned())
        .await
        .unwrap()
        .unwrap();
    drop(permit);
    assert_eq!(slots.available_permits(), 1);
}

#[tokio::test]
async fn cancelled_blocking_worker_remains_visible_until_real_capacity_is_released() {
    use std::sync::Arc;
    use std::time::Duration;

    let monitor = TaskMonitor::new(["fixture".to_owned()]);
    let slots = Arc::new(tokio::sync::Semaphore::new(1));
    let permit = Arc::new(slots.clone().acquire_owned().await.unwrap().into());
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let monitor_for_task = monitor.clone();
    let parent = tokio::spawn(async move {
        let task = monitor_for_task.queue("fixture", "search_code", "fixture", 1);
        task.start();
        let result = BLOCKING_TASK
            .scope(
                task.clone(),
                BLOCKING_PERMIT.scope(
                    permit,
                    run_blocking(move || {
                        let _ = started_tx.send(());
                        release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                        Ok(json!({"done": true}))
                    }),
                ),
            )
            .await;
        task.finish(result.is_ok(), 1);
    });
    tokio::time::timeout(Duration::from_secs(3), started_rx)
        .await
        .unwrap()
        .unwrap();
    parent.abort();
    assert!(parent.await.unwrap_err().is_cancelled());
    assert_eq!(monitor.observatory_activity("fixture")["active"], 1);
    assert_eq!(
        monitor.observatory_activity("fixture")["recent"][0]["status"],
        "running"
    );
    assert_eq!(slots.available_permits(), 0);

    release_tx.send(()).unwrap();
    let recovered = tokio::time::timeout(Duration::from_secs(3), slots.acquire_owned())
        .await
        .unwrap()
        .unwrap();
    drop(recovered);
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    while monitor.observatory_activity("fixture")["active"] != 0
        && tokio::time::Instant::now() < deadline
    {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let activity = monitor.observatory_activity("fixture");
    assert_eq!(activity["active"], 0);
    assert_eq!(activity["recent"][0]["status"], "failed");
}

#[test]
fn cancelled_queued_blocking_worker_never_starts() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::time::Duration;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            let _ = started_tx.send(());
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        });
        started_rx.await.unwrap();
        let slots = Arc::new(tokio::sync::Semaphore::new(1));
        let permit = Arc::new(slots.clone().acquire_owned().await.unwrap().into());
        let executed = Arc::new(AtomicBool::new(false));
        let worker_executed = executed.clone();
        let (queued_tx, queued_rx) = tokio::sync::oneshot::channel();
        let parent = tokio::spawn(BLOCKING_PERMIT.scope(permit, async move {
            let mut work = Box::pin(run_blocking(move || {
                worker_executed.store(true, Ordering::SeqCst);
                Ok(())
            }));
            std::future::poll_fn(|cx| {
                assert!(std::future::Future::poll(work.as_mut(), cx).is_pending());
                std::task::Poll::Ready(())
            })
            .await;
            let _ = queued_tx.send(());
            work.await
        }));
        queued_rx.await.unwrap();
        parent.abort();
        assert!(parent.await.unwrap_err().is_cancelled());
        release_tx.send(()).unwrap();
        blocker.await.unwrap();
        let permit = tokio::time::timeout(Duration::from_secs(3), slots.acquire_owned())
            .await
            .unwrap()
            .unwrap();
        assert!(!executed.load(Ordering::SeqCst));
        drop(permit);
    });
}

#[tokio::test]
async fn panicking_blocking_worker_releases_its_permit() {
    use std::sync::Arc;
    let slots = Arc::new(tokio::sync::Semaphore::new(1));
    let permit = Arc::new(slots.clone().acquire_owned().await.unwrap().into());
    let result: AnyResult<Value> = BLOCKING_PERMIT
        .scope(permit, run_blocking(|| panic!("synthetic blocking panic")))
        .await;
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("blocking task failed"));
    assert_eq!(slots.available_permits(), 1);
}

#[tokio::test]
async fn cancelled_execution_worker_retains_both_admission_permits() {
    use std::sync::Arc;
    use std::time::Duration;
    let harness = crate::harness::ToolHarness::new(2).unwrap();
    let permit = Arc::new(harness.acquire_tool(true).await.unwrap());
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    let parent = tokio::spawn(BLOCKING_PERMIT.scope(
        permit,
        run_blocking(move || {
            let _ = started_tx.send(());
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok(())
        }),
    ));
    tokio::time::timeout(Duration::from_secs(3), started_rx)
        .await
        .unwrap()
        .unwrap();
    parent.abort();
    assert!(parent.await.unwrap_err().is_cancelled());
    assert!(
        tokio::time::timeout(Duration::from_millis(25), harness.acquire_tool(true))
            .await
            .is_err()
    );
    let read = harness.acquire_tool(false).await.unwrap();
    drop(read);
    release_tx.send(()).unwrap();
    let next = tokio::time::timeout(Duration::from_secs(3), harness.acquire_tool(true))
        .await
        .unwrap()
        .unwrap();
    drop(next);
    let first = harness.acquire().await.unwrap();
    let second = harness.acquire().await.unwrap();
    drop((first, second));
}

#[test]
fn model_worker_tools_keep_revision_scope_and_result_guards_discoverable() {
    let catalog = tools();
    for name in ["worklist_claim", "worklist_submit"] {
        let tool = catalog.iter().find(|tool| tool["name"] == name).unwrap();
        assert_eq!(tool["annotations"]["readOnlyHint"], false);
        assert_eq!(tool["annotations"]["destructiveHint"], false);
        assert!(!tool["description"].as_str().unwrap().ends_with('…'));
        let schema = &tool["inputSchema"];
        assert_eq!(schema["properties"]["expected_revision"]["minimum"], 1);
        assert_eq!(
            schema["properties"]["expected_repository_revision"]["required"],
            json!(["code", "design"])
        );
        assert_eq!(
            schema["properties"]["expected_repository_revision"]["additionalProperties"],
            false
        );
        assert_required_fields_exist(schema, name);
        assert!(tool["_meta"]["dev.wcode/preloadRecommended"].is_null());
    }
    let claim = catalog
        .iter()
        .find(|tool| tool["name"] == "worklist_claim")
        .unwrap();
    assert!(claim["inputSchema"]["properties"]["claim_id"].is_object());
    assert!(!claim["inputSchema"]["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x == "claim_id"));
    let submit = catalog
        .iter()
        .find(|tool| tool["name"] == "worklist_submit")
        .unwrap();
    assert!(submit["inputSchema"]["required"]
        .as_array()
        .unwrap()
        .iter()
        .any(|x| x == "claim_id"));
    assert_eq!(
        submit["inputSchema"]["properties"]["outcome"]["enum"],
        json!(["complete", "blocked", "incomplete"])
    );
    assert_eq!(
        submit["inputSchema"]["properties"]["evidence_ids"]["maxItems"],
        32
    );
    let update = catalog
        .iter()
        .find(|tool| tool["name"] == "worklist_update")
        .unwrap();
    assert_eq!(
        update.pointer("/inputSchema/properties/items/items/properties/write_paths/maxItems"),
        Some(&json!(32))
    );
    assert!(crate::scopes::tool_scopes("worklist_claim")
        .contains(&crate::scopes::ProductScope::Runtime));
    assert!(crate::scopes::tool_scopes("worklist_submit")
        .contains(&crate::scopes::ProductScope::Runtime));
}

#[test]
fn review_output_full_details_stay_discoverable_in_model_catalog() {
    let catalog = tools();
    let tool = catalog
        .iter()
        .find(|tool| tool["name"] == "review_changes")
        .unwrap();
    assert_eq!(
        tool["inputSchema"]["properties"]["detail"]["enum"],
        json!(["summary", "full"])
    );
    assert_eq!(
        tool["inputSchema"]["properties"]["detail"]["type"],
        "string"
    );
    assert!(tool["description"]
        .as_str()
        .unwrap()
        .contains("detail=full"));
}

#[test]
fn long_command_timeouts_remain_model_visible_without_search_tuning() {
    let catalog = tools();
    for name in ["run_command", "command_task", "verify_project"] {
        let tool = catalog.iter().find(|tool| tool["name"] == name).unwrap();
        let properties = &tool["inputSchema"]["properties"];
        assert_eq!(properties["timeout_seconds"]["type"], "integer", "{name}");
        assert_eq!(properties["timeout_seconds"]["minimum"], 1);
        assert_eq!(properties["timeout_seconds"]["maximum"], 1800);
        assert!(!tool["inputSchema"]["required"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field == "timeout_seconds"));
        for tuning in ["budget", "max_files", "max_results", "max_symbols"] {
            assert!(properties.get(tuning).is_none());
        }
    }
}

fn assert_required_fields_exist(value: &Value, tool_name: &str) {
    match value {
        Value::Object(object) => {
            if let (Some(properties), Some(required)) = (
                object.get("properties").and_then(Value::as_object),
                object.get("required").and_then(Value::as_array),
            ) {
                for key in required.iter().filter_map(Value::as_str) {
                    assert!(
                        properties.contains_key(key),
                        "tool {tool_name} requires schema field {key} after it was removed from properties"
                    );
                }
            }
            for child in object.values() {
                assert_required_fields_exist(child, tool_name);
            }
        }
        Value::Array(items) => {
            for child in items {
                assert_required_fields_exist(child, tool_name);
            }
        }
        _ => {}
    }
}

fn assert_no_model_tuning_args(value: &Value, tool_name: &str) {
    match value {
        Value::Object(object) => {
            if let Some(properties) = object.get("properties").and_then(Value::as_object) {
                for key in MODEL_HIDDEN_TUNING_ARGS {
                    if *key == "timeout_seconds"
                        && matches!(tool_name, "run_command" | "command_task" | "verify_project")
                    {
                        continue;
                    }
                    assert!(
                        !properties.contains_key(*key),
                        "tool {tool_name} exposes model tuning argument {key}"
                    );
                }
            }
            for child in object.values() {
                assert_no_model_tuning_args(child, tool_name);
            }
        }
        Value::Array(items) => {
            for child in items {
                assert_no_model_tuning_args(child, tool_name);
            }
        }
        _ => {}
    }
}

#[test]
fn core_tool_routing_survives_compact_description_limits() {
    let catalog = tools();
    for name in [
        "agent_context",
        "project_context",
        "parallel_tools",
        "file_outline",
        "find_symbol",
        "symbol_context",
        "verify_project",
        "read_files",
    ] {
        let tool = catalog.iter().find(|tool| tool["name"] == name).unwrap();
        assert!(
            !tool["description"].as_str().unwrap().ends_with('…'),
            "{name} routing was truncated"
        );
    }
    let project = catalog
        .iter()
        .find(|tool| tool["name"] == "project_context")
        .unwrap();
    assert!(project["description"]
        .as_str()
        .unwrap()
        .contains("Not a second mandatory"));
}

#[test]
fn tool_catalog_exposes_model_neutral_agent_hints() {
    let catalog = tools();
    let core = [
        "agent_context",
        "workspace_info",
        "execution_status",
        "worklist_status",
    ];
    for tool in catalog {
        let name = tool["name"].as_str().unwrap();
        if core.contains(&name) {
            assert_eq!(tool["_meta"]["dev.wcode/preloadRecommended"], true);
        } else {
            assert!(tool["_meta"].get("dev.wcode/preloadRecommended").is_none());
        }
    }
}

#[test]
fn tool_catalog_marks_core_and_on_demand_capabilities_without_hiding_tools() {
    let catalog = tools();
    let core = catalog
        .iter()
        .filter(|tool| tool["_meta"]["dev.wcode/preloadRecommended"] == true)
        .count();
    let on_demand = catalog.len().saturating_sub(core);
    for tool in catalog {
        assert!(tool["_meta"]["dev.wcode/productScopes"].is_array());
        assert!(tool["_meta"].get("dev.wcode/actionGroup").is_none());
        if tool["_meta"]["dev.wcode/preloadRecommended"] != true {
            assert!(tool["_meta"].get("dev.wcode/preloadRecommended").is_none());
        }
    }
    let metrics = catalog_metrics();
    assert_eq!(metrics["tool_count"], catalog.len());
    assert_eq!(metrics["core_tool_count"], core);
    assert_eq!(metrics["on_demand_tool_count"], on_demand);
    assert!(metrics["catalog_bytes"].as_u64().unwrap() > 0);
    assert!(metrics["input_schema_bytes"].as_u64().unwrap() > 0);
    assert!(
        metrics["preload_catalog_bytes"].as_u64().unwrap()
            < metrics["catalog_bytes"].as_u64().unwrap()
    );
    assert!(
        metrics["preload_input_schema_bytes"].as_u64().unwrap()
            < metrics["input_schema_bytes"].as_u64().unwrap()
    );
    assert!(
        metrics["preload_catalog_reduction_percent"]
            .as_u64()
            .unwrap()
            >= 50
    );
    assert_eq!(metrics["action_groups"], "task_manifest_only");
    assert_eq!(metrics["dynamic_tool_list"], false);
    assert_eq!(
        metrics["dynamic_tool_list_policy"],
        "task_independent_protocol_catalog"
    );
    assert!(core > 0);
    assert!(
        on_demand > core,
        "specialized capabilities should stay metadata-first"
    );
    assert!(catalog
        .iter()
        .any(|tool| tool["name"] == "execution_status"));
}

#[test]
fn agent_context_capability_telemetry_measures_task_schema_reduction() {
    let telemetry = capability_selection_telemetry(&json!({
        "capabilities": {
            "recommended_tools": crate::harness::default_coding_tools()
        }
    }))
    .unwrap();
    assert_eq!(telemetry["requested_tool_count"], 6);
    assert_eq!(telemetry["resolved_tool_count"], 6);
    assert_eq!(telemetry["catalog_tool_count"], tools().len());
    assert!(telemetry["catalog_reduction_percent"].as_u64().unwrap() >= 50);
    assert!(telemetry["schema_reduction_percent"].as_u64().unwrap() >= 50);
    assert_eq!(telemetry["compacted"], false);
}

#[test]
fn execution_policy_status_is_read_only_and_bounded() {
    let catalog = tools();
    let tool = catalog
        .iter()
        .find(|tool| tool["name"] == "execution_policy_status")
        .expect("execution_policy_status must be exposed");
    assert_eq!(tool["annotations"]["readOnlyHint"], true);
    assert_ne!(tool["annotations"]["destructiveHint"], true);
    let schema = &tool["inputSchema"];
    assert_eq!(schema["required"], json!(["tool_name"]));
    assert!(schema["properties"]["tool_name"].is_object());
    assert!(schema["properties"]["arguments"].is_object());
    let encoded = serde_json::to_vec(tool).unwrap();
    assert!(
        encoded.len() < 2_000,
        "policy status tool must stay compact"
    );
    let text = String::from_utf8(encoded).unwrap();
    assert!(!text.contains("writer_lease"));
    assert!(!text.contains("hook_command"));
}

#[test]
fn tool_catalog_default_workspace_guidance_saves_repeated_discovery_bytes() {
    const PREVIOUS_GUIDANCE: &str = "Omit for the default Workspace.";
    const GUIDANCE: &str = "Omit for default.";
    let compact = serde_json::to_value(tools()).unwrap();
    let mut paired = compact.clone();
    let mut workspace_fields = 0;
    for tool in paired.as_array_mut().unwrap() {
        if let Some(workspace) = tool.pointer_mut("/inputSchema/properties/workspace") {
            assert_eq!(workspace["type"], "string");
            assert_eq!(workspace["description"], GUIDANCE);
            workspace["description"] = json!(PREVIOUS_GUIDANCE);
            workspace_fields += 1;
        }
    }
    let before = serde_json::to_vec(&paired).unwrap().len();
    let after = serde_json::to_vec(&compact).unwrap().len();
    assert_eq!(
        before - after,
        workspace_fields * (PREVIOUS_GUIDANCE.len() - GUIDANCE.len())
    );
    assert!(workspace_fields >= 60);
    // Required arguments, field types, guards, annotations, ordering and the
    // four preload tools remain byte-identical after normalizing this prose.
    for tool in paired.as_array_mut().unwrap() {
        if let Some(workspace) = tool.pointer_mut("/inputSchema/properties/workspace") {
            workspace["description"] = json!(GUIDANCE);
        }
    }
    assert_eq!(paired, compact);
    println!(
        "catalog {} workspace fields: {} -> {} bytes (estimated {} -> {} tokens)",
        workspace_fields,
        before,
        after,
        before.div_ceil(4),
        after.div_ceil(4)
    );
}

#[test]
fn tool_catalog_is_deterministic_compact_and_unique() {
    let first = tools();
    let second = tools();
    assert_eq!(first, second);
    for round in 0..200 {
        let current = tools();
        assert!(
            std::ptr::eq(first.as_ptr(), current.as_ptr()),
            "tool catalog storage was rebuilt in round {round}"
        );
        assert_eq!(first.len(), current.len());
    }

    let bytes = serde_json::to_vec(first).unwrap().len();
    assert!(bytes <= 60_000, "tool catalog is {bytes} bytes");
    assert!(
        first.iter().all(|tool| tool.get("title").is_none()),
        "tool catalog must not repeat display titles mechanically derived from canonical names"
    );
    for compact_name in [
        "software_graph",
        "graph_provider_import",
        "graph_provider_status",
        "semantic_provider_status",
        "semantic_provider_install",
        "semantic_provider_refresh",
        "semantic_navigation",
        "graph_history",
        "graph_query",
        "graph_diff",
    ] {
        let tool = first
            .iter()
            .find(|tool| tool["name"] == compact_name)
            .expect("compact graph/LSP tool");
        assert!(
            tool["description"]
                .as_str()
                .unwrap_or_default()
                .chars()
                .count()
                <= 100,
            "{compact_name} description grew beyond its compact catalog budget"
        );
    }

    let mut names = HashSet::new();
    for tool in first {
        let name = tool["name"].as_str().unwrap();
        assert!(names.insert(name), "duplicate tool name: {name}");
        let description = tool["description"].as_str().unwrap();
        assert!(
            description.chars().count() <= MAX_TOOL_DESCRIPTION_CHARS,
            "tool {name} description is too long"
        );
        if let Some(workspace) = tool["inputSchema"]["properties"].get("workspace") {
            assert_eq!(workspace["description"], "Omit for default.");
        }
        assert!(
            !serde_json::to_string(&tool["inputSchema"])
                .unwrap()
                .contains("\"default\":"),
            "tool {name} must not advertise model-visible default arguments"
        );
        assert_no_model_tuning_args(&tool["inputSchema"], name);
        assert_required_fields_exist(&tool["inputSchema"], name);
    }
    let agent_context = first
        .iter()
        .find(|tool| tool["name"] == "agent_context")
        .unwrap();
    assert!(agent_context["inputSchema"]["properties"]
        .get("budget")
        .is_none());
    let run_command = first
        .iter()
        .find(|tool| tool["name"] == "run_command")
        .unwrap();
    assert!(run_command["inputSchema"]["properties"]["program"]
        .get("maxLength")
        .is_none());
    let launch_env = &run_command["inputSchema"]["properties"]["env"];
    assert_eq!(launch_env["maxProperties"], 5);
    assert!(names.contains("agent_context"));
    assert!(names.contains("semantic_navigation"));
    let semantic_navigation = first
        .iter()
        .find(|tool| tool["name"] == "semantic_navigation")
        .unwrap();
    assert!(
        semantic_navigation["inputSchema"]["properties"]["intent"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .any(|intent| intent == "rename_plan")
    );
    assert!(
        semantic_navigation["inputSchema"]["properties"]["intent"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .any(|intent| intent == "organize_imports_plan")
    );
    assert!(names.contains("verify_project"));
    assert!(names.contains("apply_file_edits"));
    let review = first
        .iter()
        .find(|tool| tool["name"] == "review_changes")
        .unwrap();
    assert!(review["inputSchema"]["properties"]
        .get("adversarial")
        .is_some());
}

#[test]
fn command_lifecycle_routing_survives_compact_catalog() {
    let catalog = tools();
    let find = |name: &str| catalog.iter().find(|tool| tool["name"] == name).unwrap();
    for name in ["run_command", "command_task", "verify_project"] {
        let description = find(name)["description"].as_str().unwrap();
        assert!(
            !description.ends_with('…'),
            "{name} lifecycle was truncated"
        );
        assert!(description.to_ascii_lowercase().contains("poll"));
    }
    let run = find("run_command");
    let run_description = run["description"].as_str().unwrap();
    for required in [
        "600s",
        "1800",
        "task_mode=true",
        "command_task",
        "never rerun",
    ] {
        assert!(
            run_description.contains(required),
            "missing command routing: {required}"
        );
    }
    let task = find("command_task");
    let task_description = task["description"].as_str().unwrap();
    for required in [
        "without Tasks",
        "owner+Workspace",
        "top-level",
        "Terminal != success/Evidence",
    ] {
        assert!(
            task_description.contains(required),
            "missing task boundary: {required}"
        );
    }
    let action_description = task["inputSchema"]["properties"]["action"]["description"]
        .as_str()
        .unwrap();
    assert!(action_description.contains("600s default, max 1800"));
    assert!(action_description.contains("require task_id"));
    let properties = task["inputSchema"]["properties"].as_object().unwrap();
    for identity in ["actor", "owner", "runtime_instance", "token"] {
        assert!(!properties.contains_key(identity));
    }
    let verify_description = find("verify_project")["description"].as_str().unwrap();
    assert!(verify_description.contains("600s/check (max 1800)"));
    assert!(verify_description.contains("Reject stale Evidence"));
    assert!(verify_description.contains("tasks/get"));
    assert!(verify_description.contains("never rerun"));
}
