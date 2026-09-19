use super::mcp_tools::{
    acquire_tool_permit, agent_context_structured_result, agent_context_tool_result,
    optional_string_array_arg, required_string, reviewer_role_arg, run_blocking,
    selected_workspace, string_arg, string_array_arg, structured_tool_result, task_detail,
    tool_result, tool_result_with_text, usize_arg, workspace_arg,
};
use super::*;
use crate::scopes;

#[path = "intelligence.rs"]
mod leaf_intelligence;
#[path = "journal.rs"]
mod leaf_journal;
#[path = "workspace.rs"]
mod leaf_workspace;
#[path = "media.rs"]
mod media;
#[path = "parallel_output.rs"]
mod parallel_output;
use media::read_media_tool;
pub(crate) use parallel_output::parallel_item_from_response;
use parallel_output::{parallel_item_error, serialized_size};

pub(crate) async fn call_tool(state: &AppState, mut params: Value) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or("missing tool name")?
        .to_owned();
    if name == "read_media" {
        return read_media_tool(state, &params).await;
    }
    let args = params
        .as_object_mut()
        .and_then(|params| params.remove("arguments"))
        .unwrap_or_else(|| json!({}));
    workspace_arg(&args)?;
    if matches!(
        name.as_str(),
        "review_changes"
            | "verify_project"
            | "drift_status"
            | "risk_status"
            | "impact_analysis"
            | "verification_plan"
            | "reconciliation_plan"
            | "parallel_tools"
    ) {
        return call_orchestration_tool(state, &name, args).await;
    }
    call_leaf_tool(state, &name, args).await
}

async fn call_orchestration_tool(
    state: &AppState,
    name: &str,
    mut args: Value,
) -> Result<Value, String> {
    let journal_started = std::time::Instant::now();
    let journal_args = mcp_writer::sanitized_args(&args);
    let workspace_label = string_arg(&args, "workspace")
        .unwrap_or(state.workspaces.default_id())
        .to_owned();
    let request_bytes = serialized_size(&args) as u64;
    let task = state.monitor.queue_orchestration(
        workspace_label,
        name,
        task_detail(name, &args),
        request_bytes,
    );
    task.start();
    let outcome = match name {
        "review_changes" => review_changes_tool(state, &args).await,
        "verify_project" => verify_project_tool(state, &args).await,
        "drift_status"
        | "risk_status"
        | "impact_analysis"
        | "verification_plan"
        | "reconciliation_plan" => change_intelligence_tool(state, name, &args).await,
        "parallel_tools" => parallel_tools(state, &mut args).await,
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
    let journal_outcome = outcome
        .as_ref()
        .map(|value| {
            if value.get("isError").and_then(Value::as_bool) == Some(true) {
                "failed"
            } else {
                "succeeded"
            }
        })
        .unwrap_or("failed");
    leaf_journal::record(
        state,
        name,
        &journal_args,
        journal_outcome,
        journal_started
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX),
        outcome.as_ref().ok(),
    )
    .await;
    outcome
}

pub(super) async fn call_leaf_tool(
    state: &AppState,
    name: &str,
    args: Value,
) -> Result<Value, String> {
    call_leaf_tool_mode(state, name, args, true).await
}

async fn call_leaf_tool_structured(
    state: &AppState,
    name: &str,
    args: Value,
) -> Result<Value, String> {
    call_leaf_tool_mode(state, name, args, false).await
}

