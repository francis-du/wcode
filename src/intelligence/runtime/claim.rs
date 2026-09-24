use super::*;

impl SoftwareIntelligenceRuntime {
    pub(crate) fn reconciliation_claim(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        executor: &str,
        kinds: &[ReconciliationTaskKind],
        task_id: Option<&str>,
    ) -> Result<ReconciliationTaskRun> {
        let snapshot = self.approved_reconciliation_snapshot(workspace_id, workspace, plan_id)?;
        self.reconciliation_execution_status_from_plan(workspace_id, workspace, &snapshot.plan)?;
        reconciliation_execution_store::update_existing(workspace, plan_id, |execution| {
            Ok(execution.claim_task(executor, kinds, task_id)?)
        })
    }
}
