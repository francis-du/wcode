use super::mcp_tools::{
    acquire_tool_permit, agent_context_structured_result, agent_context_tool_result,
    bounded_usize_arg, bounded_usize_arg_range, optional_bounded_usize_arg,
    optional_string_array_arg, required_string, reviewer_role_arg, run_blocking,
    selected_workspace, string_arg, string_array_arg, structured_tool_result, task_detail,
    tool_result, tool_result_with_text, workspace_arg,
};
use super::*;
use crate::scopes;
mod acceptance;
mod change;
use change::change_intelligence_tool;
mod checkpoint;
mod policy;
mod review;
mod workspace_selection;
use review::review_changes_tool;
#[path = "evidence_export.rs"]
mod evidence_export;
#[path = "intelligence.rs"]
mod leaf_intelligence;
#[path = "journal.rs"]
mod leaf_journal;
#[path = "workspace.rs"]
mod leaf_workspace;
#[path = "media.rs"]
mod media;
#[path = "parallel.rs"]
mod parallel;
#[path = "parallel_output.rs"]
mod parallel_output;
use media::read_media_tool;
use parallel::parallel_tools;
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
            | "change_acceptance"
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
        "change_acceptance" => Ok(match acceptance::call(state, &args).await {
            Ok(value) => tool_result(value, false),
            Err(error) => tool_result(json!({"error":error.to_string()}), true),
        }),
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
    let journal_error = outcome.as_ref().err().map(|error| json!({"error":error}));
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
        outcome.as_ref().ok().or(journal_error.as_ref()),
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
    if name == "workspace_info" {
        if let Some(diagnostic) = state.harness.overload_diagnostic() {
            return Ok(if include_text {
                tool_result(diagnostic, false)
            } else {
                structured_tool_result(diagnostic, false)
            });
        }
    }
    let journal_started = std::time::Instant::now();
    let workspace_label = if name == "workspace_info" {
        "system".to_owned()
    } else {
        string_arg(&args, "workspace")
            .unwrap_or(state.workspaces.default_id())
            .to_owned()
    };
    let request_bytes = serialized_size(&args) as u64;
    let task = mcp_leaf_task::queue_leaf_task(
        &state.monitor,
        workspace_label.clone(),
        name,
        &args,
        request_bytes,
    );
    crate::mcp_tasks::bind_monitor_task(name, &task);
    let permit = if name == "workspace_info" {
        match super::mcp_tools::acquire_status_permit(state).await {
            Ok(permit) => permit,
            Err(diagnostic) => {
                task.finish(true, serialized_size(&diagnostic) as u64);
                return Ok(if include_text {
                    tool_result(diagnostic, false)
                } else {
                    structured_tool_result(diagnostic, false)
                });
            }
        }
    } else {
        acquire_tool_permit(
            state,
            matches!(
                name,
                "run_command"
                    | "language_quality_run"
                    | "semantic_provider_install"
                    | "semantic_provider_refresh"
                    | "semantic_navigation"
                    | "verification_execute_stages",
            ),
        )
        .await?
    };
    task.start();
    let operation = async {
        match name {
            "acceptance_policy" => Ok(policy::call(state, &args).await),
            "evidence_journal_export" => evidence_export::call(state, &args).await,
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
                    Some(&json!({"error":message})),
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

    let handoff_meta = (name == "worklist_claim")
        .then(|| outcome.as_mut().ok().and_then(isolate_handoff_telemetry))
        .flatten();
    let journal_error = outcome
        .as_ref()
        .err()
        .map(|error| json!({"error":error.to_string()}));
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
        outcome.as_ref().ok().or(journal_error.as_ref()),
    )
    .await;
    match outcome {
        Ok(value) => {
            let mut response = if include_text {
                tool_result_with_text(
                    value,
                    !success,
                    serialized_response.unwrap_or_else(|| "{}".to_owned()),
                )
            } else {
                structured_tool_result(value, !success)
            };
            if let Some(meta) = handoff_meta {
                response["_meta"] = meta;
            }
            Ok(response)
        }
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

pub(super) fn isolate_handoff_telemetry(value: &mut Value) -> Option<Value> {
    let context = value.pointer_mut("/handoff/agent_context")?;
    if !context.is_object() {
        return None;
    }
    let mut response = agent_context_structured_result(context.take(), false);
    *context = response["structuredContent"].take();
    Some(response["_meta"].take())
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

pub(crate) fn verification_options(args: &Value) -> Result<(String, bool, u64), String> {
    workspace_arg(args)?;
    if args.get("task_id").is_some()
        || args
            .get("action")
            .is_some_and(|action| action.as_str() != Some("run"))
    {
        return Err("verification lifecycle actions require a top-level tools/call".to_owned());
    }
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
        None => 600,
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
        Ok(report) => checkpoint::verification_tool_result(state, report).await,
        Err(error) => Ok(tool_result(
            parallel_output::verification_error_payload(error.to_string()),
            true,
        )),
    }
}

#[cfg(test)]
#[path = "../../../../tests/unit/integrations/mcp/dispatch.rs"]
mod tests;
