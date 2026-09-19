use super::*;

impl SoftwareIntelligenceRuntime {
    pub(crate) fn reconciliation_approve(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        approver: &str,
        statement: &str,
    ) -> Result<serde_json::Value> {
        let plan = self.reconciliation_status(workspace, plan_id)?;
        if plan.workspace != workspace_id {
            return Err(anyhow!(
                "reconciliation plan does not belong to the selected workspace"
            ));
        }
        let revision = self.current_revision(workspace)?;
        let execution = reconciliation_execution_store::load(workspace, plan_id)?;
        let before = crate::reconciliation_approval::status(
            workspace,
            &plan,
            &revision,
            execution.as_ref(),
        )?;
        if before["replan_required"].as_bool() == Some(true) {
            return Err(anyhow!(
                "replan_required: approved reconciliation plan no longer matches current premises: {}",
                before["reasons"]
            ));
        }
        if before["approved"].as_bool() == Some(true) {
            return Ok(before);
        }

        let snapshot = crate::reconciliation_approval::approve(
            workspace, &plan, &revision, approver, statement,
        )?;

        let mut evidence = Evidence::new(
            self.next_id("EV"),
            format!("reconciliation-plan:{plan_id}"),
            EvidenceKind::HumanApproval,
            format!("human:{}", approver.trim()),
            revision.clone(),
            EvidenceResult::Pass,
            Confidence::High,
        )?;
        evidence.policy = Some("reconciliation-plan-approval/v1".to_owned());
        evidence.artifact_digest = Some(format!("sha256:{}", snapshot.plan_digest));
        evidence.summary = Some(statement.trim().to_owned());
        evidence.targets = snapshot.acceptance_refs.iter().take(32).cloned().collect();
        evidence.validate()?;
        evidence_store::persist(workspace, &evidence)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("software intelligence state poisoned"))?;
        push_evidence(&mut state.evidence, workspace_id, evidence);
        drop(state);

        crate::reconciliation_approval::status(workspace, &plan, &revision, execution.as_ref())
    }

    pub(crate) fn reconciliation_approval_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
    ) -> Result<serde_json::Value> {
        let plan = self.reconciliation_status(workspace, plan_id)?;
        if plan.workspace != workspace_id {
            return Err(anyhow!(
                "reconciliation plan does not belong to the selected workspace"
            ));
        }
        let revision = self.current_revision(workspace)?;
        let execution = reconciliation_execution_store::load(workspace, plan_id)?;
        crate::reconciliation_approval::status(workspace, &plan, &revision, execution.as_ref())
    }

    pub(super) fn approved_reconciliation_snapshot(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
    ) -> Result<crate::reconciliation_approval::ApprovedPlanSnapshot> {
        let snapshot = crate::reconciliation_approval::require(workspace, plan_id)?;
        if snapshot.workspace != workspace_id {
            return Err(anyhow!(
                "approved reconciliation plan does not belong to the selected workspace"
            ));
        }
        let status = self.reconciliation_approval_status(workspace_id, workspace, plan_id)?;
        if status["replan_required"].as_bool() == Some(true) {
            return Err(anyhow!(
                "replan_required: approved reconciliation plan no longer matches current premises: {}",
                status["reasons"]
            ));
        }
        Ok(snapshot)
    }

    pub(super) fn reconciliation_verification_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan: &ReconciliationPlan,
        execution: Option<&ReconciliationExecution>,
    ) -> Result<VerificationStatus> {
        let original =
            self.verification_status(workspace_id, workspace, &plan.verification_plan.id)?;
        let current_revision = self.current_revision(workspace)?;
        let approved = crate::reconciliation_approval::load(workspace, &plan.id)?.is_some();
        if !approved
            || !reconciliation_model_execution_started(execution)
            || plan.verification_plan.revision.as_ref() == Some(&current_revision)
        {
            return Ok(original);
        }

        let history = self.verification_history(workspace_id, workspace, 100)?;
        Ok(
            select_current_reconciliation_verification(plan, &current_revision, &history)
                .cloned()
                .unwrap_or(original),
        )
    }
}

fn select_current_reconciliation_verification<'a>(
    plan: &ReconciliationPlan,
    current_revision: &Revision,
    history: &'a [VerificationStatus],
) -> Option<&'a VerificationStatus> {
    let max_risk = history
        .iter()
        .filter(|status| {
            status.plan.revision.as_ref() == Some(current_revision)
                && status.plan.risk_level >= plan.risk_level
        })
        .map(|status| status.plan.risk_level)
        .max()?;
    history
        .iter()
        .filter(|status| {
            status.plan.revision.as_ref() == Some(current_revision)
                && status.plan.risk_level == max_risk
        })
        .max_by_key(|status| status.ready)
}

fn reconciliation_model_execution_started(execution: Option<&ReconciliationExecution>) -> bool {
    execution.is_some_and(|execution| {
        execution.tasks.iter().any(|run| {
            matches!(
                run.task.kind,
                ReconciliationTaskKind::Design
                    | ReconciliationTaskKind::Implementation
                    | ReconciliationTaskKind::Review
            ) && run.status != ReconciliationRunStatus::Pending
        })
    })
}

#[cfg(test)]
#[path = "../../../tests/unit/intelligence/approval.rs"]
mod tests;
