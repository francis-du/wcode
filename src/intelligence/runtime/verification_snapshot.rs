use super::*;

type StatusProofReconciliationSnapshot = (
    EvidenceStatus,
    Vec<ReconciliationPlan>,
    Vec<ReconciliationExecutionStatus>,
    Vec<VerificationStatus>,
);

impl SoftwareIntelligenceRuntime {
    pub(crate) fn verification_status_for_revision(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        revision: &Revision,
    ) -> Result<Option<VerificationStatus>> {
        self.ensure_verification_loaded(workspace_id, workspace)?;
        let status = {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            match state
                .verification
                .latest_plan_for_workspace_revision(workspace_id, revision)
            {
                Some(plan) => Some(state.verification.status(&plan.id)?),
                None => None,
            }
        };
        let Some(status) = status else {
            return Ok(None);
        };
        let evidence = self.evidence_records(workspace_id, workspace)?;
        Self::verification_status_from_snapshot(status, revision, &evidence).map(Some)
    }

    pub(crate) fn verification_status_if_present(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
    ) -> Result<Option<VerificationStatus>> {
        self.ensure_verification_loaded(workspace_id, workspace)?;
        let status = {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            match state.verification.status(plan_id) {
                Ok(status) => Some(status),
                Err(crate::verification::VerificationError::UnknownPlan) => None,
                Err(error) => return Err(error.into()),
            }
        };
        let Some(status) = status else {
            return Ok(None);
        };
        if status.plan.workspace != workspace_id {
            return Err(anyhow!(
                "verification plan does not belong to the selected workspace"
            ));
        }
        let revision = self.current_revision(workspace)?;
        let evidence = self.evidence_records(workspace_id, workspace)?;
        Self::verification_status_from_snapshot(status, &revision, &evidence).map(Some)
    }

    pub(crate) fn status_proof_reconciliation_snapshot(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        evidence_limit: usize,
        verification_limit: usize,
        reconciliation_limit: usize,
    ) -> Result<StatusProofReconciliationSnapshot> {
        let (reconciliation, ((revision, evidence), history)) = rayon::join(
            || self.reconciliation_history(workspace, reconciliation_limit),
            || {
                rayon::join(
                    || {
                        rayon::join(
                            || self.current_revision(workspace),
                            || self.evidence_records(workspace_id, workspace),
                        )
                    },
                    || self.verification_base_history(workspace_id, workspace, verification_limit),
                )
            },
        );
        let reconciliation = reconciliation?;
        let revision = revision?;
        let evidence = evidence?;
        let history = history?;
        let evidence_status =
            Self::evidence_status_from_snapshot(workspace_id, None, evidence_limit, &evidence);
        let verification = history
            .into_iter()
            .map(|status| Self::verification_status_from_snapshot(status, &revision, &evidence))
            .collect::<Result<Vec<_>>>()?;
        let reconciliation_execution = self
            .reconciliation_execution_statuses_from_snapshot(
                workspace_id,
                workspace,
                &reconciliation,
                &revision,
                &evidence,
            )
            .unwrap_or_default();
        Ok((
            evidence_status,
            reconciliation,
            reconciliation_execution,
            verification,
        ))
    }

    pub(crate) fn verification_statuses_from_snapshot(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_ids: &[String],
        revision: &Revision,
        evidence: &[Evidence],
    ) -> Result<Vec<VerificationStatus>> {
        self.ensure_verification_loaded(workspace_id, workspace)?;
        let statuses = {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            plan_ids
                .iter()
                .map(|plan_id| state.verification.status(plan_id).map_err(Into::into))
                .collect::<Result<Vec<_>>>()?
        };
        statuses
            .into_iter()
            .map(|status| {
                if status.plan.workspace != workspace_id {
                    return Err(anyhow!(
                        "verification plan does not belong to the selected workspace"
                    ));
                }
                Self::verification_status_from_snapshot(status, revision, evidence)
            })
            .collect()
    }
}
