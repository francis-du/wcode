use super::*;

pub(in crate::verification::acceptance) fn context_reasons(
    input: &AcceptanceInput<'_>,
    reasons: &mut Vec<AcceptanceReason>,
) {
    if !hash(&input.revision.code) || !input.revision.design.as_deref().is_some_and(hash) {
        reason(
            reasons,
            "revision_incomplete",
            None,
            AcceptanceAction::CaptureContext,
        );
    }
    if !input.discovery.complete {
        reason(
            reasons,
            "discovery_incomplete",
            None,
            AcceptanceAction::ResolveDiscovery,
        );
    }
    if !input.discovery.mappings_complete {
        reason(
            reasons,
            "mapping_incomplete",
            None,
            AcceptanceAction::ResolveDiscovery,
        );
    }
    let git = input.git;
    if !git.complete
        || !git.unknown_reasons.is_empty()
        || !git.base_sha.as_deref().is_some_and(full_oid)
        || !git.target_sha.as_deref().is_some_and(full_oid)
        || !git.binding.as_ref().is_some_and(|binding| binding.valid())
        || git.executable_changes.is_none()
        || git.regular_file_changes.is_none()
    {
        reason(
            reasons,
            "git_capture_incomplete",
            None,
            AcceptanceAction::CaptureGit,
        );
    }
    if let (Some(binding), Some(target)) = (&git.binding, &git.target_sha) {
        if binding.head_sha != *target {
            reason(
                reasons,
                "git_candidate_changed",
                None,
                AcceptanceAction::CaptureGit,
            );
        }
    }
    if let crate::verification::change::GitChangeTarget::Commit { revision } = &git.target {
        if !full_oid(revision)
            || git.target_sha.as_ref() != Some(revision)
            || git.binding.as_ref().is_none_or(|binding| binding.dirty)
        {
            reason(
                reasons,
                "git_commit_binding_incomplete",
                None,
                AcceptanceAction::CaptureGit,
            );
        }
    }
    let (code, action) = match input.policy_state {
        AcceptancePolicyState::Active => {
            ("policy_binding_missing", AcceptanceAction::ActivatePolicy)
        }
        AcceptancePolicyState::Inactive => ("policy_inactive", AcceptanceAction::ActivatePolicy),
        AcceptancePolicyState::Revoked => ("policy_revoked", AcceptanceAction::ActivatePolicy),
        AcceptancePolicyState::Expired => ("policy_expired", AcceptanceAction::ActivatePolicy),
        AcceptancePolicyState::StaleDefinition => {
            ("policy_definition_changed", AcceptanceAction::RefreshPolicy)
        }
        AcceptancePolicyState::Unavailable => {
            ("policy_unavailable", AcceptanceAction::RefreshPolicy)
        }
    };
    if input.policy_state != AcceptancePolicyState::Active || input.policy.is_none() {
        reason(reasons, code, None, action);
    }
    if let Some(policy) = input.policy {
        if policy.workspace != input.workspace || policy.root_digest != input.current_root_digest {
            reason(
                reasons,
                "policy_workspace_mismatch",
                None,
                AcceptanceAction::ActivatePolicy,
            );
        }
        if policy.generation != policy.current_generation
            || policy.snapshot_digest != policy.current_snapshot_digest
        {
            reason(
                reasons,
                "policy_generation_changed",
                None,
                AcceptanceAction::RefreshPolicy,
            );
        }
        if policy.source_seal_digest != policy.current_source_seal_digest {
            reason(
                reasons,
                "policy_definition_changed",
                None,
                AcceptanceAction::RefreshPolicy,
            );
        }
        if policy
            .expires_at_ms
            .is_some_and(|expiry| input.now_ms >= expiry)
        {
            reason(
                reasons,
                "policy_expired",
                None,
                AcceptanceAction::ActivatePolicy,
            );
        }
        if policy.required_checks.is_empty() {
            reason(
                reasons,
                "policy_native_matrix_missing",
                None,
                AcceptanceAction::ResolveDiscovery,
            );
        }
    }
}
pub(in crate::verification::acceptance) fn plan_reasons(
    input: &AcceptanceInput<'_>,
    status: &VerificationStatus,
    reasons: &mut Vec<AcceptanceReason>,
) {
    let plan = &status.plan;
    if plan.revision.as_ref() != Some(input.revision) {
        reason(
            reasons,
            "plan_revision_changed",
            None,
            AcceptanceAction::RefreshRevision,
        );
    }
    if plan.subject != format!("change:{}", input.revision.code) {
        reason(
            reasons,
            "plan_subject_unbound",
            None,
            AcceptanceAction::PlanVerification,
        );
    }
    if !plan.revision.as_ref().is_some_and(|revision| {
        hash(&revision.code) && revision.design.as_deref().is_some_and(hash)
    }) {
        reason(
            reasons,
            "plan_revision_unbound",
            None,
            AcceptanceAction::PlanVerification,
        );
    }
    if let Some(policy) = input.policy {
        if plan.policy != policy.plan_policy() {
            reason(
                reasons,
                "plan_policy_changed",
                None,
                AcceptanceAction::PlanVerification,
            );
        }
        let requirements = &policy.selection.requirements;
        let profile = VerificationProfile::for_risk(input.risk_level);
        let stages = &requirements.stages;
        let human = requirements.human_approval
            || requirements
                .human_approval_min_risk
                .is_some_and(|risk| input.risk_level >= risk)
            || profile.require_human_approval;
        if plan.risk_level < input.risk_level
            || plan.deterministic_level != minimum_level(input)
                && plan.deterministic_level != "full"
            || !requirements
                .reviewers
                .iter()
                .all(|role| plan.reviewer_roles.contains(role))
            || !crate::verification::reviewer_roles(&profile)
                .iter()
                .all(|role| plan.reviewer_roles.contains(role))
            || (profile.require_property || stages.contains(&VerificationStage::Property))
                && !plan.require_property
            || (profile.require_mutation || stages.contains(&VerificationStage::Mutation))
                && !plan.require_mutation
            || (profile.require_fuzz || stages.contains(&VerificationStage::Fuzz))
                && !plan.require_fuzz
            || (stages.contains(&VerificationStage::RuntimeCanary)
                || profile
                    .deterministic_checks
                    .iter()
                    .any(|check| check == "runtime-gate"))
                && !plan
                    .deterministic_checks
                    .iter()
                    .any(|check| check == "runtime-gate")
            || human && !plan.require_human_approval
        {
            reason(
                reasons,
                "plan_requirements_incomplete",
                None,
                AcceptanceAction::PlanVerification,
            );
        }
    }
    if !plan.automation_gaps.is_empty() {
        reason(
            reasons,
            "plan_automation_gap",
            None,
            AcceptanceAction::ResolveDiscovery,
        );
    }
}
pub(in crate::verification::acceptance) fn verification_reasons(
    input: &AcceptanceInput<'_>,
    status: &VerificationStatus,
    reasons: &mut Vec<AcceptanceReason>,
) -> Result<()> {
    for blocker in &status.blockers {
        let action = if blocker.contains("failed") || blocker == "reviewer-failure" {
            AcceptanceAction::InspectFailure
        } else if blocker.starts_with("reviewer-") {
            AcceptanceAction::RequestReview
        } else if blocker == "human-approval-required" {
            AcceptanceAction::RequestHumanApproval
        } else if blocker.contains("revision") {
            AcceptanceAction::RefreshRevision
        } else {
            AcceptanceAction::RunVerification
        };
        reason(reasons, blocker, None, action);
    }
    let plan = &status.plan;
    for (required, kind, label, stage) in [
        (
            plan.require_property,
            EvidenceKind::Property,
            "property",
            VerificationStage::Property,
        ),
        (
            plan.require_mutation,
            EvidenceKind::Mutation,
            "mutation",
            VerificationStage::Mutation,
        ),
        (
            plan.require_fuzz,
            EvidenceKind::Fuzz,
            "fuzz",
            VerificationStage::Fuzz,
        ),
        (
            plan.deterministic_checks
                .iter()
                .any(|check| check == "runtime-gate"),
            EvidenceKind::Runtime,
            "runtime_canary",
            VerificationStage::RuntimeCanary,
        ),
    ] {
        if !required {
            continue;
        }
        let expected_policy = format!("{}/stage/{stage:?}", plan.policy).to_ascii_lowercase();
        let records = input.evidence.iter().filter(|record| {
            record.revision == *input.revision
                && record.subject == plan.subject
                && record.kind == kind
                && record.authority == EvidenceAuthority::NativeStage
                && record.policy.as_deref() == Some(&expected_policy)
                && record.execution_git_binding.as_ref() == input.git.binding.as_ref()
                && input.git.binding.is_some()
                && input.policy.is_some_and(|policy| {
                    record.execution_policy_binding.as_deref()
                        == Some(policy.plan_policy().as_str())
                })
        });
        let latest = crate::evidence::latest_current(records, input.revision);
        let targets: Vec<Option<&str>> = if plan.stage_targets.is_empty() {
            vec![None]
        } else {
            plan.stage_targets
                .iter()
                .map(|target| Some(target.as_str()))
                .collect()
        };
        for target in targets {
            let result = latest
                .iter()
                .filter(|record| {
                    target
                        .is_none_or(|target| record.targets.iter().any(|covered| covered == target))
                })
                .map(|record| record.result)
                .max_by_key(|result| crate::evidence::result_severity(*result));
            match result {
                Some(EvidenceResult::Pass) => {}
                Some(EvidenceResult::Fail) => reason(
                    reasons,
                    "native_stage_failed",
                    Some(label),
                    AcceptanceAction::InspectFailure,
                ),
                Some(EvidenceResult::Disagree | EvidenceResult::Inconclusive) => reason(
                    reasons,
                    "native_stage_inconclusive",
                    Some(label),
                    AcceptanceAction::RequestReview,
                ),
                None => reason(
                    reasons,
                    "native_stage_binding_missing",
                    Some(label),
                    AcceptanceAction::RunVerification,
                ),
            }
        }
    }
    if plan.require_human_approval {
        let policy = format!(
            "human-approval/v1/sha256:{:x}",
            Sha256::digest(serde_json::to_vec(plan)?)
        );
        let records = input
            .evidence
            .iter()
            .filter(|record| {
                record.revision == *input.revision
                    && record.subject == plan.subject
                    && record.kind == EvidenceKind::HumanApproval
                    && record.authority == EvidenceAuthority::LocalOperator
                    && record.policy.as_deref() == Some(&policy)
                    && record.execution_git_binding.as_ref() == input.git.binding.as_ref()
                    && input.git.binding.is_some()
                    && input.policy.is_some_and(|policy| {
                        record.execution_policy_binding.as_deref()
                            == Some(policy.plan_policy().as_str())
                    })
            })
            .collect::<Vec<_>>();
        let newest = records.iter().map(|record| record.timestamp_ms).max();
        let result = records
            .iter()
            .filter(|record| Some(record.timestamp_ms) == newest)
            .map(|record| record.result)
            .max_by_key(|result| crate::evidence::result_severity(*result));
        if result != Some(EvidenceResult::Pass) {
            reason(
                reasons,
                "human_approval_binding_missing",
                None,
                AcceptanceAction::RequestHumanApproval,
            );
        }
    }
    Ok(())
}
