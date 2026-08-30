use super::mcp_tools::{
    agent_context_model_bytes, agent_context_tool_result, optional_string_array_arg,
    required_string, reviewer_role_arg, run_blocking, selected_workspace, string_arg,
    string_array_arg, task_detail, tool_result, usize_arg,
};
use super::*;
use crate::scopes;

#[path = "intelligence.rs"]
mod leaf_intelligence;
#[path = "workspace.rs"]
mod leaf_workspace;

pub(crate) async fn call_tool(state: &AppState, params: Value) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or("missing tool name")?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if name == "read_media" {
        return read_media_tool(state, &params).await;
    }
    if matches!(
        name,
        "review_changes"
            | "verify_project"
            | "drift_status"
            | "risk_status"
            | "impact_analysis"
            | "verification_plan"
            | "reconciliation_plan"
            | "parallel_tools"
    ) {
        return call_orchestration_tool(state, name, &args).await;
    }
    call_leaf_tool(state, name, args).await
}

async fn read_media_tool(state: &AppState, params: &Value) -> Result<Value, String> {
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    let workspace_label = string_arg(&args, "workspace")
        .unwrap_or(state.workspaces.default_id())
        .to_owned();
    let request_bytes = serialized_size(&args) as u64;
    let mut task = state.monitor.queue(
        workspace_label,
        "read_media",
        task_detail("read_media", &args),
        request_bytes,
    );
    let _permit = state.harness.acquire().await?;
    task.start();

    let (workspace_id, workspace) = match selected_workspace(state, &args) {
        Ok(selected) => selected,
        Err(error) => {
            task.finish(false, error.len() as u64);
            return Ok(tool_result(json!({"error": error}), true));
        }
    };
    let path = match required_string(&args, "path") {
        Ok(path) => path.to_owned(),
        Err(error) => {
            task.finish(false, error.len() as u64);
            return Ok(tool_result(json!({"error": error}), true));
        }
    };
    let include_content = args
        .get("include_content")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let read = tokio::task::spawn_blocking(move || workspace.read_media(&path))
        .await
        .map_err(|error| format!("blocking media read failed: {error}"))?;
    let view = match read {
        Ok(view) => view,
        Err(error) => {
            let response = tool_result(json!({"error": error.to_string()}), true);
            task.finish(false, serialized_size(&response) as u64);
            return Ok(response);
        }
    };

    let mut metadata = view.metadata();
    metadata["workspace"] = json!(workspace_id);
    metadata["content_available"] = json!(matches!(view.kind, "image" | "audio"));
    metadata["content_requested"] = json!(include_content);
    metadata["content_returned"] = json!(false);

    if !include_content {
        let response = tool_result(metadata, false);
        task.finish(true, serialized_size(&response) as u64);
        return Ok(response);
    }
    if !matches!(view.kind, "image" | "audio") {
        metadata["error_code"] = json!("media_content_type_not_supported");
        metadata["error"] = json!(
            "MCP tool results do not expose a standard video content block; video is metadata-only"
        );
        let response = tool_result(metadata, true);
        task.finish(false, serialized_size(&response) as u64);
        return Ok(response);
    }
    if !client_supports_media_content(params, view.kind, view.mime_type) {
        metadata["error_code"] = json!("multimodal_not_supported");
        metadata["required_client_extension"] = json!(MEDIA_CONTENT_EXTENSION_ID);
        metadata["error"] = json!(
            "client/model media capability was not explicitly advertised; wcode did not emit a multimodal payload"
        );
        let response = tool_result(metadata, true);
        task.finish(false, serialized_size(&response) as u64);
        return Ok(response);
    }

    metadata["content_returned"] = json!(true);
    let encoded = STANDARD.encode(&view.data);
    let text = serde_json::to_string(&metadata).unwrap_or_else(|_| "{}".to_owned());
    let response = json!({
        "content": [
            {"type": "text", "text": text},
            {"type": view.kind, "data": encoded, "mimeType": view.mime_type}
        ],
        "structuredContent": metadata,
        "isError": false,
    });
    task.finish(true, serialized_size(&response) as u64);
    Ok(response)
}

