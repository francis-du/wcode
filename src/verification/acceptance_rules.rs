use super::*;
use crate::evidence::{EvidenceAuthority, EvidenceKind};
use crate::risk::VerificationProfile;
use crate::verification::{change::full_oid, VerificationStage};
use std::io::{self, Write};

#[path = "acceptance_reasons.rs"]
mod acceptance_reasons;
pub(super) use acceptance_reasons::{context_reasons, plan_reasons, verification_reasons};

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
pub(crate) fn gate_evidence_ids(
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
