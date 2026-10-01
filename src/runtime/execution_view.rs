use super::*;

pub(crate) fn active_summary(
    harness: &ToolHarness,
    workspace_id: &str,
    workspace: &Workspace,
) -> Result<Option<Value>> {
    Ok(summary(&refresh(harness, workspace_id, workspace, false)?))
}

pub(super) fn summary(status: &Value) -> Option<Value> {
    // Completed state is still authoritative context, not an absent execution.
    if !status["exists"].as_bool().unwrap_or(false) {
        return None;
    }
    Some(json!({
        "id": status["execution_id"],
        "revision": status["revision"],
        "objective": status["objective"],
        "phase": status["phase"],
        "checkpoint": status["checkpoint"],
        "proposal": status["proposal"],
        "pending_directive": status["pending_directive"],
        "replan_required": status["replan_required"],
        "verification_floor": status["verification_floor"],
        "lineage": status["lineage"],
        "scope_completion": status["scope_completion"],
        "guidance": "Resume from this checkpoint before reconstructing completed work. Apply pending structured steering through a newer Worklist revision and, when replan_required=true, a new Reconciliation plan before unrelated work. A model proposal is advisory only; current-revision Worklist/Reconciliation/Verification authority settles terminal state."
    }))
}

pub(super) fn status_value(execution: &Execution) -> Value {
    json!({
        "exists": true,
        "active": execution.phase != ExecutionPhase::Completed,
        "scope_completion": scope_completion(execution),
        "execution_id": execution.execution_id,
        "revision": execution.revision,
        "objective": execution.objective,
        "phase": execution.phase,
        "created_at_ms": execution.created_at_ms,
        "updated_at_ms": execution.updated_at_ms,
        "checkpoint": execution.checkpoint,
        "proposal": execution.proposal,
        "pending_directive": execution.pending_directive,
        "replan_required": execution.pending_directive.as_ref().is_some_and(|directive| directive.requires_replan),
        "lineage": execution.lineage,
        "verification_floor": execution.verification_floor,
        "settlement_authority": "worklist+reconciliation+verification_evidence",
    })
}

pub(super) fn empty_status() -> Value {
    json!({
        "exists": false,
        "active": false,
        "scope_completion": {"allowed":false,"open_items":null,"required_action":"initialize_scope"},
        "revision": 0,
        "checkpoint": Value::Null,
        "proposal": Value::Null,
        "pending_directive": Value::Null,
        "replan_required": false,
        "lineage": Value::Null,
        "verification_floor": Value::Null,
    })
}

fn scope_completion(execution: &Execution) -> Value {
    let action = match execution.phase {
        ExecutionPhase::Completed => "none",
        ExecutionPhase::Executing => "continue",
        ExecutionPhase::Blocked => "resolve_blockers",
        ExecutionPhase::Verifying => "verify",
    };
    json!({
        "allowed": execution.phase == ExecutionPhase::Completed,
        "open_items": execution.checkpoint.open_items,
        "required_action": action
    })
}
