use super::*;

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
