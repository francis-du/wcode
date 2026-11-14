use super::*;
use crate::design::VerificationRef;

#[path = "verification_support.rs"]
mod verification_support;
use verification_support::*;

// Scope an operator decision to the entire immutable plan, including policy,
// check signatures, revision and plan identity. Legacy revision-only approvals
// stay readable but cannot authorize a new or amended plan.
pub(super) fn human_approval_policy(plan: &VerificationPlan) -> Result<String> {
    Ok(format!(
        "human-approval/v1/sha256:{:x}",
        Sha256::digest(serde_json::to_vec(plan)?)
    ))
}

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
        Self::verification_status_from_snapshot(status, revision, &evidence, None).map(Some)
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
        Self::verification_status_from_snapshot(status, &revision, &evidence, None).map(Some)
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
            .map(|status| {
                Self::verification_status_from_snapshot(status, &revision, &evidence, None)
            })
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
        git_binding: Option<&crate::verification::change::ExecutionGitBinding>,
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
                Self::verification_status_from_snapshot(status, revision, evidence, git_binding)
            })
            .collect()
    }
}
impl SoftwareIntelligenceRuntime {
    pub(crate) fn verification_status_from_snapshot(
        mut status: VerificationStatus,
        current_revision: &Revision,
        evidence: &[Evidence],
        git_binding: Option<&crate::verification::change::ExecutionGitBinding>,
    ) -> Result<VerificationStatus> {
        if !revision_identity_complete(current_revision) {
            status.blockers.push("workspace-revision-incomplete".into());
        }
        if let Some(plan_revision) = status.plan.revision.as_ref() {
            if !revision_identity_complete(plan_revision) {
                status
                    .blockers
                    .push("verification-plan-revision-incomplete".into());
            }
            if current_revision.code != plan_revision.code {
                status
                    .blockers
                    .push("workspace-revision-changed-since-plan".into());
            }
            if current_revision.design != plan_revision.design {
                status
                    .blockers
                    .push("design-revision-changed-since-plan".into());
            }
        } else {
            // A legacy subject binds only code text, never the full code and
            // Design identity. Keep the record readable without granting ready.
            status
                .blockers
                .push("verification-plan-revision-unbound".into());
            let current_subject = format!("change:{}", current_revision.code);
            if current_subject != status.plan.subject {
                status
                    .blockers
                    .push("workspace-revision-changed-since-plan".into());
            }
        }
        status.deterministic_result = deterministic_plan_result(&mut status, evidence, git_binding);
        match status.deterministic_result {
            Some(EvidenceResult::Pass) => {}
            Some(EvidenceResult::Fail) => status
                .blockers
                .push("deterministic-verification-failed".into()),
            Some(EvidenceResult::Inconclusive | EvidenceResult::Disagree) => status
                .blockers
                .push("deterministic-verification-inconclusive".into()),
            None => status
                .blockers
                .push("deterministic-verification-missing".into()),
        }
        let require_property = status.plan.require_property;
        let require_mutation = status.plan.require_mutation;
        let require_fuzz = status.plan.require_fuzz;
        apply_stage_status(
            &mut status,
            evidence,
            VerificationStage::Property,
            EvidenceKind::Property,
            require_property,
        );
        apply_stage_status(
            &mut status,
            evidence,
            VerificationStage::Mutation,
            EvidenceKind::Mutation,
            require_mutation,
        );
        apply_stage_status(
            &mut status,
            evidence,
            VerificationStage::Fuzz,
            EvidenceKind::Fuzz,
            require_fuzz,
        );
        let runtime_required = status
            .plan
            .deterministic_checks
            .iter()
            .any(|check| check == "runtime-gate");
        apply_stage_status(
            &mut status,
            evidence,
            VerificationStage::RuntimeCanary,
            EvidenceKind::Runtime,
            runtime_required,
        );
        let approval_policy = human_approval_policy(&status.plan)?;
        let human_approvals = evidence
            .iter()
            .filter(|record| {
                evidence_matches_plan_revision(record, &status.plan)
                    && record.kind == EvidenceKind::HumanApproval
                    && record.authority == EvidenceAuthority::LocalOperator
                    && record.policy.as_deref() == Some(approval_policy.as_str())
            })
            .collect::<Vec<_>>();
        let latest_human_timestamp = human_approvals
            .iter()
            .map(|record| record.timestamp_ms)
            .max();
        status.human_approval = latest_human_timestamp.is_some_and(|timestamp| {
            aggregate_results(
                human_approvals
                    .iter()
                    .filter(|record| record.timestamp_ms == timestamp)
                    .map(|record| record.result),
            ) == Some(EvidenceResult::Pass)
        });
        if status.plan.require_human_approval && !status.human_approval {
            status.blockers.push("human-approval-required".into());
        }
        status.blockers.sort();
        status.blockers.dedup();
        status.ready = status.blockers.is_empty()
            && status.submitted == status.plan.job_ids.len()
            && status.deterministic_result == Some(EvidenceResult::Pass);
        Ok(status)
    }

