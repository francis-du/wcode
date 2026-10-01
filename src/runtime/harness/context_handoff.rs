use super::*;

pub(super) fn prune(value: &mut Value) {
    if let Some(pack) = value.as_object_mut() {
        pack.remove("worklist");
        if let Some(execution) = pack.get_mut("execution").and_then(Value::as_object_mut) {
            execution.retain(|key, _| {
                matches!(
                    key.as_str(),
                    "id" | "revision"
                        | "scope_completion"
                        | "pending_directive"
                        | "verification_floor"
                        | "replan_required"
                )
            });
        }
    }
}

impl ToolHarness {
    pub(crate) fn agent_handoff_context(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        query: &str,
    ) -> Result<Value> {
        self.build_agent_context(workspace_id, workspace, query, 0, &[], true)
    }

    pub(crate) fn finalize_handoff_context(value: &mut Value) -> Result<()> {
        // Also protect synthetic context builders used by guarded claims.
        prune(value);
        let Some(budget) = value
            .get("budget")
            .and_then(Value::as_u64)
            .and_then(|budget| usize::try_from(budget).ok())
        else {
            return Ok(());
        };
        let baseline = value
            .get("baseline_context_bytes")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        // Preserve exact delivered-byte telemetry after any final filtering.
        finalize_agent_context(value, baseline, budget)
    }
}
