//! Read-only evaluation of a caller-supplied, trusted workspace snapshot.
//!
//! This contract reuses the native readiness engine. It does not execute checks,
//! persist state, authenticate producers, authorize an operator, or activate a
//! project policy. Evidence has no workspace field: the caller must obtain the
//! complete evidence slice from the selected workspace's trusted store. Imported
//! JSON, an authority label, and a digest alone do not establish that trust.
//!
//! Inputs are bounded to 256 plans/jobs, 2 MiB of serialized state, 4,096
//! Evidence records, 128 KiB per record, and 32 MiB of Evidence in total.
//! Legacy unbound plans remain inspectable but cannot become ready.

use super::{VerificationJobStatus, VerificationState, VerificationStatus, MAX_VERIFICATION_JOBS};
use crate::evidence::{Evidence, Revision};
use crate::intelligence::SoftwareIntelligenceRuntime;
use anyhow::{ensure, Context, Result};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{self, Write};

const MAX_STATE_BYTES: usize = 2 * 1024 * 1024;
const MAX_EVIDENCE_RECORDS: usize = 4_096;
const MAX_EVIDENCE_BYTES: usize = 128 * 1024;
const MAX_EVIDENCE_TOTAL_BYTES: usize = 32 * 1024 * 1024;

/// Evaluate native readiness without changing state or minting authority.
///
/// The caller supplies a complete, trusted workspace Evidence snapshot and a
/// freshly captured current Revision. Structural validation cannot prove their
/// provenance or completeness. Invalid or over-budget inputs return an error;
/// stale, partial, skipped, missing, and legacy proof remain non-ready.
pub fn evaluate_snapshot(
    state: &VerificationState,
    workspace: &str,
    plan_id: &str,
    current_revision: &Revision,
    evidence: &[Evidence],
) -> Result<VerificationStatus> {
    ensure!(
        bounded_text(workspace, 256) && bounded_text(plan_id, 160),
        "verification snapshot workspace or plan identity is invalid"
    );
    ensure!(
        bounded_revision(current_revision),
        "verification snapshot current Revision is invalid"
    );
    validate_state(state)?;
    let plan = state
        .plans
        .get(plan_id)
        .context("verification plan does not exist")?;
    ensure!(
        plan.workspace == workspace,
        "verification plan does not belong to the selected workspace"
    );
    validate_evidence(evidence)?;
    let status = state.status(plan_id)?;
    SoftwareIntelligenceRuntime::verification_status_from_snapshot(
        status,
        current_revision,
        evidence,
    )
}

