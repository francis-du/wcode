use super::*;
use crate::evidence::{EvidenceAuthority, EvidenceKind};
use crate::risk::VerificationProfile;
use crate::verification::{change::full_oid, VerificationStage};
use std::io::{self, Write};

const MAX_BYTES: usize = 2 * 1024 * 1024;
struct BudgetWriter {
    bytes: usize,
    limit: usize,
}
impl Write for BudgetWriter {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        self.bytes = self
            .bytes
            .checked_add(data.len())
            .filter(|bytes| *bytes <= self.limit)
            .ok_or_else(|| io::Error::other("acceptance byte budget exceeded"))?;
        Ok(data.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
fn bounded_size(value: &impl Serialize, limit: usize) -> Result<usize> {
    let mut writer = BudgetWriter { bytes: 0, limit };
    serde_json::to_writer(&mut writer, value)?;
    Ok(writer.bytes)
}
pub(super) fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}
fn hash(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|value| {
        value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
    })
}
pub(super) fn validate_input(input: &AcceptanceInput<'_>) -> Result<()> {
    ensure!(text(input.workspace, 256), "invalid acceptance workspace");
    ensure!(
        text(&input.revision.code, 256)
            && input
                .revision
                .design
                .as_deref()
                .is_none_or(|value| text(value, 256)),
        "invalid acceptance Revision"
    );
    ensure!(
        hash(input.current_root_digest),
        "invalid acceptance root digest"
    );
    ensure!(
        input.discovery.checks.len() <= 1088
            && input
                .discovery
                .checks
                .iter()
                .all(RequiredVerificationCheck::valid),
        "invalid or oversized acceptance discovery"
    );
    let ids = input
        .discovery
        .checks
        .iter()
        .map(|check| &check.id)
        .collect::<BTreeSet<_>>();
    ensure!(
        ids.len() == input.discovery.checks.len(),
        "duplicate discovery check identity"
    );
    ensure!(
        input.git.changes.len() <= 4096
            && input.git.unknown_reasons.len() <= 64
            && text(&input.git.requested_base, 256)
            && input
                .git
                .unknown_reasons
                .iter()
                .all(|reason| text(reason, 256)),
        "invalid or oversized acceptance Git capture"
    );
    bounded_size(input.git, MAX_BYTES)?;
    for change in &input.git.changes {
        ensure!(
            text(&change.status, 16)
                && change
                    .old_path
                    .as_ref()
                    .into_iter()
                    .chain(change.new_path.as_ref())
                    .all(|path| super::super::change::changed_path(path).is_ok()),
            "invalid acceptance Git changed path"
        );
    }
    if let Some(policy) = input.policy {
        ensure!(
            text(&policy.workspace, 256)
                && hash(&policy.root_digest)
                && policy.generation > 0
                && policy.current_generation > 0
                && [
                    &policy.snapshot_digest,
                    &policy.current_snapshot_digest,
                    &policy.source_seal_digest,
                    &policy.current_source_seal_digest,
                    &policy.selection.policy_digest
                ]
                .into_iter()
                .all(|digest| hash(digest))
                && text(&policy.selection.policy_id, 160)
                && policy.selection.policy_version > 0
                && policy.selection.matched_rules.len() <= 64
                && policy
                    .selection
                    .matched_rules
                    .iter()
                    .all(|id| text(id, 160))
                && policy.required_checks.len() <= 32
                && policy
                    .required_checks
                    .iter()
                    .all(RequiredVerificationCheck::valid),
            "invalid acceptance Policy binding"
        );
        let requirements = &policy.selection.requirements;
        ensure!(
            requirements.checks.len() <= 32
                && requirements.stages.len() <= 4
                && requirements.reviewers.len() <= 9
                && requirements.checks.iter().all(|id| text(id, 160))
                && requirements.checks.iter().collect::<BTreeSet<_>>().len()
                    == requirements.checks.len()
                && requirements.reviewers.iter().collect::<BTreeSet<_>>().len()
                    == requirements.reviewers.len()
                && requirements
                    .stages
                    .iter()
                    .map(|stage| format!("{stage:?}"))
                    .collect::<BTreeSet<_>>()
                    .len()
                    == requirements.stages.len()
                && policy
                    .required_checks
                    .iter()
                    .map(|check| &check.id)
                    .collect::<BTreeSet<_>>()
                    .len()
                    == policy.required_checks.len(),
            "invalid acceptance Policy requirements"
        );
        bounded_size(policy, MAX_BYTES)?;
    }
    ensure!(
        input.verification.plans.len() <= super::super::MAX_VERIFICATION_JOBS
            && input.verification.jobs.len() <= super::super::MAX_VERIFICATION_JOBS,
        "acceptance Verification count budget exceeded"
    );
    bounded_size(input.verification, MAX_BYTES)?;
    ensure!(
        input.evidence.len() <= 4096,
        "acceptance Evidence count exceeds 4096"
    );
    let mut total = 0usize;
    let mut ids = BTreeMap::new();
    for record in input.evidence {
        record.validate()?;
        total = total
            .checked_add(bounded_size(record, 128 * 1024)?)
            .ok_or_else(|| anyhow::anyhow!("acceptance Evidence byte budget overflow"))?;
        ensure!(
            total <= 32 * 1024 * 1024,
            "acceptance Evidence exceeds 32 MiB"
        );
        let serialized = serde_json::to_vec(record)?;
        if let Some(prior) = ids.insert(&record.id, serialized.clone()) {
            ensure!(
                prior == serialized,
                "conflicting acceptance Evidence identity"
            );
        }
    }
    Ok(())
}
pub(super) fn reason(
    reasons: &mut Vec<AcceptanceReason>,
    code: &str,
    subject: Option<&str>,
    action: AcceptanceAction,
) {
    reasons.push(AcceptanceReason {
        code: code.into(),
        subject: subject.map(str::to_owned),
        action,
    });
}
pub(super) fn context_reasons(input: &AcceptanceInput<'_>, reasons: &mut Vec<AcceptanceReason>) {
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
    if let super::super::change::GitChangeTarget::Commit { revision } = &git.target {
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
pub(super) fn plan_reasons(
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
            || !super::super::reviewer_roles(&profile)
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
pub(super) fn verification_reasons(
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
pub(super) fn gate_evidence_ids(
    input: &AcceptanceInput<'_>,
    status: Option<&VerificationStatus>,
) -> Vec<String> {
    let Some(status) = status else {
        return Vec::new();
    };
    input
        .evidence
        .iter()
        .filter(|record| {
            record.revision == *input.revision
                && record.subject == status.plan.subject
                && matches!(
                    record.authority,
                    EvidenceAuthority::NativeVerification
                        | EvidenceAuthority::NativeStage
                        | EvidenceAuthority::LocalOperator
                )
        })
        .map(|record| record.id.clone())
        .collect()
}
pub(super) fn classify(reasons: &[AcceptanceReason]) -> AcceptanceState {
    if reasons.is_empty() {
        return AcceptanceState::Ready;
    }
    if reasons
        .iter()
        .any(|reason| reason.action == AcceptanceAction::InspectFailure)
    {
        return AcceptanceState::Blocked;
    }
    if reasons.iter().any(|reason| {
        matches!(
            reason.code.as_str(),
            "policy_inactive"
                | "policy_revoked"
                | "policy_expired"
                | "policy_unavailable"
                | "policy_binding_missing"
        )
    }) {
        return AcceptanceState::Incomplete;
    }
    if reasons.iter().any(|reason| {
        matches!(
            reason.code.as_str(),
            "policy_definition_changed"
                | "policy_generation_changed"
                | "plan_revision_changed"
                | "plan_policy_changed"
                | "git_candidate_changed"
                | "check_evidence_stale"
        ) || reason.code.ends_with("changed-since-plan")
    }) {
        return AcceptanceState::Stale;
    }
    if reasons.iter().all(|reason| {
        matches!(
            reason.action,
            AcceptanceAction::RequestReview | AcceptanceAction::RequestHumanApproval
        )
    }) {
        return AcceptanceState::NeedsReview;
    }
    AcceptanceState::Incomplete
}
