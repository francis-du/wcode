use super::*;

impl ToolHarness {
    pub fn reconciliation_plan(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        review: &ChangeReviewReport,
    ) -> Result<ReconciliationPlan> {
        let known_checks = self.known_checks(workspace)?;
        self.intelligence.reconciliation_plan(
            workspace_id,
            workspace,
            &self.code_index,
            &known_checks,
            review,
        )
    }

    pub fn reconciliation_status(
        &self,
        workspace: &Workspace,
        plan_id: &str,
    ) -> Result<ReconciliationPlan> {
        self.intelligence.reconciliation_status(workspace, plan_id)
    }

    pub fn reconciliation_history(
        &self,
        workspace: &Workspace,
        limit: usize,
    ) -> Result<Vec<ReconciliationPlan>> {
        self.intelligence.reconciliation_history(workspace, limit)
    }

    pub fn reconciliation_execution_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
    ) -> Result<ReconciliationExecutionStatus> {
        self.intelligence
            .reconciliation_execution_status(workspace_id, workspace, plan_id)
    }

    pub fn reconciliation_approve(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        approver: &str,
        statement: &str,
    ) -> Result<Value> {
        self.intelligence.reconciliation_approve(
            workspace_id,
            workspace,
            plan_id,
            approver,
            statement,
        )
    }

    pub fn reconciliation_approval_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
    ) -> Result<Value> {
        self.intelligence
            .reconciliation_approval_status(workspace_id, workspace, plan_id)
    }

    pub fn reconciliation_claim(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        executor: &str,
        kinds: &[ReconciliationTaskKind],
        task_id: Option<&str>,
    ) -> Result<ReconciliationTaskRun> {
        self.intelligence.reconciliation_claim(
            workspace_id,
            workspace,
            plan_id,
            executor,
            kinds,
            task_id,
        )
    }

    pub fn reconciliation_submit(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        task_id: &str,
        executor: &str,
        submission: ReconciliationTaskSubmission,
    ) -> Result<ReconciliationTaskRun> {
        self.intelligence.reconciliation_submit(
            workspace_id,
            workspace,
            plan_id,
            task_id,
            executor,
            submission,
        )
    }

    pub fn reconciliation_retry(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        task_id: &str,
    ) -> Result<ReconciliationTaskRun> {
        self.intelligence
            .reconciliation_retry(workspace_id, workspace, plan_id, task_id)
    }
}