async fn call_leaf_tool_mode(
    state: &AppState,
    name: &str,
    mut args: Value,
    include_text: bool,
) -> Result<Value, String> {
    let writer_guard = mcp_writer::before_tool(state, name, &mut args)?;
    let journal_started = std::time::Instant::now();
    let workspace_label = if name == "workspace_info" {
        "system".to_owned()
    } else {
        string_arg(&args, "workspace")
            .unwrap_or(state.workspaces.default_id())
            .to_owned()
    };
    let request_bytes = serialized_size(&args) as u64;
    let detail = task_detail(name, &args);
    let task = state
        .monitor
        .queue(workspace_label.clone(), name, detail, request_bytes);
    let permit = acquire_tool_permit(
        state,
        matches!(name, "run_command" | "language_quality_run"),
    )
    .await?;
    task.start();
    let operation = async {
        match name {
            name if leaf_intelligence::handles(name) => {
                leaf_intelligence::call(state, name, &args).await
            }
            _ => leaf_workspace::call(state, name, &args).await,
        }
    };
    let mut outcome: AnyResult<Value> = super::mcp_tools::BLOCKING_TASK
        .scope(
            task.clone(),
            super::mcp_tools::BLOCKING_PERMIT.scope(permit, operation),
        )
        .await?;
    mcp_writer::after_tool(state, writer_guard, &mut outcome);
    let success = match &mut outcome {
        Ok(value) if matches!(name, "run_command" | "language_quality_run") => {
            value.get("success").and_then(Value::as_bool) == Some(true)
        }
        Ok(value) => leaf_workspace::batch_succeeded(name, value),
        Err(_) => false,
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
    if name == "agent_context" {
        match outcome {
            Ok(value) => {
                let response = if include_text {
                    agent_context_tool_result(value, false)
                } else {
                    agent_context_structured_result(value, false)
                };
                let telemetry = &response["_meta"]["dev.wcode/agentContextTelemetry"];
                let response_bytes = telemetry["model_serialized_bytes"]
                    .as_u64()
                    .unwrap_or_else(|| serialized_size(&response["structuredContent"]) as u64);
                let context_bytes_avoided = telemetry["context_bytes_avoided"]
                    .as_u64()
                    .unwrap_or_default();
                let repo_map_cache_hit = telemetry["repo_map"]["cache_hit"]
                    .as_bool()
                    .unwrap_or(false);
                let repo_map_candidates = telemetry["repo_map"]["candidates"]
                    .as_u64()
                    .unwrap_or_default();
                let repo_map_delivered = response["structuredContent"]["repo_map"]["items"]
                    .as_array()
                    .map_or(0, |items| items.len() as u64);
                let model_tokens = telemetry["model_estimated_tokens"]
                    .as_u64()
                    .unwrap_or_else(|| response_bytes.div_ceil(4));
                let budget_tokens = telemetry["budget_tokens"].as_u64().unwrap_or_default();
                let build_ms = telemetry["timing"]["build_ms"].as_u64().unwrap_or_default();
                state.monitor.record_agent_context_metrics(
                    &workspace_label,
                    crate::monitor::AgentContextMetrics {
                        model_bytes: response_bytes,
                        model_tokens,
                        budget_tokens,
                        context_bytes_avoided,
                        repo_map_cache_hit,
                        repo_map_candidates,
                        repo_map_delivered,
                        build_ms,
                    },
                );
                task.finish_with_context_savings(success, response_bytes, context_bytes_avoided);
                let journal_outcome = if response
                    .pointer("/structuredContent/status")
                    .and_then(Value::as_str)
                    == Some("partial")
                {
                    "partial"
                } else if success {
                    "succeeded"
                } else {
                    "failed"
                };
                leaf_journal::record(
                    state,
                    name,
                    &args,
                    journal_outcome,
                    journal_started
                        .elapsed()
                        .as_millis()
                        .try_into()
                        .unwrap_or(u64::MAX),
                    Some(&response),
                )
                .await;
                return Ok(response);
            }
            Err(error) => {
                let blocked = error.downcast_ref::<AuthorizationRequired>().is_some();
                let message = error.to_string();
                task.finish_with_context_savings(false, message.len() as u64, 0);
                leaf_journal::record(
                    state,
                    name,
                    &args,
                    if blocked { "blocked" } else { "failed" },
                    journal_started
                        .elapsed()
                        .as_millis()
                        .try_into()
                        .unwrap_or(u64::MAX),
                    None,
                )
                .await;
                let value = if let Some(required) = error.downcast_ref::<AuthorizationRequired>() {
                    json!({
                        "error": message,
                        "authorization_required": required.request,
                    })
                } else {
                    json!({"error": message})
                };
                return Ok(if include_text {
                    tool_result(value, true)
                } else {
                    structured_tool_result(value, true)
                });
            }
        }
    }

    let serialized_response = match &outcome {
        Ok(value) if include_text => serde_json::to_string(value).ok(),
        _ => None,
    };
    let response_bytes = match &outcome {
        Ok(value) => serialized_response
            .as_ref()
            .map_or_else(|| serialized_size(value) as u64, |text| text.len() as u64),
        Err(error) => error.to_string().len() as u64,
    };
    let context_bytes_avoided = outcome
        .as_ref()
        .ok()
        .map(|value| estimated_context_bytes_avoided(name, value, response_bytes))
        .unwrap_or(0);
    task.finish_with_context_savings(success, response_bytes, context_bytes_avoided);
    let journal_outcome = match &outcome {
        Ok(value) if value.get("status").and_then(Value::as_str) == Some("partial") => "partial",
        Ok(_) if success => "succeeded",
        Err(error) if error.downcast_ref::<AuthorizationRequired>().is_some() => "blocked",
        _ => "failed",
    };
    leaf_journal::record(
        state,
        name,
        &args,
        journal_outcome,
        journal_started
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX),
        outcome.as_ref().ok(),
    )
    .await;
    match outcome {
        Ok(value) => Ok(if include_text {
            tool_result_with_text(
                value,
                !success,
                serialized_response.unwrap_or_else(|| "{}".to_owned()),
            )
        } else {
            structured_tool_result(value, !success)
        }),
        Err(error) => {
            let value = if let Some(required) = error.downcast_ref::<AuthorizationRequired>() {
                json!({
                    "error": error.to_string(),
                    "authorization_required": required.request,
                })
            } else {
                json!({"error": error.to_string()})
            };
            Ok(if include_text {
                tool_result(value, true)
            } else {
                structured_tool_result(value, true)
            })
        }
    }
}

