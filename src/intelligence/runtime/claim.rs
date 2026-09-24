use super::*;

impl SoftwareIntelligenceRuntime {
    pub(crate) fn reconciliation_claim_owned(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        executor: &str,
        kinds: &[ReconciliationTaskKind],
        selection: crate::reconcile::ReconciliationClaimSelection<'_>,
    ) -> Result<ReconciliationTaskRun> {
        let snapshot = self.approved_reconciliation_snapshot(workspace_id, workspace, plan_id)?;
        self.reconciliation_execution_status_from_plan(workspace_id, workspace, &snapshot.plan)?;
        reconciliation_execution_store::update_existing(workspace, plan_id, |execution| {
            Ok(execution.claim_task_with_owner_binding(
                executor,
                kinds,
                selection.task_id,
                selection.owner_binding,
            )?)
        })
    }
}