fn client_supports_media_content(params: &Value, kind: &str, mime_type: &str) -> bool {
    let Some(extension) = params
        .get("_meta")
        .and_then(Value::as_object)
        .and_then(|meta| meta.get("io.modelcontextprotocol/clientCapabilities"))
        .and_then(Value::as_object)
        .and_then(|capabilities| capabilities.get("extensions"))
        .and_then(Value::as_object)
        .and_then(|extensions| extensions.get(MEDIA_CONTENT_EXTENSION_ID))
        .and_then(Value::as_object)
    else {
        return false;
    };

    let kind_supported = extension
        .get("contentTypes")
        .and_then(Value::as_array)
        .is_some_and(|types| types.iter().any(|value| value.as_str() == Some(kind)));
    if !kind_supported {
        return false;
    }
    extension
        .get("mimeTypes")
        .and_then(Value::as_array)
        .is_none_or(|types| types.iter().any(|value| value.as_str() == Some(mime_type)))
}

async fn call_orchestration_tool(
    state: &AppState,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    let workspace_label = string_arg(args, "workspace")
        .unwrap_or(state.workspaces.default_id())
        .to_owned();
    let request_bytes = serialized_size(args) as u64;
    let mut task = state.monitor.queue_orchestration(
        workspace_label,
        name,
        task_detail(name, args),
        request_bytes,
    );
    task.start();
    let outcome = match name {
        "review_changes" => review_changes_tool(state, args).await,
        "verify_project" => verify_project_tool(state, args).await,
        "drift_status"
        | "risk_status"
        | "impact_analysis"
        | "verification_plan"
        | "reconciliation_plan" => change_intelligence_tool(state, name, args).await,
        "parallel_tools" => parallel_tools(state, args).await,
        _ => Err(format!("unknown orchestration tool: {name}")),
    };
    let success = outcome
        .as_ref()
        .map(|value| value["isError"].as_bool() != Some(true))
        .unwrap_or(false);
    let response_bytes = outcome
        .as_ref()
        .map(|value| serialized_size(value) as u64)
        .unwrap_or_else(|error| error.len() as u64);
    task.finish(success, response_bytes);
    outcome
}

async fn convention_status_tool(state: &AppState, args: &Value) -> AnyResult<Value> {
    let (_workspace_id, workspace) = selected_workspace(state, args).map_err(anyhow::Error::msg)?;
    let harness = state.harness.clone();
    run_blocking(move || {
        harness
            .convention_status(&workspace)
            .and_then(|status| serde_json::to_value(status).map_err(Into::into))
    })
    .await
}