fn agent_context_model_tokens(context: &Value) -> u64 {
    let preview = agent_context_structured_result(context.clone(), false);
    preview
        .pointer("/_meta/dev.wcode/agentContextTelemetry/model_estimated_tokens")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| serialized_size(context).div_ceil(4) as u64)
}

fn enforce_agent_context_postlude_budget(context: &mut Value) {
    let Some(budget) = context
        .get("budget")
        .and_then(Value::as_u64)
        .filter(|budget| *budget > 0)
    else {
        return;
    };
    if agent_context_model_tokens(context) <= budget {
        return;
    }
    if let Some(object) = context.as_object_mut() {
        object.remove("verification_impact");
    }
    if agent_context_model_tokens(context) <= budget {
        return;
    }
    if let Some(object) = context.as_object_mut() {
        object.remove("worktree");
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
            return baseline.saturating_sub(response_bytes);
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
    let adversarial = match args.get("adversarial") {
        None => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err("adversarial must be a boolean when provided".to_owned()),
    };
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
        Ok(report) => {
            let mut value = serde_json::to_value(&report).map_err(|error| error.to_string())?;
            if adversarial {
                let permit = acquire_tool_permit(state, false).await?;
                let harness = state.harness.clone();
                let packet = super::mcp_tools::BLOCKING_PERMIT
                    .scope(
                        permit,
                        run_blocking(move || {
                            Ok(harness.adversarial_review_with_candidates(&workspace, &report))
                        }),
                    )
                    .await
                    .map_err(|error| error.to_string())?;
                value["adversarial"] =
                    serde_json::to_value(packet).map_err(|error| error.to_string())?;
            }
            Ok(tool_result(value, false))
        }
        Err(error) => Ok(tool_result(json!({"error": error.to_string()}), true)),
    }
}

pub(crate) fn verification_options(args: &Value) -> Result<(String, bool, u64), String> {
    workspace_arg(args)?;
    let level = match args.get("level") {
        None => "quick".to_owned(),
        Some(Value::String(level)) if matches!(level.as_str(), "quick" | "full") => level.clone(),
        Some(_) => return Err("level must be quick or full when provided".to_owned()),
    };
    let fail_fast = match args.get("fail_fast") {
        None => true,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err("fail_fast must be a boolean when provided".to_owned()),
    };
    let timeout_seconds = match args.get("timeout_seconds") {
        None => 120,
        Some(value) => value
            .as_u64()
            .filter(|value| (1..=1800).contains(value))
            .ok_or("timeout_seconds must be an integer between 1 and 1800")?,
    };
    Ok((level, fail_fast, timeout_seconds))
}