    #[cfg(test)]
    pub(crate) fn record_verification_report(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        expected_revision: &Revision,
        report: &VerificationReport,
    ) -> Result<Vec<Evidence>> {
        self.record_verification_report_from_design(
            workspace_id,
            workspace,
            expected_revision,
            None,
            report,
        )
    }

    pub(crate) fn record_verification_report_from_design(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        expected_revision: &Revision,
        design_snapshot: Option<&design::DesignLoad>,
        report: &VerificationReport,
    ) -> Result<Vec<Evidence>> {
        self.record_verification_report_policy_bound(
            workspace_id,
            workspace,
            expected_revision,
            design_snapshot,
            report,
            None,
        )
    }

    pub(crate) fn record_verification_report_policy_bound(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        expected_revision: &Revision,
        design_snapshot: Option<&design::DesignLoad>,
        report: &VerificationReport,
        policy_binding: Option<&str>,
    ) -> Result<Vec<Evidence>> {
        // A bounded scan is not proof of the whole workspace, even when two
        // partial digests match. Never mint complete verification evidence.
        if expected_revision.code.ends_with(":partial")
            || expected_revision
                .design
                .as_deref()
                .is_some_and(|revision| revision.ends_with(":partial"))
        {
            return Err(anyhow!(
                "verification revision is incomplete; no evidence was recorded; use a workspace within the revision scan limit"
            ));
        }
        // Evidence belongs to the inputs captured before execution, never to
        // whichever files happen to exist when the checks finish. Reused
        // checks already have exact-revision proof from the earlier run and
        // must not mint duplicate Evidence merely because a report cites them.
        let receipt = native_report_receipt(workspace, expected_revision, report, policy_binding)?;
        let revision = expected_revision.clone();
        let executed_checks = report.checks.iter().filter(|check| !check.reused).count();
        let loaded_design = if executed_checks > 0 && design_snapshot.is_none() {
            Some(self.design_load(workspace)?)
        } else {
            None
        };
        let design = if executed_checks > 0 {
            design_snapshot.or(loaded_design.as_deref())
        } else {
            None
        };
        let mut produced = Vec::new();
        for check in report.checks.iter().filter(|check| !check.reused) {
            let mut evidence = Evidence::new(
                self.next_id("EV"),
                format!("verification:{}", check.id),
                evidence_kind_for_check(&check.id),
                check.command.clone(),
                revision.clone(),
                if check.success {
                    EvidenceResult::Pass
                } else {
                    EvidenceResult::Fail
                },
                Confidence::Deterministic,
            )?;
            if let Some(item) = receipt
                .as_ref()
                .and_then(|receipt| receipt.checks.iter().find(|item| item.check.id == check.id))
            {
                if item.execution != crate::evidence::VerificationCheckExecution::Executed {
                    evidence.result = EvidenceResult::Inconclusive;
                }
            }
            evidence.execution_git_binding = report.execution_git_binding.clone();
            evidence.execution_policy_binding = policy_binding.map(str::to_owned);
            evidence.authority = EvidenceAuthority::NativeVerification;
            evidence.policy = Some(format!(
                "deterministic/{}/v{}",
                report.level,
                if receipt.is_some() { 2 } else { 1 }
            ));
            if let Some(item) = receipt
                .as_ref()
                .and_then(|receipt| receipt.checks.iter().find(|item| item.check.id == check.id))
            {
                evidence.execution_receipt = Some(VerificationExecutionReceipt {
                    schema_version: 1,
                    level: report.level.clone(),
                    required_checks: vec![item.check.clone()],
                    checks: vec![item.clone()],
                    skipped_checks: vec![],
                    execution_git_binding: report.execution_git_binding.clone(),
                });
            }
            evidence.summary = Some(crate::report_types::verification_metrics_summary(
                check.execution_ms,
                check.phase,
            ));
            evidence.artifact_digest = Some(format!(
                "sha256:{}",
                digest_text(&format!(
                    "{}\n{:?}\n{}\n{}",
                    check.command, check.exit_code, check.stdout_tail, check.stderr_tail
                ))
            ));
            produced.push(evidence);
        }
        // A single language provider proves only its own check, never the
        // complete project gate. Keep quick/full policies distinct as well.
        if executed_checks > 0 && matches!(report.level.as_str(), "quick" | "full") {
            let mut aggregate = Evidence::new(
                self.next_id("EV"),
                format!("change:{}", revision.code),
                EvidenceKind::Verification,
                "verify_project".into(),
                revision.clone(),
                receipt.as_ref().map_or(
                    if report.checks.iter().any(|check| !check.success) {
                        EvidenceResult::Fail
                    } else {
                        EvidenceResult::Inconclusive
                    },
                    VerificationExecutionReceipt::result,
                ),
                Confidence::Deterministic,
            )?;
            aggregate.execution_git_binding = report.execution_git_binding.clone();
            aggregate.execution_policy_binding = policy_binding.map(str::to_owned);
            aggregate.authority = EvidenceAuthority::NativeVerification;
            aggregate.policy = Some(format!(
                "deterministic/{}/v{}",
                report.level,
                if receipt.is_some() { 2 } else { 1 }
            ));
            aggregate.execution_receipt = receipt.clone();
            aggregate.artifact_digest = Some(format!(
                "sha256:{}",
                digest_text(&format!(
                    "{}\n{}\n{}\n{}\n{}",
                    report.level,
                    report.checks_run,
                    report.checks_reused,
                    report.checks_failed,
                    report.summary
                ))
            ));
            produced.push(aggregate);
        }
        if let Some(design) = design.as_ref().filter(|_| receipt.is_some()) {
            for criterion in design.state.acceptance.values() {
                if !criterion
                    .verification
                    .iter()
                    .any(|reference| verification_reference_executed(reference, report))
                {
                    continue;
                }
                let outcomes = criterion
                    .verification
                    .iter()
                    .filter_map(|reference| verification_reference_outcome(reference, report))
                    .collect::<Vec<_>>();
                if outcomes.is_empty() {
                    continue;
                }
                let result = if outcomes.iter().any(|outcome| !outcome) {
                    EvidenceResult::Fail
                } else if outcomes.len() < criterion.verification.len() {
                    EvidenceResult::Inconclusive
                } else {
                    EvidenceResult::Pass
                };
                let mut evidence = Evidence::new(
                    self.next_id("EV"),
                    criterion.id.clone(),
                    EvidenceKind::IntegrationTest,
                    "deterministic-verification-mesh".into(),
                    revision.clone(),
                    result,
                    Confidence::Deterministic,
                )?;
                evidence.execution_git_binding = report.execution_git_binding.clone();
                evidence.execution_policy_binding = policy_binding.map(str::to_owned);
                evidence.authority = EvidenceAuthority::NativeVerification;
                evidence.policy = Some(format!("acceptance/{}/v2", report.level));
                evidence.execution_receipt = receipt.clone();
                produced.push(evidence);
            }
        }
        let current = self.current_revision(workspace)?;
        if current.code != revision.code || current.design != revision.design {
            let code_changed = current.code != revision.code;
            let design_changed = current.design != revision.design;
            return Err(anyhow!(
                "verification revision changed during execution (code_changed={code_changed}, design_changed={design_changed}, expected_code={}, current_code={}, expected_design={}, current_design={}); results are stale, no evidence was recorded; rerun verification on a stable workspace",
                revision.code,
                current.code,
                revision.design.as_deref().unwrap_or("none"),
                current.design.as_deref().unwrap_or("none"),
            ));
        }
        for evidence in &produced {
            evidence_store::persist(workspace, evidence)?;
        }
        // Local lessons are advisory metadata. A retention error must neither
        // discard executed Evidence nor change deterministic verification.
        if crate::engineering_journal::failure_memory::observe_native_verification(
            workspace,
            &revision,
            report.execution_git_binding.as_ref(),
            policy_binding,
            &produced,
            &[],
        )
        .is_err()
        {
            tracing::warn!("local verification failure memory could not be retained");
        }
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("software intelligence state poisoned"))?;
        for evidence in &produced {
            push_evidence(&mut state.evidence, workspace_id, evidence.clone());
        }
        Ok(produced)
    }
}