pub(super) async fn call_leaf_tool(
    state: &AppState,
    name: &str,
    args: Value,
) -> Result<Value, String> {
    let workspace_label = if name == "workspace_info" {
        "system".to_owned()
    } else {
        string_arg(&args, "workspace")
            .unwrap_or(state.workspaces.default_id())
            .to_owned()
    };
    let request_bytes = serialized_size(&args) as u64;
    let detail = task_detail(name, &args);
    let mut task = state
        .monitor
        .queue(workspace_label.clone(), name, detail, request_bytes);
    let _permit = state.harness.acquire().await?;
    task.start();

    let outcome: AnyResult<Value> = match name {
        name if leaf_intelligence::handles(name) => {
            leaf_intelligence::call(state, name, &args).await?
        }
        _ => leaf_workspace::call(state, name, &args).await?,
    };

    if let Ok(value) = &outcome {
        state
            .monitor
            .record_intelligence_result(&workspace_label, name, value);
        if name == "software_graph" {
            if let Ok((workspace_id, workspace)) = selected_workspace(state, &args) {
                if let Ok(diff) = state.harness.graph_diff(
                    &workspace,
                    &GraphDiffInput {
                        from_snapshot_id: None,
                        to_snapshot_id: None,
                        limit: 1,
                    },
                ) {
                    if let Ok(diff) = serde_json::to_value(diff) {
                        state.monitor.record_intelligence_result(
                            &workspace_id,
                            "graph_diff",
                            &diff,
                        );
                    }
                }
            }
        }
    }
    let success = outcome.is_ok();
    let response_bytes = match &outcome {
        Ok(value) if name == "agent_context" => agent_context_model_bytes(value),
        Ok(value) => serialized_size(value) as u64,
        Err(error) => error.to_string().len() as u64,
    };
    let context_bytes_avoided = outcome
        .as_ref()
        .ok()
        .map(|value| estimated_context_bytes_avoided(name, value, response_bytes))
        .unwrap_or(0);
    if name == "agent_context" {
        if let Ok(value) = &outcome {
            let repo_map_cache_hit = value
                .pointer("/repo_map/cache_hit")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            state.monitor.record_agent_context_metrics(
                &workspace_label,
                response_bytes,
                context_bytes_avoided,
                repo_map_cache_hit,
            );
        }
    }
    task.finish_with_context_savings(success, response_bytes, context_bytes_avoided);
    match outcome {
        Ok(value) if name == "agent_context" => Ok(agent_context_tool_result(value, false)),
        Ok(value) => Ok(tool_result(value, false)),
        Err(error) => Ok(tool_result(json!({"error": error.to_string()}), true)),
    }
}

fn merge_agent_worktree_status(context: &mut Value, snapshot: &Value) {
    if snapshot.get("available").and_then(Value::as_bool) != Some(true) {
        return;
    }
    let target_paths = context
        .get("targets")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|target| target.get("path").and_then(Value::as_str))
        .collect::<BTreeSet<_>>();
    if target_paths.is_empty() {
        return;
    }
    let changed = snapshot
        .get("files")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|file| {
            file.get("path")
                .and_then(Value::as_str)
                .map(|path| (path, file))
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut targets = Vec::new();
    let mut has_existing_changes = false;
    let mut has_conflict = false;
    for path in target_paths {
        if let Some(file) = changed.get(path) {
            let status = file
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("modified");
            has_existing_changes = true;
            has_conflict |= status == "unmerged";
            targets.push(json!({
                "path": path,
                "status": status,
                "staged": file.get("staged").cloned().unwrap_or(Value::Bool(false)),
                "unstaged": file.get("unstaged").cloned().unwrap_or(Value::Bool(false)),
                "untracked": file.get("untracked").cloned().unwrap_or(Value::Bool(false)),
            }));
        } else {
            targets.push(json!({"path": path, "status": "clean"}));
        }
    }
    context["worktree"] = json!({
        "targets": targets,
        "has_existing_changes": has_existing_changes,
        "truncated": snapshot.get("truncated").cloned().unwrap_or(Value::Bool(false)),
    });
    let Some(readiness) = context.get_mut("readiness").and_then(Value::as_object_mut) else {
        return;
    };
    let advisories = readiness
        .entry("advisories".to_owned())
        .or_insert_with(|| json!([]));
    if let Some(advisories) = advisories.as_array_mut() {
        if has_existing_changes
            && !advisories
                .iter()
                .any(|advisory| advisory == "target_has_worktree_changes")
        {
            advisories.push(json!("target_has_worktree_changes"));
        }
    }
    if has_conflict {
        readiness.insert("edit".to_owned(), json!("worktree_conflict"));
        readiness.insert("next_actions".to_owned(), json!(["review_changes"]));
    }
}