async fn verify_project_tool(state: &AppState, args: &Value) -> Result<Value, String> {
    let (workspace_id, workspace) = selected_workspace(state, args)?;
    let (level, fail_fast, timeout_seconds) = verification_options(args)?;
    let outcome = if fail_fast {
        state
            .harness
            .verify_project(
                workspace_id,
                &workspace,
                &level,
                timeout_seconds,
                &state.monitor,
            )
            .await
    } else {
        state
            .harness
            .verify_project_mode(
                workspace_id,
                &workspace,
                (&level, false),
                timeout_seconds,
                &state.monitor,
            )
            .await
    };
    match outcome {
        Ok(report) => {
            let is_error = !report.passed;
            serde_json::to_value(report)
                .map(|value| tool_result(value, is_error))
                .map_err(|error| error.to_string())
        }
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

fn inherit_parallel_workspace(arguments: &mut Value, workspace: &str) {
    let Some(object) = arguments.as_object_mut() else {
        return;
    };
    object
        .entry("workspace".to_owned())
        .or_insert_with(|| json!(workspace));
}

async fn parallel_tools(state: &AppState, args: &mut Value) -> Result<Value, String> {
    let owner = mcp_writer::current_owner();
    let dry_run = match args.get("dry_run") {
        None => false,
        Some(Value::Bool(value)) => *value,
        Some(_) => return Err("dry_run must be a boolean; no tasks executed".to_owned()),
    };
    let inherited_workspace = workspace_arg(args)?
        .unwrap_or(state.workspaces.default_id())
        .to_owned();
    let items = args
        .get_mut("tasks")
        .and_then(Value::as_array_mut)
        .ok_or("tasks must be an array")?;
    if !(2..=MAX_PARALLEL_FANOUT_ITEMS).contains(&items.len()) {
        return Err(format!(
            "tasks must contain between 2 and {MAX_PARALLEL_FANOUT_ITEMS} items"
        ));
    }

    for (index, item) in items.iter().enumerate() {
        if let Some(arguments) = item.get("arguments") {
            workspace_arg(arguments).map_err(|error| {
                format!(
                    "parallel task {} failed preflight: {error}; no tasks executed",
                    index + 1
                )
            })?;
        }
    }
    state
        .workspaces
        .select(Some(&inherited_workspace))
        .map_err(|error| error.to_string())?;
    let (prepared, aliases, skipped) =
        scheduler::coalesce_apply_edits(&inherited_workspace, items)?;
    let mut prepared = match prepared {
        std::borrow::Cow::Borrowed(_) => std::mem::take(items),
        std::borrow::Cow::Owned(prepared) => {
            items.clear();
            prepared
        }
    };
    let task_count = prepared.len();
    let started = std::time::Instant::now();
    let mut results = vec![None; task_count];
    let mut workloads = Vec::new();
    let mut execution_arguments = vec![None; task_count];

    for (index, item) in prepared.iter_mut().enumerate() {
        if skipped.contains(&index) {
            continue;
        }
        let id = item
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("task-{}", index + 1));
        let Some(name) = item.get("tool").and_then(Value::as_str).map(str::to_owned) else {
            results[index] = Some(parallel_item_error(id, "unknown", "missing tool name"));
            continue;
        };
        if !parallel_tool_allowed(&name) {
            results[index] = Some(parallel_item_error(
                id,
                &name,
                "parallel_tools only accepts bounded read/discovery tools and workspace file primitives",
            ));
            continue;
        }
        let mut arguments = item
            .get_mut("arguments")
            .map(std::mem::take)
            .unwrap_or_else(|| json!({}));
        if !arguments.is_object() {
            results[index] = Some(parallel_item_error(
                id,
                &name,
                "arguments must be an object",
            ));
            continue;
        }
        inherit_parallel_workspace(&mut arguments, &inherited_workspace);
        match scheduler::resource_model(&inherited_workspace, &name, &arguments).and_then(
            |resources| {
                selected_workspace(state, &arguments)
                    .map(|(_, workspace)| resources.in_root(workspace.root()))
            },
        ) {
            Ok(resources) => {
                execution_arguments[index] = Some(arguments);
                workloads.push((index, resources));
            }
            Err(error) => results[index] = Some(parallel_item_error(id, name, error)),
        }
    }

    // An invalid resource model cannot safely disappear from the dependency
    // graph: a later mutation might otherwise run against stale input. Reject
    // the entire batch before spawning anything when preflight is incomplete.
    if let Some((index, item)) = results
        .iter()
        .enumerate()
        .find_map(|(index, item)| item.as_ref().map(|item| (index, item)))
    {
        return Err(format!(
            "parallel task {} failed preflight: {}; no tasks executed",
            index + 1,
            item["error"].as_str().unwrap_or("invalid task")
        ));
    }

    let active = workloads
        .iter()
        .map(|(index, _)| *index)
        .collect::<BTreeSet<_>>();
    let graph = scheduler::dependency_graph(&workloads, task_count);
    let dependency_edges = graph.predecessors.iter().map(BTreeSet::len).sum::<usize>();
    if dry_run {
        let layers = graph.layers(&active)?;
        let plan = scheduler::preview(
            &graph,
            &layers,
            &workloads,
            &aliases,
            state.harness.max_parallel(),
        );
        return Ok(tool_result(plan, false));
    }
    let mut fanout_response_bytes = 0usize;

    let mut completion_schedule = graph.completion_schedule(&active)?;
    let mut pending = active.len();
    let mut failed_tasks = vec![false; task_count];
    let mut task_depths = vec![0usize; task_count];
    let mut dependency_layers = 0usize;
    // JoinSet owns the children: cancellation must not detach queued writes.
    let mut running = tokio::task::JoinSet::new();
    let mut task_indices = std::collections::HashMap::new();
    let mut task_ids = (0..task_count)
        .map(|_| None)
        .collect::<Vec<Option<tokio::task::Id>>>();
    while pending > 0 || !running.is_empty() {
        for index in completion_schedule.take_ready() {
            pending = pending.saturating_sub(1);
            let dependency_failed = graph.predecessors[index]
                .iter()
                .any(|dependency| failed_tasks[*dependency]);
            let depth = graph.predecessors[index]
                .iter()
                .map(|dependency| task_depths[*dependency])
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            task_depths[index] = depth;
            dependency_layers = dependency_layers.max(depth);
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
            let arguments = execution_arguments[index]
                .take()
                .expect("scheduled task arguments were prepared during preflight");
            let child_state = state.clone();
            let child_owner = owner.clone();
            let child_name = name.to_owned();
            let id_for_task = id.clone();
            let name_for_task = child_name.clone();
            let handle = running.spawn(async move {
                let response = if dependency_failed {
                    Err("dependency failed; task was not executed".to_owned())
                } else {
                    mcp_writer::with_owner(
                        child_owner,
                        call_leaf_tool_structured(&child_state, &name_for_task, arguments),
                    )
                    .await
                };
                let result = match response {
                    Ok(response) => {
                        parallel_item_from_response(id_for_task, name_for_task, response)
                    }
                    Err(error) => {
                        let item = parallel_item_error(id_for_task, name_for_task, error);
                        let bytes = serialized_size(&item);
                        (item, bytes)
                    }
                };
                (index, result)
            });
            let task_id = handle.id();
            task_indices.insert(task_id, index);
            task_ids[index] = Some(task_id);
        }

        // Release a successor as soon as its own predecessors finish, without
        // waiting for an unrelated slow branch in the same topological layer.
        let (index, outcome) = match running.join_next().await {
            Some(Ok((index, result))) => {
                if let Some(task_id) = task_ids[index].take() {
                    task_indices.remove(&task_id);
                }
                (index, Ok(result))
            }
            Some(Err(error)) => {
                let task_id = error.id();
                let index = task_indices
                    .remove(&task_id)
                    .ok_or("parallel task identity was lost")?;
                task_ids[index] = None;
                (index, Err(error.to_string()))
            }
            None => return Err("parallel scheduler made no progress".to_owned()),
        };
        let id = prepared[index]
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| format!("task-{}", index + 1));
        let name = prepared[index]["tool"]
            .as_str()
            .unwrap_or("unknown")
            .to_owned();
        {
            let item = match outcome {
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
            if item["ok"].as_bool() != Some(true) {
                failed_tasks[index] = true;
            }
            completion_schedule.complete(index);
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
            results[index] = Some(item);
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
            "dispatch": "completion-driven",
            "dependency_layers": dependency_layers,
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
        failed > 0,
    ))
}

#[cfg(test)]
#[path = "../../../../tests/unit/integrations/mcp/dispatch.rs"]
mod tests;
