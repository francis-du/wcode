use super::mcp_tools::{
    optional_string_array_arg, required_string, reviewer_role_arg, run_blocking,
    selected_workspace, string_arg, string_array_arg, task_detail, tool_result, usize_arg,
};
use super::*;
use crate::scopes;

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
        "workspace_info" => {
            let mut info = state.workspaces.capabilities();
            info["harness"] = state.harness.capabilities();
            info["mcp"] = json!({
                "transports": ["streamable-http", "stdio"],
                "modern_protocol": MODERN_PROTOCOL_VERSION,
                "legacy_protocols": LEGACY_PROTOCOL_VERSIONS,
                "capabilities": ["tools", "prompts", "resources", "tasks"],
                "extensions": {
                    (MEDIA_CONTENT_EXTENSION_ID): {
                        "content_types": ["image", "audio"],
                        "tool": "read_media",
                        "per_request_capability": true,
                        "unknown_or_legacy_client": "metadata-only-or-fail-closed",
                        "video": "metadata-only"
                    }
                },
                "local_command": "wcode --workspace <ABSOLUTE_REPOSITORY_PATH> mcp-stdio",
                "agent_plugin_export": "wcode --workspace <REPOSITORY> agent-plugin",
                "remote_auth": "oauth-pkce-resource-bound",
            });
            info["mcp_tasks"] = task_capabilities();
            info["product_scopes"] = json!(scopes::registry());
            info["scope_guidance"] = json!({
                "semantic_scopes": "semantic_query and software_context accept optional scopes; canonical wcode Product Scopes narrow source/semantic context while freeform business scopes remain supported",
                "tool_metadata": "tools/list includes dev.wcode/productScopes in each tool _meta",
            });
            info["scheduling"] = json!({
                "max_parallel": state.harness.max_parallel(),
                "semantics": "global-cap-not-target",
                "fanout_tool": "parallel_tools",
                "bulk_tools": ["read_files", "search_many"],
                "composite_tools": ["review_changes", "verify_project"],
                "guidance": "Use bulk tools for one traversal and parallel_tools for already-known operations. Same-file apply_edits on one SHA are coalesced; a reusable resource dependency graph fans out independent work and orders overlapping read/write, parent/child, move, delete, and directory-creation dependencies."
            });
            Ok(info)
        }
        "convention_status" => convention_status_tool(state, &args).await,
        "scope_status" => {
            let (_workspace_id, workspace) = selected_workspace(state, &args)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .product_scope_status(&workspace)
                    .and_then(|status| serde_json::to_value(status).map_err(Into::into))
            })
            .await
        }
        "design_status" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .design_status(workspace_id, &workspace)
                    .and_then(|status| serde_json::to_value(status).map_err(Into::into))
            })
            .await
        }
        "design_init" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let name = string_arg(&args, "name")
                .unwrap_or(&workspace_id)
                .to_owned();
            let description = string_arg(&args, "description").unwrap_or("").to_owned();
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .design_init(workspace_id, &workspace, &name, &description)
                    .and_then(|status| serde_json::to_value(status).map_err(Into::into))
            })
            .await
        }
        "software_graph" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let path = string_arg(&args, "path").unwrap_or(".").to_owned();
            let max_files = usize_arg(&args, "max_files").unwrap_or(500);
            let max_symbols = usize_arg(&args, "max_symbols").unwrap_or(1_000);
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .software_graph(workspace_id, &workspace, &path, max_files, max_symbols)
                    .and_then(|snapshot| serde_json::to_value(snapshot).map_err(Into::into))
            })
            .await
        }
        "graph_provider_import" => {
            let (_workspace_id, workspace) = selected_workspace(state, &args)?;
            let import = args
                .get("provider_graph")
                .cloned()
                .ok_or_else(|| "missing object argument: provider_graph".to_owned())
                .and_then(|value| {
                    serde_json::from_value::<GraphProviderImport>(value)
                        .map_err(|error| format!("invalid graph provider import: {error}"))
                })?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .graph_provider_import(&workspace, import)
                    .and_then(|stored| serde_json::to_value(stored).map_err(Into::into))
            })
            .await
        }
        "graph_provider_status" => {
            let (_workspace_id, workspace) = selected_workspace(state, &args)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .graph_provider_status(&workspace)
                    .and_then(|providers| serde_json::to_value(providers).map_err(Into::into))
            })
            .await
        }
        "semantic_provider_status" => {
            let (_workspace_id, workspace) = selected_workspace(state, &args)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .semantic_provider_status(&workspace)
                    .and_then(|providers| serde_json::to_value(providers).map_err(Into::into))
            })
            .await
        }
        "semantic_provider_refresh" => {
            let (_workspace_id, workspace) = selected_workspace(state, &args)?;
            let path = string_arg(&args, "path").unwrap_or(".").to_owned();
            let max_files = usize_arg(&args, "max_files").unwrap_or(128);
            let max_symbols = usize_arg(&args, "max_symbols").unwrap_or(1_000);
            state
                .harness
                .semantic_provider_refresh(&workspace, &path, max_files, max_symbols)
                .await
                .and_then(|refresh| serde_json::to_value(refresh).map_err(Into::into))
        }
        "graph_history" => {
            let (_workspace_id, workspace) = selected_workspace(state, &args)?;
            let limit = usize_arg(&args, "limit").unwrap_or(20);
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .graph_history(&workspace, limit)
                    .and_then(|history| serde_json::to_value(history).map_err(Into::into))
            })
            .await
        }
        "graph_query" => {
            let (_workspace_id, workspace) = selected_workspace(state, &args)?;
            let query = args
                .get("query")
                .cloned()
                .ok_or_else(|| "missing object argument: query".to_owned())
                .and_then(|value| {
                    serde_json::from_value::<GraphQueryInput>(value)
                        .map_err(|error| format!("invalid graph query: {error}"))
                })?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .graph_query(&workspace, &query)
                    .and_then(|result| serde_json::to_value(result).map_err(Into::into))
            })
            .await
        }
        "graph_diff" => {
            let (_workspace_id, workspace) = selected_workspace(state, &args)?;
            let diff = args.get("diff").cloned().unwrap_or_else(|| json!({}));
            let diff = serde_json::from_value::<GraphDiffInput>(diff)
                .map_err(|error| format!("invalid graph diff: {error}"))?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .graph_diff(&workspace, &diff)
                    .and_then(|result| serde_json::to_value(result).map_err(Into::into))
            })
            .await
        }
        "traceability_status" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .traceability_status(workspace_id, &workspace)
                    .and_then(|status| serde_json::to_value(status).map_err(Into::into))
            })
            .await
        }
        "software_context" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let query = required_string(&args, "query")?.to_owned();
            let intent = string_arg(&args, "intent").unwrap_or("inspect").to_owned();
            let budget = usize_arg(&args, "budget").unwrap_or(12_000);
            let requested_scopes = optional_string_array_arg(&args, "scopes", 32)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .software_context(
                        workspace_id,
                        &workspace,
                        &query,
                        &intent,
                        budget,
                        &requested_scopes,
                    )
                    .and_then(|context| serde_json::to_value(context).map_err(Into::into))
            })
            .await
        }
        "semantic_status" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let limit = usize_arg(&args, "limit").unwrap_or(50);
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .semantic_status(&workspace_id, &workspace, limit)
                    .and_then(|status| serde_json::to_value(status).map_err(Into::into))
            })
            .await
        }
        "semantic_query" => {
            let (_workspace_id, workspace) = selected_workspace(state, &args)?;
            let query = required_string(&args, "query")?.to_owned();
            let requested_scopes = optional_string_array_arg(&args, "scopes", 32)?;
            let include_candidates = args
                .get("include_candidates")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let limit = usize_arg(&args, "limit").unwrap_or(20);
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .semantic_query(
                        &workspace,
                        &query,
                        &requested_scopes,
                        include_candidates,
                        limit,
                    )
                    .and_then(|matches| serde_json::to_value(matches).map_err(Into::into))
            })
            .await
        }
        "semantic_record" => {
            let (_workspace_id, workspace) = selected_workspace(state, &args)?;
            let input = args
                .get("fact")
                .cloned()
                .ok_or_else(|| "missing object argument: fact".to_owned())
                .and_then(|value| {
                    serde_json::from_value::<SemanticCandidateInput>(value)
                        .map_err(|error| format!("invalid semantic candidate: {error}"))
                })?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .semantic_record_candidate(&workspace, input)
                    .and_then(|fact| serde_json::to_value(fact).map_err(Into::into))
            })
            .await
        }
        "semantic_confirm" => {
            let (_workspace_id, workspace) = selected_workspace(state, &args)?;
            let fact_id = required_string(&args, "fact_id")?.to_owned();
            let attested_by = required_string(&args, "attested_by")?.to_owned();
            let confirmed = args
                .get("confirmed")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !confirmed {
                return Err(
                    "semantic_confirm requires confirmed=true after explicit human confirmation"
                        .to_owned(),
                );
            }
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .semantic_confirm(&workspace, &fact_id, &attested_by)
                    .and_then(|fact| serde_json::to_value(fact).map_err(Into::into))
            })
            .await
        }
        "semantic_retire" => {
            let (_workspace_id, workspace) = selected_workspace(state, &args)?;
            let fact_id = required_string(&args, "fact_id")?.to_owned();
            let attested_by = required_string(&args, "attested_by")?.to_owned();
            let confirmed = args
                .get("confirmed")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !confirmed {
                return Err(
                    "semantic_retire requires confirmed=true after explicit human confirmation"
                        .to_owned(),
                );
            }
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .semantic_retire(&workspace, &fact_id, &attested_by)
                    .and_then(|fact| serde_json::to_value(fact).map_err(Into::into))
            })
            .await
        }
        "evidence_status" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let subject = string_arg(&args, "subject").map(str::to_owned);
            let limit = usize_arg(&args, "limit").unwrap_or(50);
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .evidence_status(&workspace_id, &workspace, subject.as_deref(), limit)
                    .and_then(|status| serde_json::to_value(status).map_err(Into::into))
            })
            .await
        }
        "verification_claim" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let reviewer = required_string(&args, "reviewer")?.to_owned();
            let capabilities = string_array_arg(&args, "capabilities", 32)?;
            let role = reviewer_role_arg(&args)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .verification_claim(&workspace_id, &workspace, &reviewer, &capabilities, role)
                    .and_then(|job| serde_json::to_value(job).map_err(Into::into))
            })
            .await
        }
        "verification_submit" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let job_id = required_string(&args, "job_id")?.to_owned();
            let reviewer = required_string(&args, "reviewer")?.to_owned();
            let submission = args
                .get("submission")
                .cloned()
                .ok_or_else(|| "missing object argument: submission".to_owned())
                .and_then(|value| {
                    serde_json::from_value::<ReviewSubmission>(value)
                        .map_err(|error| format!("invalid verification submission: {error}"))
                })?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .verification_submit(&workspace_id, &workspace, &job_id, &reviewer, submission)
                    .and_then(|job| serde_json::to_value(job).map_err(Into::into))
            })
            .await
        }
        "verification_executor_status" => {
            let (_workspace_id, workspace) = selected_workspace(state, &args)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .verification_executor_status(&workspace)
                    .and_then(|status| serde_json::to_value(status).map_err(Into::into))
            })
            .await
        }
        "language_quality_status" => {
            let (_workspace_id, workspace) = selected_workspace(state, &args)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .language_quality_status(&workspace)
                    .and_then(|status| serde_json::to_value(status).map_err(Into::into))
            })
            .await
        }
        "language_quality_run" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let language = serde_json::from_value::<crate::semantic_provider::SemanticLanguage>(
                Value::String(required_string(&args, "language")?.to_owned()),
            )
            .map_err(|error| format!("invalid language: {error}"))?;
            let provider_id = required_string(&args, "provider_id")?.to_owned();
            let timeout_seconds = usize_arg(&args, "timeout_seconds").unwrap_or(120) as u64;
            state
                .harness
                .language_quality_run(
                    &workspace_id,
                    &workspace,
                    language,
                    &provider_id,
                    timeout_seconds,
                )
                .await
                .and_then(|run| serde_json::to_value(run).map_err(Into::into))
        }
        "verification_execute_stages" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let plan_id = required_string(&args, "plan_id")?.to_owned();
            state
                .harness
                .verification_execute_stages(&workspace_id, &workspace, &plan_id)
                .await
        }
        "verification_stage_submit" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let plan_id = required_string(&args, "plan_id")?.to_owned();
            let submission = args
                .get("submission")
                .cloned()
                .ok_or_else(|| "missing object argument: submission".to_owned())
                .and_then(|value| {
                    serde_json::from_value::<StageSubmission>(value)
                        .map_err(|error| format!("invalid verification stage submission: {error}"))
                })?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .verification_stage_submit(&workspace_id, &workspace, &plan_id, submission)
                    .and_then(|evidence| serde_json::to_value(evidence).map_err(Into::into))
            })
            .await
        }
        "verification_approve" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let plan_id = required_string(&args, "plan_id")?.to_owned();
            let approver = required_string(&args, "approver")?.to_owned();
            let statement = required_string(&args, "statement")?.to_owned();
            let confirmed = args
                .get("confirmed")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if !confirmed {
                return Err(
                    "verification_approve requires confirmed=true from an explicit human approval"
                        .to_owned(),
                );
            }
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .verification_approve(
                        &workspace_id,
                        &workspace,
                        &plan_id,
                        &approver,
                        &statement,
                    )
                    .and_then(|evidence| serde_json::to_value(evidence).map_err(Into::into))
            })
            .await
        }
        "verification_status" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let plan_id = required_string(&args, "plan_id")?.to_owned();
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .verification_status(&workspace_id, &workspace, &plan_id)
                    .and_then(|status| serde_json::to_value(status).map_err(Into::into))
            })
            .await
        }
        "verification_history" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let limit = usize_arg(&args, "limit").unwrap_or(20);
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .verification_history(&workspace_id, &workspace, limit)
                    .and_then(|history| serde_json::to_value(history).map_err(Into::into))
            })
            .await
        }
        "reconciliation_status" => {
            let (_workspace_id, workspace) = selected_workspace(state, &args)?;
            let plan_id = required_string(&args, "plan_id")?.to_owned();
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .reconciliation_status(&workspace, &plan_id)
                    .and_then(|plan| serde_json::to_value(plan).map_err(Into::into))
            })
            .await
        }
        "reconciliation_history" => {
            let (_workspace_id, workspace) = selected_workspace(state, &args)?;
            let limit = usize_arg(&args, "limit").unwrap_or(10).clamp(1, 100);
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .reconciliation_history(&workspace, limit)
                    .and_then(|plans| serde_json::to_value(plans).map_err(Into::into))
            })
            .await
        }
        "reconciliation_execution_status" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let plan_id = required_string(&args, "plan_id")?.to_owned();
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .reconciliation_execution_status(&workspace_id, &workspace, &plan_id)
                    .and_then(|status| serde_json::to_value(status).map_err(Into::into))
            })
            .await
        }
        "reconciliation_claim" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let plan_id = required_string(&args, "plan_id")?.to_owned();
            let executor = required_string(&args, "executor")?.to_owned();
            let kinds = args
                .get("kinds")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .cloned()
                        .map(|value| {
                            serde_json::from_value::<ReconciliationTaskKind>(value).map_err(
                                |error| format!("invalid reconciliation task kind: {error}"),
                            )
                        })
                        .collect::<Result<Vec<_>, _>>()
                })
                .transpose()?
                .unwrap_or_default();
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .reconciliation_claim(&workspace_id, &workspace, &plan_id, &executor, &kinds)
                    .and_then(|run| serde_json::to_value(run).map_err(Into::into))
            })
            .await
        }
        "reconciliation_submit" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let plan_id = required_string(&args, "plan_id")?.to_owned();
            let task_id = required_string(&args, "task_id")?.to_owned();
            let executor = required_string(&args, "executor")?.to_owned();
            let submission = args
                .get("submission")
                .cloned()
                .ok_or_else(|| "missing object argument: submission".to_owned())
                .and_then(|value| {
                    serde_json::from_value::<ReconciliationTaskSubmission>(value)
                        .map_err(|error| format!("invalid reconciliation task submission: {error}"))
                })?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .reconciliation_submit(
                        &workspace_id,
                        &workspace,
                        &plan_id,
                        &task_id,
                        &executor,
                        submission,
                    )
                    .and_then(|run| serde_json::to_value(run).map_err(Into::into))
            })
            .await
        }
        "reconciliation_retry" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let plan_id = required_string(&args, "plan_id")?.to_owned();
            let task_id = required_string(&args, "task_id")?.to_owned();
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .reconciliation_retry(&workspace_id, &workspace, &plan_id, &task_id)
                    .and_then(|run| serde_json::to_value(run).map_err(Into::into))
            })
            .await
        }
        "project_context" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .project_context(workspace_id, &workspace)
                    .and_then(|context| serde_json::to_value(context).map_err(Into::into))
            })
            .await
        }
        "list_files" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let path = string_arg(&args, "path").unwrap_or(".").to_owned();
            let limit = usize_arg(&args, "max_entries").unwrap_or(2_000);
            run_blocking(move || {
                workspace.list_files(&path, limit).map(|files| {
                    json!({"workspace": workspace_id, "files": files, "count": files.len()})
                })
            })
            .await
        }
        "search_code" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let query = required_string(&args, "query")?.to_owned();
            let path = string_arg(&args, "path").unwrap_or(".").to_owned();
            let limit = usize_arg(&args, "max_results").unwrap_or(100);
            run_blocking(move || {
                workspace.search(&query, &path, limit).map(|matches| {
                    json!({"workspace": workspace_id, "matches": matches, "count": matches.len()})
                })
            })
            .await
        }
        "search_many" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let queries = string_array_arg(&args, "queries", 32)?;
            let path = string_arg(&args, "path").unwrap_or(".").to_owned();
            let limit = usize_arg(&args, "max_results").unwrap_or(200);
            run_blocking(move || {
                workspace.search_many(&queries, &path, limit).map(|matches| {
                    json!({"workspace": workspace_id, "matches": matches, "count": matches.len()})
                })
            })
            .await
        }
        "file_outline" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let path = required_string(&args, "path")?.to_owned();
            let max_symbols = usize_arg(&args, "max_symbols").unwrap_or(500);
            let harness = state.harness.clone();
            run_blocking(move || harness.file_outline(workspace_id, &workspace, &path, max_symbols))
                .await
        }
        "find_symbol" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let query = required_string(&args, "query")?.to_owned();
            let path = string_arg(&args, "path").unwrap_or(".").to_owned();
            let kind = string_arg(&args, "kind").map(str::to_owned);
            let max_results = usize_arg(&args, "max_results").unwrap_or(50);
            let harness = state.harness.clone();
            run_blocking(move || {
                harness.find_symbol(
                    workspace_id,
                    &workspace,
                    &query,
                    &path,
                    kind.as_deref(),
                    max_results,
                )
            })
            .await
        }
        "symbol_context" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let symbol_id = required_string(&args, "symbol_id")?.to_owned();
            let max_body_lines = usize_arg(&args, "max_body_lines").unwrap_or(200);
            let harness = state.harness.clone();
            run_blocking(move || {
                harness.symbol_context(workspace_id, &workspace, &symbol_id, max_body_lines)
            })
            .await
        }
        "read_file" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let path = required_string(&args, "path")?.to_owned();
            let start = usize_arg(&args, "start_line").unwrap_or(1);
            let end = usize_arg(&args, "end_line");
            run_blocking(move || {
                workspace.read_file(&path, start, end).and_then(|view| {
                    let mut value = serde_json::to_value(view)?;
                    value["workspace"] = json!(workspace_id);
                    Ok(value)
                })
            })
            .await
        }
        "read_files" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let paths = string_array_arg(&args, "paths", 32)?;
            let start = usize_arg(&args, "start_line").unwrap_or(1);
            let end = usize_arg(&args, "end_line");
            run_blocking(move || {
                workspace.read_files(&paths, start, end).map(|files| {
                    json!({"workspace": workspace_id, "count": files.len(), "files": files})
                })
            })
            .await
        }
        "path_info" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let path = required_string(&args, "path")?.to_owned();
            run_blocking(move || {
                workspace.path_info(&path).and_then(|info| {
                    let mut value = serde_json::to_value(info)?;
                    value["workspace"] = json!(workspace_id);
                    Ok(value)
                })
            })
            .await
        }
        "replace_text" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let path = required_string(&args, "path")?.to_owned();
            let old_text = required_string(&args, "old_text")?.to_owned();
            let new_text = required_string(&args, "new_text")?.to_owned();
            let expected = required_string(&args, "expected_sha256")?.to_owned();
            let start_line = usize_arg(&args, "start_line");
            let end_line = usize_arg(&args, "end_line");
            let harness = state.harness.clone();
            run_blocking(move || {
                workspace
                    .apply_edits(
                        &path,
                        &[TextEdit {
                            old_text,
                            new_text,
                            start_line,
                            end_line,
                        }],
                        &expected,
                    )
                    .and_then(|result| {
                        harness.invalidate_code_file(&workspace, &result.path);
                        let mut value = serde_json::to_value(result)?;
                        value["workspace"] = json!(workspace_id);
                        Ok(value)
                    })
            })
            .await
        }
        "apply_edits" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let path = required_string(&args, "path")?.to_owned();
            let expected = required_string(&args, "expected_sha256")?.to_owned();
            let edits: Vec<TextEdit> =
                serde_json::from_value(args.get("edits").cloned().unwrap_or(Value::Null))
                    .map_err(|error| error.to_string())?;
            let harness = state.harness.clone();
            run_blocking(move || {
                workspace
                    .apply_edits(&path, &edits, &expected)
                    .and_then(|result| {
                        harness.invalidate_code_file(&workspace, &result.path);
                        let mut value = serde_json::to_value(result)?;
                        value["workspace"] = json!(workspace_id);
                        Ok(value)
                    })
            })
            .await
        }
        "write_file" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let path = required_string(&args, "path")?.to_owned();
            let content = required_string(&args, "content")?.to_owned();
            let expected = string_arg(&args, "expected_sha256").map(str::to_owned);
            let harness = state.harness.clone();
            run_blocking(move || {
                workspace
                    .write_file(&path, &content, expected.as_deref())
                    .and_then(|result| {
                        harness.invalidate_code_file(&workspace, &result.path);
                        let mut value = serde_json::to_value(result)?;
                        value["workspace"] = json!(workspace_id);
                        Ok(value)
                    })
            })
            .await
        }
        "create_directory" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let path = required_string(&args, "path")?.to_owned();
            run_blocking(move || {
                workspace.create_directory(&path).and_then(|result| {
                    let mut value = serde_json::to_value(result)?;
                    value["workspace"] = json!(workspace_id);
                    Ok(value)
                })
            })
            .await
        }
        "create_file" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let path = required_string(&args, "path")?.to_owned();
            let content = required_string(&args, "content")?.to_owned();
            let harness = state.harness.clone();
            run_blocking(move || {
                workspace.create_file(&path, &content).and_then(|result| {
                    harness.invalidate_code_file(&workspace, &result.path);
                    let mut value = serde_json::to_value(result)?;
                    value["workspace"] = json!(workspace_id);
                    Ok(value)
                })
            })
            .await
        }
        "create_files" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let files: Vec<CreateFileRequest> =
                serde_json::from_value(args.get("files").cloned().unwrap_or(Value::Null))
                    .map_err(|error| error.to_string())?;
            let harness = state.harness.clone();
            run_blocking(move || {
                let results = workspace.create_files(&files)?;
                for item in &results {
                    if item.ok {
                        harness.invalidate_code_file(&workspace, &item.path);
                    }
                }
                Ok(json!({"workspace":workspace_id,"count":results.len(),"results":results}))
            })
            .await
        }
        "apply_file_edits" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let files: Vec<FileEditRequest> =
                serde_json::from_value(args.get("files").cloned().unwrap_or(Value::Null))
                    .map_err(|error| error.to_string())?;
            let harness = state.harness.clone();
            run_blocking(move || {
                let results = workspace.apply_file_edits(&files)?;
                for item in &results {
                    if item.ok {
                        harness.invalidate_code_file(&workspace, &item.path);
                    }
                }
                Ok(json!({"workspace":workspace_id,"count":results.len(),"results":results}))
            })
            .await
        }
        "move_path" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let source = required_string(&args, "source")?.to_owned();
            let destination = required_string(&args, "destination")?.to_owned();
            let expected = string_arg(&args, "expected_source_sha256").map(str::to_owned);
            let harness = state.harness.clone();
            run_blocking(move || {
                workspace
                    .move_path_checked(&source, &destination, expected.as_deref())
                    .and_then(|result| {
                        harness.invalidate_code_prefix(&workspace, &result.source);
                        harness.invalidate_code_prefix(&workspace, &result.destination);
                        let mut value = serde_json::to_value(result)?;
                        value["workspace"] = json!(workspace_id);
                        Ok(value)
                    })
            })
            .await
        }
        "move_paths" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let moves: Vec<MovePathRequest> =
                serde_json::from_value(args.get("moves").cloned().unwrap_or(Value::Null))
                    .map_err(|error| error.to_string())?;
            let harness = state.harness.clone();
            run_blocking(move || {
                let results = workspace.move_paths(&moves)?;
                for item in &results {
                    if item.ok {
                        harness.invalidate_code_prefix(&workspace, &item.source);
                        harness.invalidate_code_prefix(&workspace, &item.destination);
                    }
                }
                Ok(json!({"workspace":workspace_id,"count":results.len(),"results":results}))
            })
            .await
        }
        "delete_path" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let path = required_string(&args, "path")?.to_owned();
            let expected = string_arg(&args, "expected_sha256").map(str::to_owned);
            let harness = state.harness.clone();
            run_blocking(move || {
                workspace
                    .delete_path(&path, expected.as_deref())
                    .and_then(|result| {
                        harness.invalidate_code_prefix(&workspace, &result.path);
                        let mut value = serde_json::to_value(result)?;
                        value["workspace"] = json!(workspace_id);
                        Ok(value)
                    })
            })
            .await
        }
        "run_command" => {
            let (workspace_id, workspace) = selected_workspace(state, &args)?;
            let program = required_string(&args, "program")?.to_owned();
            let command_args = args
                .get("args")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            let cwd = string_arg(&args, "cwd").unwrap_or(".").to_owned();
            let timeout_seconds = args
                .get("timeout_seconds")
                .and_then(Value::as_u64)
                .unwrap_or(120);
            workspace
                .run_command(&program, &command_args, &cwd, timeout_seconds)
                .await
                .and_then(|result| {
                    let mut value = serde_json::to_value(result)?;
                    value["workspace"] = json!(workspace_id);
                    Ok(value)
                })
        }
        _ => return Err(format!("unknown tool: {name}")),
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
        Ok(value) => serialized_size(value) as u64,
        Err(error) => error.to_string().len() as u64,
    };
    let context_bytes_avoided = outcome
        .as_ref()
        .ok()
        .map(|value| estimated_context_bytes_avoided(name, value, response_bytes))
        .unwrap_or(0);
    task.finish_with_context_savings(success, response_bytes, context_bytes_avoided);
    match outcome {
        Ok(value) => Ok(tool_result(value, false)),
        Err(error) => Ok(tool_result(json!({"error": error.to_string()}), true)),
    }
}

pub(super) fn estimated_context_bytes_avoided(
    name: &str,
    value: &Value,
    response_bytes: u64,
) -> u64 {
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
mod media_capability_tests {
    use super::*;

    #[test]
    fn media_content_is_fail_closed_without_explicit_client_extension() {
        assert!(!client_supports_media_content(
            &json!({"name":"read_media","arguments":{}}),
            "image",
            "image/png"
        ));
    }

    #[test]
    fn media_content_requires_matching_kind_and_optional_mime_filter() {
        let params = json!({
            "_meta": {
                "io.modelcontextprotocol/clientCapabilities": {
                    "extensions": {
                        (MEDIA_CONTENT_EXTENSION_ID): {
                            "contentTypes": ["image"],
                            "mimeTypes": ["image/png"]
                        }
                    }
                }
            }
        });
        assert!(client_supports_media_content(&params, "image", "image/png"));
        assert!(!client_supports_media_content(
            &params,
            "audio",
            "audio/mpeg"
        ));
        assert!(!client_supports_media_content(
            &params,
            "image",
            "image/jpeg"
        ));
    }
}
