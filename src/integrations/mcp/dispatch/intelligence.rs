use super::*;

pub(super) fn handles(name: &str) -> bool {
    matches!(
        name,
        "workspace_info"
            | "convention_status"
            | "scope_status"
            | "design_status"
            | "design_init"
            | "software_graph"
            | "graph_provider_import"
            | "graph_provider_status"
            | "semantic_provider_status"
            | "semantic_provider_refresh"
            | "graph_history"
            | "graph_query"
            | "graph_diff"
            | "traceability_status"
            | "software_context"
            | "agent_context"
            | "semantic_status"
            | "semantic_query"
            | "semantic_record"
            | "semantic_confirm"
            | "semantic_retire"
            | "evidence_status"
            | "verification_claim"
            | "verification_submit"
            | "verification_executor_status"
            | "language_quality_status"
            | "language_quality_run"
            | "verification_execute_stages"
            | "verification_stage_submit"
            | "verification_approve"
            | "verification_status"
            | "verification_history"
            | "reconciliation_status"
            | "reconciliation_history"
            | "reconciliation_execution_status"
            | "reconciliation_claim"
            | "reconciliation_submit"
            | "reconciliation_retry"
    )
}

pub(super) async fn call(
    state: &AppState,
    name: &str,
    args: &Value,
) -> Result<AnyResult<Value>, String> {
    let outcome: AnyResult<Value> = match name {
        "workspace_info" => {
            let mut info = state.workspaces.capabilities();
            info["harness"] = state.harness.capabilities();
            info["mcp"] = json!({
                "transports": ["stdio", "streamable-http", "legacy-sse"],
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
        "convention_status" => convention_status_tool(state, args).await,
        "scope_status" => {
            let (_workspace_id, workspace) = selected_workspace(state, args)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .product_scope_status(&workspace)
                    .and_then(|status| serde_json::to_value(status).map_err(Into::into))
            })
            .await
        }
        "design_status" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .design_status(workspace_id, &workspace)
                    .and_then(|status| serde_json::to_value(status).map_err(Into::into))
            })
            .await
        }
        "design_init" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let name = string_arg(args, "name").unwrap_or(&workspace_id).to_owned();
            let description = string_arg(args, "description").unwrap_or("").to_owned();
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .design_init(workspace_id, &workspace, &name, &description)
                    .and_then(|status| serde_json::to_value(status).map_err(Into::into))
            })
            .await
        }
        "software_graph" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let path = string_arg(args, "path").unwrap_or(".").to_owned();
            let max_files = usize_arg(args, "max_files").unwrap_or(500);
            let max_symbols = usize_arg(args, "max_symbols").unwrap_or(1_000);
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .software_graph(workspace_id, &workspace, &path, max_files, max_symbols)
                    .and_then(|snapshot| serde_json::to_value(snapshot).map_err(Into::into))
            })
            .await
        }
        "graph_provider_import" => {
            let (_workspace_id, workspace) = selected_workspace(state, args)?;
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
            let (_workspace_id, workspace) = selected_workspace(state, args)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .graph_provider_status(&workspace)
                    .and_then(|providers| serde_json::to_value(providers).map_err(Into::into))
            })
            .await
        }
        "semantic_provider_status" => {
            let (_workspace_id, workspace) = selected_workspace(state, args)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .semantic_provider_status(&workspace)
                    .and_then(|providers| serde_json::to_value(providers).map_err(Into::into))
            })
            .await
        }
        "semantic_provider_refresh" => {
            let (_workspace_id, workspace) = selected_workspace(state, args)?;
            let path = string_arg(args, "path").unwrap_or(".").to_owned();
            let max_files = usize_arg(args, "max_files").unwrap_or(128);
            let max_symbols = usize_arg(args, "max_symbols").unwrap_or(1_000);
            state
                .harness
                .semantic_provider_refresh(&workspace, &path, max_files, max_symbols)
                .await
                .and_then(|refresh| serde_json::to_value(refresh).map_err(Into::into))
        }
        "graph_history" => {
            let (_workspace_id, workspace) = selected_workspace(state, args)?;
            let limit = usize_arg(args, "limit").unwrap_or(20);
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .graph_history(&workspace, limit)
                    .and_then(|history| serde_json::to_value(history).map_err(Into::into))
            })
            .await
        }
        "graph_query" => {
            let (_workspace_id, workspace) = selected_workspace(state, args)?;
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
            let (_workspace_id, workspace) = selected_workspace(state, args)?;
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
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .traceability_status(workspace_id, &workspace)
                    .and_then(|status| serde_json::to_value(status).map_err(Into::into))
            })
            .await
        }
        "software_context" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let query = required_string(args, "query")?.to_owned();
            let intent = string_arg(args, "intent").unwrap_or("inspect").to_owned();
            let budget = usize_arg(args, "budget").unwrap_or(12_000);
            let requested_scopes = optional_string_array_arg(args, "scopes", 32)?;
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
        "agent_context" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let query = required_string(args, "query")?.to_owned();
            let budget = usize_arg(args, "budget").unwrap_or(0);
            let requested_scopes = optional_string_array_arg(args, "scopes", 32)?;
            let context_harness = state.harness.clone();
            let context_workspace = workspace.clone();
            let context_future = run_blocking(move || {
                context_harness.agent_context(
                    workspace_id,
                    &context_workspace,
                    &query,
                    budget,
                    &requested_scopes,
                )
            });
            let worktree_harness = state.harness.clone();
            let worktree_future = worktree_harness.worktree_status_snapshot(&workspace);
            let (context, worktree) = tokio::join!(context_future, worktree_future);
            let mut context = context.map_err(|error| error.to_string())?;
            if let Ok(worktree) = worktree {
                merge_agent_worktree_status(&mut context, &worktree);
            }
            Ok(context)
        }
        "semantic_status" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let limit = usize_arg(args, "limit").unwrap_or(50);
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .semantic_status(&workspace_id, &workspace, limit)
                    .and_then(|status| serde_json::to_value(status).map_err(Into::into))
            })
            .await
        }
        "semantic_query" => {
            let (_workspace_id, workspace) = selected_workspace(state, args)?;
            let query = required_string(args, "query")?.to_owned();
            let requested_scopes = optional_string_array_arg(args, "scopes", 32)?;
            let include_candidates = args
                .get("include_candidates")
                .and_then(Value::as_bool)
                .unwrap_or(true);
            let limit = usize_arg(args, "limit").unwrap_or(20);
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
            let (_workspace_id, workspace) = selected_workspace(state, args)?;
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
            let (_workspace_id, workspace) = selected_workspace(state, args)?;
            let fact_id = required_string(args, "fact_id")?.to_owned();
            let attested_by = required_string(args, "attested_by")?.to_owned();
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
            let (_workspace_id, workspace) = selected_workspace(state, args)?;
            let fact_id = required_string(args, "fact_id")?.to_owned();
            let attested_by = required_string(args, "attested_by")?.to_owned();
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
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let subject = string_arg(args, "subject").map(str::to_owned);
            let limit = usize_arg(args, "limit").unwrap_or(50);
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .evidence_status(&workspace_id, &workspace, subject.as_deref(), limit)
                    .and_then(|status| serde_json::to_value(status).map_err(Into::into))
            })
            .await
        }
        "verification_claim" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let reviewer = required_string(args, "reviewer")?.to_owned();
            let capabilities = string_array_arg(args, "capabilities", 32)?;
            let role = reviewer_role_arg(args)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .verification_claim(&workspace_id, &workspace, &reviewer, &capabilities, role)
                    .and_then(|job| serde_json::to_value(job).map_err(Into::into))
            })
            .await
        }
        "verification_submit" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let job_id = required_string(args, "job_id")?.to_owned();
            let reviewer = required_string(args, "reviewer")?.to_owned();
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
            let (_workspace_id, workspace) = selected_workspace(state, args)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .verification_executor_status(&workspace)
                    .and_then(|status| serde_json::to_value(status).map_err(Into::into))
            })
            .await
        }
        "language_quality_status" => {
            let (_workspace_id, workspace) = selected_workspace(state, args)?;
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .language_quality_status(&workspace)
                    .and_then(|status| serde_json::to_value(status).map_err(Into::into))
            })
            .await
        }
        "language_quality_run" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let language = serde_json::from_value::<crate::semantic_provider::SemanticLanguage>(
                Value::String(required_string(args, "language")?.to_owned()),
            )
            .map_err(|error| format!("invalid language: {error}"))?;
            let provider_id = required_string(args, "provider_id")?.to_owned();
            let timeout_seconds = usize_arg(args, "timeout_seconds").unwrap_or(120) as u64;
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
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let plan_id = required_string(args, "plan_id")?.to_owned();
            state
                .harness
                .verification_execute_stages(&workspace_id, &workspace, &plan_id)
                .await
        }
        "verification_stage_submit" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let plan_id = required_string(args, "plan_id")?.to_owned();
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
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let plan_id = required_string(args, "plan_id")?.to_owned();
            let approver = required_string(args, "approver")?.to_owned();
            let statement = required_string(args, "statement")?.to_owned();
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
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let plan_id = required_string(args, "plan_id")?.to_owned();
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .verification_status(&workspace_id, &workspace, &plan_id)
                    .and_then(|status| serde_json::to_value(status).map_err(Into::into))
            })
            .await
        }
        "verification_history" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let limit = usize_arg(args, "limit").unwrap_or(20);
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .verification_history(&workspace_id, &workspace, limit)
                    .and_then(|history| serde_json::to_value(history).map_err(Into::into))
            })
            .await
        }
        "reconciliation_status" => {
            let (_workspace_id, workspace) = selected_workspace(state, args)?;
            let plan_id = required_string(args, "plan_id")?.to_owned();
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .reconciliation_status(&workspace, &plan_id)
                    .and_then(|plan| serde_json::to_value(plan).map_err(Into::into))
            })
            .await
        }
        "reconciliation_history" => {
            let (_workspace_id, workspace) = selected_workspace(state, args)?;
            let limit = usize_arg(args, "limit").unwrap_or(10).clamp(1, 100);
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .reconciliation_history(&workspace, limit)
                    .and_then(|plans| serde_json::to_value(plans).map_err(Into::into))
            })
            .await
        }
        "reconciliation_execution_status" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let plan_id = required_string(args, "plan_id")?.to_owned();
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .reconciliation_execution_status(&workspace_id, &workspace, &plan_id)
                    .and_then(|status| serde_json::to_value(status).map_err(Into::into))
            })
            .await
        }
        "reconciliation_claim" => {
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let plan_id = required_string(args, "plan_id")?.to_owned();
            let executor = required_string(args, "executor")?.to_owned();
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
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let plan_id = required_string(args, "plan_id")?.to_owned();
            let task_id = required_string(args, "task_id")?.to_owned();
            let executor = required_string(args, "executor")?.to_owned();
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
            let (workspace_id, workspace) = selected_workspace(state, args)?;
            let plan_id = required_string(args, "plan_id")?.to_owned();
            let task_id = required_string(args, "task_id")?.to_owned();
            let harness = state.harness.clone();
            run_blocking(move || {
                harness
                    .reconciliation_retry(&workspace_id, &workspace, &plan_id, &task_id)
                    .and_then(|run| serde_json::to_value(run).map_err(Into::into))
            })
            .await
        }
        _ => Err(anyhow!("unknown intelligence tool: {name}")),
    };
    Ok(outcome)
}
