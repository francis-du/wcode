use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ObservatoryRevisionState {
    pub stable_inputs_key: String,
    pub full_snapshot_key: String,
    pub git_observation: Value,
}

// A stable observation key can describe known unavailability. It is never
// a Git receipt or authorization to execute a command in a read-only workspace.
pub(super) async fn git_revision_signal(
    harness: &ToolHarness,
    workspace: &Workspace,
) -> AnyResult<(String, Value)> {
    if !workspace.exec_enabled() {
        let observation = json!({"available":false,"reason":"execution_disabled"});
        return Ok((serde_json::to_string(&observation)?, observation));
    }
    let binding = harness.execution_git_binding(workspace).await?;
    let observation = if binding.is_some() {
        json!({"available":true})
    } else {
        json!({"available":false,"reason":"not_a_repository"})
    };
    Ok((serde_json::to_string(&binding)?, observation))
}

pub(crate) async fn revision_state(
    harness: &ToolHarness,
    workspace_id: &str,
    workspace: &Workspace,
) -> AnyResult<ObservatoryRevisionState> {
    let source = harness.observatory_revision_signal(workspace).await?;
    if source.full_refresh_required || source.truncated {
        anyhow::bail!("observatory revision inputs are truncated");
    }
    let (git, git_observation) = git_revision_signal(harness, workspace).await?;
    let harness = harness.clone();
    let workspace = workspace.clone();
    let workspace_id = workspace_id.to_owned();
    let (graph, proof, engineering) = mcp_tools::run_blocking(move || -> AnyResult<_> {
        let (graph, (proof, engineering)) = rayon::join(
            || harness.observatory_graph_signal(&workspace),
            || {
                rayon::join(
                    || harness.observatory_proof_signal(&workspace_id, &workspace),
                    || harness.observatory_engineering_signal(&workspace),
                )
            },
        );
        Ok((graph?, proof?, engineering?))
    })
    .await?;
    let source_key = source.fingerprint.unwrap_or_else(|| "full".to_owned());
    let stable_inputs_key = format!("{source_key}|{proof}|{engineering}|git={git}");
    let graph_key = graph
        .map(|(revision, signal)| if signal.is_empty() { revision } else { signal })
        .unwrap_or_default();
    let full_snapshot_key = format!("{source_key}|{graph_key}|{proof}|{engineering}|git={git}");
    Ok(ObservatoryRevisionState {
        stable_inputs_key,
        full_snapshot_key,
        git_observation,
    })
}

pub(super) fn snapshot(
    harness: &ToolHarness,
    workspace_id: &str,
    workspace: &Workspace,
) -> AnyResult<Value> {
    let ((design_traceability, semantics_scope), (graph_providers, proof_reconciliation)) =
        rayon::join(
            || {
                rayon::join(
                    || design_traceability(harness, workspace_id, workspace),
                    || semantics_scope(harness, workspace_id, workspace),
                )
            },
            || {
                rayon::join(
                    || graph_providers(harness, workspace),
                    || proof_reconciliation(harness, workspace_id, workspace),
                )
            },
        );

    let (design, traceability) = design_traceability?;
    let (semantics, scope_status) = semantics_scope?;
    let (graph_history, graph_diff, graph_providers, semantic_providers, verification_executors) =
        graph_providers?;
    let (evidence, reconciliation, reconciliation_execution, verification) = proof_reconciliation?;
    Ok(json!({
        "design": design,
        "traceability": traceability,
        "semantics": semantics,
        "scope_status": scope_status,
        "graph_history": graph_history,
        "graph_diff": graph_diff,
        "graph_providers": graph_providers,
        "semantic_providers": semantic_providers,
        "verification_executors": verification_executors,
        "evidence": evidence,
        "reconciliation": reconciliation,
        "reconciliation_execution": reconciliation_execution,
        "verification": verification,
    }))
}

fn design_traceability(
    harness: &ToolHarness,
    workspace_id: &str,
    workspace: &Workspace,
) -> AnyResult<(Value, Value)> {
    // Keep these in one lane so a cold traceability request reuses the Design
    // snapshot that design_status just populated instead of racing a duplicate load.
    let design = harness.design_status(workspace_id.to_owned(), workspace)?;
    let traceability = harness.traceability_status(workspace_id.to_owned(), workspace)?;
    Ok((
        serde_json::to_value(design)?,
        serde_json::to_value(traceability)?,
    ))
}

fn semantics_scope(
    harness: &ToolHarness,
    workspace_id: &str,
    workspace: &Workspace,
) -> AnyResult<(Value, Value)> {
    let (semantics, scope_status) = rayon::join(
        || harness.semantic_status(workspace_id, workspace, 100),
        || harness.product_scope_status(workspace),
    );
    Ok((
        serde_json::to_value(semantics?)?,
        serde_json::to_value(scope_status?)?,
    ))
}

fn graph_providers(
    harness: &ToolHarness,
    workspace: &Workspace,
) -> AnyResult<(Value, Value, Value, Value, Value)> {
    let ((history_diff, graph_providers), (semantic_providers, verification_executors)) =
        rayon::join(
            || {
                rayon::join(
                    || graph_history_diff(harness, workspace),
                    || harness.graph_provider_status(workspace),
                )
            },
            || {
                rayon::join(
                    || harness.semantic_provider_status(workspace),
                    || harness.verification_executor_status(workspace),
                )
            },
        );
    let (graph_history, graph_diff) = history_diff?;
    Ok((
        graph_history,
        graph_diff,
        serde_json::to_value(graph_providers?)?,
        serde_json::to_value(semantic_providers?)?,
        serde_json::to_value(verification_executors?)?,
    ))
}

fn graph_history_diff(harness: &ToolHarness, workspace: &Workspace) -> AnyResult<(Value, Value)> {
    let graph_history = harness.graph_history(workspace, 20)?;
    let graph_diff = if graph_history.len() >= 2 {
        harness
            .graph_diff(
                workspace,
                &GraphDiffInput {
                    from_snapshot_id: None,
                    to_snapshot_id: None,
                    limit: 20,
                },
            )
            .ok()
    } else {
        None
    };
    Ok((
        serde_json::to_value(graph_history)?,
        serde_json::to_value(graph_diff)?,
    ))
}

fn proof_reconciliation(
    harness: &ToolHarness,
    workspace_id: &str,
    workspace: &Workspace,
) -> AnyResult<(Value, Value, Value, Value)> {
    harness.intelligence_status_proof_reconciliation_snapshot(workspace_id, workspace, 100, 20, 20)
}
