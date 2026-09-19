use super::*;

impl SoftwareIntelligenceRuntime {
    pub(crate) fn reconciliation_claim(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        executor: &str,
        kinds: &[ReconciliationTaskKind],
    ) -> Result<ReconciliationTaskRun> {
        let snapshot = self.approved_reconciliation_snapshot(workspace_id, workspace, plan_id)?;
        self.reconciliation_execution_status_from_plan(workspace_id, workspace, &snapshot.plan)?;
        let mut execution = reconciliation_execution_store::load(workspace, plan_id)?
            .ok_or_else(|| anyhow!("reconciliation execution state does not exist"))?;
        let run = execution.claim(executor, kinds)?;
        reconciliation_execution_store::persist(workspace, &execution)?;
        Ok(run)
    }
}