pub(super) fn estimated_context_bytes_avoided(
    name: &str,
    value: &Value,
    response_bytes: u64,
) -> u64 {
    if name == "agent_context" {
        if let Some(baseline) = value.get("baseline_context_bytes").and_then(Value::as_u64) {
            return baseline.saturating_sub(agent_context_model_bytes(value));
        }
        return value
            .get("context_bytes_avoided")
            .and_then(Value::as_u64)
            .unwrap_or(0);
    }
    if !matches!(name, "file_outline" | "symbol_context") {
        return 0;
    }
    value
        .get("source_bytes")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .saturating_sub(response_bytes)
}

async fn review_changes_tool(state: &AppState, args: &Value) -> Result<Value, String> {
    let (workspace_id, workspace) = selected_workspace(state, args)?;
    let timeout_seconds = args
        .get("timeout_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(30);
    match state
        .harness
        .review_changes(workspace_id, &workspace, timeout_seconds, &state.monitor)
        .await
    {
        Ok(report) => serde_json::to_value(report)
            .map(|value| tool_result(value, false))
            .map_err(|error| error.to_string()),
        Err(error) => Ok(tool_result(json!({"error": error.to_string()}), true)),
    }
}

async fn verify_project_tool(state: &AppState, args: &Value) -> Result<Value, String> {
    let (workspace_id, workspace) = selected_workspace(state, args)?;
    let level = string_arg(args, "level").unwrap_or("quick").to_owned();
    let timeout_seconds = args
        .get("timeout_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(120);
    match state
        .harness
        .verify_project(
            workspace_id,
            &workspace,
            &level,
            timeout_seconds,
            &state.monitor,
        )
        .await
    {
        Ok(report) => serde_json::to_value(report)
            .map(|value| tool_result(value, false))
            .map_err(|error| error.to_string()),
        Err(error) => Ok(tool_result(json!({"error": error.to_string()}), true)),
    }
}

async fn change_intelligence_tool(
    state: &AppState,
    name: &str,
    args: &Value,
) -> Result<Value, String> {
    let (workspace_id, workspace) = selected_workspace(state, args)?;
    let timeout_seconds = args
        .get("timeout_seconds")
        .and_then(Value::as_u64)
        .unwrap_or(30)
        .clamp(1, 120);
    let review = match state
        .harness
        .review_changes(
            workspace_id.clone(),
            &workspace,
            timeout_seconds,
            &state.monitor,
        )
        .await
    {
        Ok(review) => review,
        Err(error) => return Ok(tool_result(json!({"error": error.to_string()}), true)),
    };

    let result: AnyResult<Value> = match name {
        "drift_status" => state
            .harness
            .drift_status(workspace_id.clone(), &workspace, &review)
            .and_then(|status| serde_json::to_value(status).map_err(Into::into)),
        "risk_status" => state
            .harness
            .risk_status(workspace_id.clone(), &workspace, &review)
            .and_then(|status| serde_json::to_value(status).map_err(Into::into)),
        "impact_analysis" => state
            .harness
            .impact_analysis(workspace_id.clone(), &workspace, &review)
            .and_then(|impact| serde_json::to_value(impact).map_err(Into::into)),
        "verification_plan" => state
            .harness
            .verification_plan(workspace_id.clone(), &workspace, &review)
            .and_then(|plan| serde_json::to_value(plan).map_err(Into::into)),
        "reconciliation_plan" => state
            .harness
            .reconciliation_plan(workspace_id.clone(), &workspace, &review)
            .and_then(|plan| serde_json::to_value(plan).map_err(Into::into)),
        _ => return Err(format!("unknown change intelligence tool: {name}")),
    };

    Ok(match result {
        Ok(value) => {
            state
                .monitor
                .record_intelligence_result(&workspace_id, name, &value);
            tool_result(value, false)
        }
        Err(error) => tool_result(json!({"error": error.to_string()}), true),
    })
}

fn parallel_tool_allowed(name: &str) -> bool {
    PARALLEL_READ_TOOLS.contains(&name) || PARALLEL_WRITE_TOOLS.contains(&name)
}

async fn parallel_tools(state: &AppState, args: &Value) -> Result<Value, String> {
    let items = args
        .get("tasks")
        .and_then(Value::as_array)
        .ok_or("tasks must be an array")?;
    if !(2..=MAX_PARALLEL_FANOUT_ITEMS).contains(&items.len()) {
        return Err(format!(
            "tasks must contain between 2 and {MAX_PARALLEL_FANOUT_ITEMS} items"
        ));
    }

    let default_workspace = state.workspaces.default_id();
    let (prepared, aliases, skipped) = scheduler::coalesce_apply_edits(default_workspace, items)?;
    let started = std::time::Instant::now();
    let mut results = vec![None; items.len()];
    let mut workloads = Vec::new();

    for (index, item) in prepared.iter().enumerate() {
        if skipped.contains(&index) {
            continue;
        }
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("task-{}", index + 1));
        let Some(name) = item.get("tool").and_then(Value::as_str) else {
            results[index] = Some(parallel_item_error(id, "unknown", "missing tool name"));
            continue;
        };
        if !parallel_tool_allowed(name) {
            results[index] = Some(parallel_item_error(
                id,
                name,
                "parallel_tools only accepts bounded read/discovery tools and workspace file primitives",
            ));
            continue;
        }
        let arguments = item.get("arguments").cloned().unwrap_or_else(|| json!({}));
        if !arguments.is_object() {
            results[index] = Some(parallel_item_error(id, name, "arguments must be an object"));
            continue;
        }
        match scheduler::resource_model(default_workspace, name, &arguments) {
            Ok(resources) => workloads.push((index, resources)),
            Err(error) => results[index] = Some(parallel_item_error(id, name, error)),
        }
    }

    let active = workloads
        .iter()
        .map(|(index, _)| *index)
        .collect::<BTreeSet<_>>();
    let graph = scheduler::dependency_graph(&workloads, items.len());
    let layers = graph.layers(&active)?;
    let dependency_edges = graph.predecessors.iter().map(BTreeSet::len).sum::<usize>();
    let mut fanout_response_bytes = 0usize;

    for layer in &layers {
        let mut handles = Vec::with_capacity(layer.len());
        for index in layer.iter().copied() {
            let item = &prepared[index];
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| format!("task-{}", index + 1));
            let name = item
                .get("tool")
                .and_then(Value::as_str)
                .expect("scheduled tasks were validated before graph construction");
            let arguments = item.get("arguments").cloned().unwrap_or_else(|| json!({}));
            let child_state = state.clone();
            let child_name = name.to_owned();
            let id_for_task = id.clone();
            let name_for_task = child_name.clone();
            let handle = tokio::spawn(async move {
                match call_leaf_tool(&child_state, &name_for_task, arguments).await {
                    Ok(response) => {
                        parallel_item_from_response(id_for_task, name_for_task, response)
                    }
                    Err(error) => {
                        let item = parallel_item_error(id_for_task, name_for_task, error);
                        let bytes = serialized_size(&item);
                        (item, bytes)
                    }
                }
            });
            handles.push((index, id, child_name, handle));
        }

        for (index, id, name, handle) in handles {
            let item = match handle.await {
                Ok((item, item_bytes))
                    if fanout_response_bytes.saturating_add(item_bytes)
                        <= MAX_PARALLEL_FANOUT_RESPONSE_BYTES =>
                {
                    fanout_response_bytes = fanout_response_bytes.saturating_add(item_bytes);
                    item
                }
                Ok((_item, item_bytes)) => parallel_item_error(
                    id.clone(),
                    name.clone(),
                    format!(
                        "parallel fan-out response budget exceeded ({item_bytes}B item, {}B aggregate limit); narrow paths, line ranges, or result limits",
                        MAX_PARALLEL_FANOUT_RESPONSE_BYTES
                    ),
                ),
                Err(error) => parallel_item_error(
                    id.clone(),
                    name.clone(),
                    format!("task join failed: {error}"),
                ),
            };
            results[index] = Some(item.clone());

            if let Some(alias_items) = aliases.get(&index) {
                for (alias_index, alias_id) in alias_items {
                    let mut alias_item = item.clone();
                    alias_item["id"] = json!(alias_id);
                    alias_item["coalesced_with"] = json!(id);
                    alias_item["coalesced"] = json!(true);
                    let alias_bytes = serialized_size(&alias_item);
                    if fanout_response_bytes.saturating_add(alias_bytes)
                        <= MAX_PARALLEL_FANOUT_RESPONSE_BYTES
                    {
                        fanout_response_bytes = fanout_response_bytes.saturating_add(alias_bytes);
                        results[*alias_index] = Some(alias_item);
                    } else {
                        results[*alias_index] = Some(parallel_item_error(
                            alias_id.clone(),
                            "apply_edits",
                            "parallel fan-out response budget exceeded while expanding coalesced result",
                        ));
                    }
                }
            }
        }
    }

    let items = results
        .into_iter()
        .enumerate()
        .map(|(index, item)| {
            item.unwrap_or_else(|| {
                parallel_item_error(
                    format!("task-{}", index + 1),
                    "unknown",
                    "task did not produce a result",
                )
            })
        })
        .collect::<Vec<_>>();
    let succeeded = items
        .iter()
        .filter(|item| item.get("ok").and_then(Value::as_bool) == Some(true))
        .count();
    let failed = items.len().saturating_sub(succeeded);

    Ok(tool_result(
        json!({
            "execution": "parallel-fanout",
            "scheduler": "dependency-graph",
            "dependency_layers": layers.len(),
            "dependency_edges": dependency_edges,
            "max_parallel": state.harness.max_parallel(),
            "tasks": items.len(),
            "succeeded": succeeded,
            "failed": failed,
            "elapsed_ms": started.elapsed().as_millis(),
            "response_bytes": fanout_response_bytes,
            "item_limit_bytes": MAX_PARALLEL_FANOUT_ITEM_BYTES,
            "response_limit_bytes": MAX_PARALLEL_FANOUT_RESPONSE_BYTES,
            "coalesced_same_file_edits": aliases.values().map(Vec::len).sum::<usize>(),
            "items": items,
        }),
        false,
    ))
}