fn validate_state(state: &VerificationState) -> Result<()> {
    ensure!(
        state.plans.len() <= MAX_VERIFICATION_JOBS
            && state.jobs.len() <= MAX_VERIFICATION_JOBS
            && state.plan_order.len() <= MAX_VERIFICATION_JOBS,
        "verification snapshot state count exceeds its bounded capacity"
    );
    serialized_size(state, MAX_STATE_BYTES)
        .context("verification snapshot state exceeds bounded serialization limit")?;
    // Preserve the native loader's validation without mutating the input. The
    // public boundary adds relational checks that legacy history loading lacks.
    VerificationState::default().restore_workspace(state.clone())?;
    let mut referenced_jobs = BTreeSet::new();
    for (key, plan) in &state.plans {
        ensure!(
            key == &plan.id
                && bounded_text(&plan.id, 160)
                && bounded_text(&plan.workspace, 256)
                && bounded_text(&plan.subject, 512)
                && bounded_text(&plan.policy, 256)
                && matches!(plan.deterministic_level.as_str(), "quick" | "full")
                && plan.revision.as_ref().is_none_or(bounded_revision)
                && plan.deterministic_checks.len() <= 32
                && plan
                    .deterministic_checks
                    .iter()
                    .all(|id| bounded_text(id, 160)),
            "verification snapshot plan identity or metadata is invalid"
        );
        ensure!(
            !plan.reviewer_roles.is_empty()
                && plan.reviewer_roles.len() <= 9
                && plan.reviewer_roles.iter().collect::<BTreeSet<_>>().len()
                    == plan.reviewer_roles.len()
                && plan.job_ids.len() == plan.reviewer_roles.len(),
            "verification snapshot plan reviewer/job contract is invalid"
        );
        for (id, role) in plan.job_ids.iter().zip(&plan.reviewer_roles) {
            ensure!(
                bounded_text(id, 160) && referenced_jobs.insert(id.as_str()),
                "verification snapshot contains a duplicate or invalid job reference"
            );
            let job = state
                .jobs
                .get(id)
                .context("verification snapshot is missing a required job")?;
            ensure!(
                job.id == *id
                    && job.plan_id == plan.id
                    && job.workspace == plan.workspace
                    && job.subject == plan.subject
                    && job.role == *role,
                "verification snapshot plan/job binding is inconsistent"
            );
        }
    }
    for (key, job) in &state.jobs {
        ensure!(
            key == &job.id
                && referenced_jobs.contains(key.as_str())
                && bounded_text(&job.id, 160)
                && bounded_text(&job.plan_id, 160)
                && bounded_text(&job.workspace, 256)
                && bounded_text(&job.subject, 512)
                && job.blind
                && job.required_capabilities.len() <= 32
                && job
                    .required_capabilities
                    .iter()
                    .all(|id| bounded_text(id, 160))
                && super::role_capabilities(job.role)
                    .iter()
                    .all(|id| job.required_capabilities.contains(id)),
            "verification snapshot job identity or reviewer contract is invalid"
        );
        let claimed = job
            .claimed_by
            .as_deref()
            .is_some_and(|actor| bounded_text(actor, 256));
        let consistent = match job.status {
            VerificationJobStatus::Queued => job.claimed_by.is_none() && job.submission.is_none(),
            VerificationJobStatus::Claimed => claimed && job.submission.is_none(),
            VerificationJobStatus::Submitted => claimed && job.submission.is_some(),
        };
        ensure!(
            consistent,
            "verification snapshot job status/submission is inconsistent"
        );
        if let Some(submission) = &job.submission {
            submission.validate()?;
        }
    }
    Ok(())
}

fn validate_evidence(evidence: &[Evidence]) -> Result<()> {
    ensure!(
        evidence.len() <= MAX_EVIDENCE_RECORDS,
        "verification snapshot Evidence count exceeds 4096"
    );
    let mut total = 0usize;
    let mut identities = BTreeMap::new();
    for record in evidence {
        record.validate()?;
        let bytes = serialized_size(record, MAX_EVIDENCE_BYTES)
            .context("verification snapshot Evidence record exceeds bounded serialization limit")?;
        total = total
            .checked_add(bytes)
            .context("verification snapshot Evidence size overflow")?;
        ensure!(
            total <= MAX_EVIDENCE_TOTAL_BYTES,
            "verification snapshot Evidence total exceeds bounded serialization limit"
        );
        if let Some(previous) = identities.insert(&record.id, record) {
            ensure!(
                previous == record,
                "verification snapshot contains conflicting Evidence with the same id"
            );
        }
    }
    Ok(())
}

fn bounded_revision(revision: &Revision) -> bool {
    bounded_text(&revision.code, 256)
        && revision
            .design
            .as_deref()
            .is_none_or(|value| bounded_text(value, 256))
}

fn bounded_text(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}

// Count bounded serialization without allocating a second copy of large input.
// serde_json stops writing as soon as the caller's limit would be exceeded.
fn serialized_size(value: &impl Serialize, limit: usize) -> Result<usize> {
    struct Counter {
        used: usize,
        limit: usize,
    }
    impl Write for Counter {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() > self.limit.saturating_sub(self.used) {
                return Err(io::Error::other("bounded serialization limit exceeded"));
            }
            self.used += bytes.len();
            Ok(bytes.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    let mut counter = Counter { used: 0, limit };
    serde_json::to_writer(&mut counter, value)?;
    Ok(counter.used)
}

#[cfg(test)]
#[path = "../../tests/unit/verification/contract.rs"]
mod tests;
