//! Current native Acceptance capture; JSON and historical records never mint this receipt.
use super::*;
use crate::verification::acceptance::{
    evaluate_acceptance, AcceptanceDiscovery, AcceptanceInput, AcceptancePolicyBinding,
    AcceptancePolicyState,
};
use crate::verification::acceptance_native::NativeAcceptanceRecord;
use crate::verification::change::{GitChangeSnapshot, GitChangeTarget};
use crate::verification::policy_store;
use std::io::{self, Write};

impl ToolHarness {
    pub(crate) async fn acceptance_plan(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        base: &str,
        target: GitChangeTarget,
    ) -> Result<VerificationPlan> {
        let revision = self.current_revision(workspace)?;
        let authority =
            harness_policy_select::policy_authority_fingerprint(workspace, workspace_id)?;
        let git = self
            .git_change_snapshot(workspace, base, target.clone())
            .await?;
        let profile = harness_profile::capture_policy_profile(workspace)?;
        let review = harness_policy_select::candidate_review(workspace_id, &git);
        let selected = self
            .select_native_policy(workspace_id, workspace, &git, &review, &profile)?
            .context("activate a native Acceptance Policy before planning Acceptance")?;
        let plan = self.intelligence.create_policy_verification_plan(
            workspace_id,
            workspace,
            &review,
            crate::intelligence::acceptance_plan::NativePolicyPlan {
                risk_level: selected.risk_level,
                requirements: &selected.selection.requirements,
                required_checks: selected.required_checks.clone(),
                policy_binding: &selected.plan_binding()?,
            },
        )?;
        self.guard_acceptance_inputs(workspace_id, workspace, &revision, &authority, &git)
            .await?;
        Ok(plan)
    }

    pub(crate) async fn acceptance_verify<T: TaskTelemetry>(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        base: &str,
        target: GitChangeTarget,
        timeout_seconds: u64,
        monitor: &T,
    ) -> Result<NativeAcceptanceRecord> {
        let plan = self
            .acceptance_plan(workspace_id, workspace, base, target.clone())
            .await?;
        self.verify_project_candidate(
            workspace_id,
            workspace,
            (&plan.deterministic_level, false),
            timeout_seconds,
            monitor,
            Some((base, target.clone())),
        )
        .await?;
        self.verification_execute_stages(workspace_id, workspace, &plan.id)
            .await?;
        self.record_acceptance(workspace_id, workspace, base, target)
            .await
    }