pub(super) fn parallel_item_from_response(
    id: String,
    tool: String,
    response: Value,
) -> (Value, usize) {
    let is_error = response
        .get("isError")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let result = response
        .get("structuredContent")
        .cloned()
        .unwrap_or(Value::Null);
    let result_bytes = serialized_size(&result);
    if result_bytes > MAX_PARALLEL_FANOUT_ITEM_BYTES {
        let item = parallel_item_error(
            id,
            tool,
            format!(
                "child result is {result_bytes}B, above the {}B fan-out item limit; use line bounds or a smaller result limit",
                MAX_PARALLEL_FANOUT_ITEM_BYTES
            ),
        );
        let bytes = serialized_size(&item);
        return (item, bytes);
    }
    let item = json!({
        "id": id,
        "tool": tool,
        "ok": !is_error,
        "result": result,
    });
    let bytes = serialized_size(&item);
    (item, bytes)
}

fn serialized_size(value: &Value) -> usize {
    struct ByteCounter(usize);

    impl std::io::Write for ByteCounter {
        fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
            self.0 = self.0.saturating_add(buffer.len());
            Ok(buffer.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let mut counter = ByteCounter(0);
    if serde_json::to_writer(&mut counter, value).is_ok() {
        counter.0
    } else {
        0
    }
}

fn parallel_item_error(
    id: impl Into<String>,
    tool: impl Into<String>,
    error: impl Into<String>,
) -> Value {
    json!({
        "id": id.into(),
        "tool": tool.into(),
        "ok": false,
        "error": error.into(),
    })
}

#[cfg(test)]
#[path = "../../../../tests/unit/integrations/mcp/dispatch.rs"]
mod tests;