    pub(crate) async fn capture_acceptance(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        base: &str,
        target: GitChangeTarget,
    ) -> Result<NativeAcceptanceRecord> {
        let revision = self.current_revision(workspace)?;
        let authority =
            harness_policy_select::policy_authority_fingerprint(workspace, workspace_id)?;
        let root = policy_store::workspace_root_digest(workspace)?;
        let git = self.git_change_snapshot(workspace, base, target).await?;
        let profile = harness_profile::capture_policy_profile(workspace)?;
        let review = harness_policy_select::candidate_review(workspace_id, &git);
        let status = self.acceptance_policy_status(workspace_id, workspace)?;
        let state = match status.get("status").and_then(Value::as_str) {
            Some("active") => AcceptancePolicyState::Active,
            Some("inactive") => AcceptancePolicyState::Inactive,
            Some("revoked") => AcceptancePolicyState::Revoked,
            Some("expired") => AcceptancePolicyState::Expired,
            Some("stale_definition") => AcceptancePolicyState::StaleDefinition,
            _ => AcceptancePolicyState::Unavailable,
        };
        let selected =
            if state == AcceptancePolicyState::Active && git.complete && profile.discovery.complete
            {
                self.select_native_policy(workspace_id, workspace, &git, &review, &profile)?
            } else {
                None
            };
        let binding = selected
            .as_ref()
            .map(|selected| -> Result<_> {
                let source_seal = selected.snapshot.source_seal_digest()?;
                let current = policy_store::load(workspace, workspace_id)?
                    .context("Policy authority disappeared during capture")?;
                let current_snapshot = current
                    .snapshot()
                    .context("Policy was revoked during capture")?;
                let current_definitions =
                    self.acceptance_policy_preview(workspace_id, workspace)?;
                Ok(AcceptancePolicyBinding {
                    workspace: workspace_id.into(),
                    root_digest: root.clone(),
                    generation: selected.record.generation(),
                    current_generation: current.generation(),
                    snapshot_digest: selected.snapshot.digest()?,
                    current_snapshot_digest: current_snapshot.digest()?,
                    source_seal_digest: source_seal.clone(),
                    current_source_seal_digest: current_definitions.source_seal_digest()?,
                    selection: selected.selection.clone(),
                    required_checks: selected.required_checks.clone(),
                    expires_at_ms: selected.record.expires_at_ms(),
                })
            })
            .transpose()?;
        let risk = if let Some(selected) = &selected {
            selected.risk.clone()
        } else {
            let known = harness_profile::known_checks_from_profile(&profile);
            self.intelligence.risk_status(
                workspace_id,
                workspace,
                &self.code_index,
                &known,
                &review,
            )?
        };
        let risk_level = crate::execution::verification_risk_floor(workspace, risk.level)?;
        let design = self.intelligence.design_load(workspace)?;
        let discovery = AcceptanceDiscovery {
            complete: profile.discovery.complete,
            mappings_complete: design.initialized
                && design.error_count() == 0
                && selected
                    .as_ref()
                    .is_none_or(|selected| selected.snapshot.mappings.complete),
            checks: profile
                .recommended_checks
                .iter()
                .map(verification_check_binding)
                .collect(),
        };
        let verification = verification_store::load(workspace)?.unwrap_or_default();
        let evidence = evidence_store::load(workspace)?;
        let state_digest = snapshot_digest(&verification, 2 * 1024 * 1024)?;
        let evidence_digest = snapshot_digest(&evidence, 32 * 1024 * 1024)?;
        let plans = verification.plans_for_workspace(workspace_id);
        let plan = plans.iter().rev().find(|plan| {
            plan.revision.as_ref() == Some(&revision)
                && binding
                    .as_ref()
                    .is_none_or(|binding| plan.policy == binding.plan_policy())
        });
        let now_ms = u64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)?
                .as_millis(),
        )?;
        let mut record = evaluate_acceptance(AcceptanceInput {
            workspace: workspace_id,
            current_root_digest: &root,
            revision: &revision,
            git: &git,
            policy_state: if state == AcceptancePolicyState::Active && binding.is_none() {
                AcceptancePolicyState::Unavailable
            } else {
                state
            },
            policy: binding.as_ref(),
            verification: &verification,
            plan_id: plan.map(|plan| plan.id.as_str()),
            evidence: &evidence,
            discovery: &discovery,
            risk_level,
            now_ms,
        })?;
        record.supplement_inspection(self.acceptance_change_inspection(
            workspace_id,
            workspace,
            &revision,
            &git,
            &risk,
        )?)?;
        self.guard_acceptance_inputs(workspace_id, workspace, &revision, &authority, &git)
            .await?;
        let confirmed_state = verification_store::load(workspace)?.unwrap_or_default();
        let confirmed_evidence = evidence_store::load(workspace)?;
        if snapshot_digest(&confirmed_state, 2 * 1024 * 1024)? != state_digest
            || snapshot_digest(&confirmed_evidence, 32 * 1024 * 1024)? != evidence_digest
        {
            bail!("Acceptance evidence or verification state changed during capture; retry");
        }
        self.guard_acceptance_inputs(workspace_id, workspace, &revision, &authority, &git)
            .await?;
        Ok(NativeAcceptanceRecord::captured(record))
    }

    pub(crate) async fn record_acceptance(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        base: &str,
        target: GitChangeTarget,
    ) -> Result<NativeAcceptanceRecord> {
        let captured = self
            .capture_acceptance(workspace_id, workspace, base, target.clone())
            .await?;
        crate::verification::acceptance_store::persist(workspace, workspace_id, &captured)?;
        let confirmed = self
            .capture_acceptance(workspace_id, workspace, base, target)
            .await?;
        if captured.record().digest() != confirmed.record().digest() {
            bail!("Acceptance changed after history append; retained Record is historical, recompute before any gate");
        }
        Ok(captured)
    }

    pub(crate) fn acceptance_metrics(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
    ) -> Result<Value> {
        crate::verification::acceptance_metrics::summarize(
            &self.acceptance_history(workspace_id, workspace)?,
        )
    }

    pub(crate) fn acceptance_history(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
    ) -> Result<Value> {
        crate::verification::acceptance_store::history(workspace, workspace_id)
    }

    async fn guard_acceptance_inputs(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        revision: &crate::evidence::Revision,
        authority: &str,
        git: &GitChangeSnapshot,
    ) -> Result<()> {
        if self.current_revision(workspace)? != *revision {
            bail!("repository revision changed during Acceptance capture; retry");
        }
        harness_policy_select::ensure_policy_authority(workspace, workspace_id, authority)?;
        let confirmed = self
            .git_change_snapshot(workspace, &git.requested_base, git.target.clone())
            .await?;
        if confirmed != *git {
            bail!("Git candidate changed during Acceptance capture; retry");
        }
        Ok(())
    }

    pub(crate) fn verification_approve_authorized_bound(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        approver: &str,
        statement: &str,
        binding: Option<crate::verification::change::ExecutionGitBinding>,
    ) -> Result<crate::evidence::Evidence> {
        self.intelligence.verification_approve_authorized_bound(
            workspace_id,
            workspace,
            plan_id,
            approver,
            statement,
            binding,
        )
    }
}

/// Content digest with a hard streaming byte limit, not a producer authentication.
fn snapshot_digest(value: &impl Serialize, limit: usize) -> Result<String> {
    struct Writer {
        hash: Sha256,
        bytes: usize,
        limit: usize,
    }
    impl Write for Writer {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            let count = self
                .bytes
                .checked_add(bytes.len())
                .filter(|count| *count <= self.limit)
                .ok_or_else(|| io::Error::other("Acceptance snapshot byte limit exceeded"))?;
            self.bytes = count;
            self.hash.update(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut writer = Writer {
        hash: Sha256::new(),
        bytes: 0,
        limit,
    };
    serde_json::to_writer(&mut writer, value)?;
    Ok(format!("sha256:{:x}", writer.hash.finalize()))
}

#[cfg(test)]
#[path = "../../../tests/unit/runtime/harness/acceptance.rs"]
mod tests;
