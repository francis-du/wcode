use crate::evidence::{Confidence, Evidence, EvidenceResult, Revision};
use crate::graph::NodeId;
use crate::risk::{RiskLevel, VerificationProfile};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_VERIFICATION_JOBS: usize = 256;
const MAX_REVIEW_GUIDANCE_ITEMS: usize = 16;
const MAX_REVIEW_GUIDANCE_CHARS: usize = 1_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewerRole {
    DesignCompliance,
    Correctness,
    Maintainability,
    Architecture,
    Security,
    Performance,
    Compatibility,
    Adversarial,
    TestSynthesis,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewVerdict {
    Pass,
    Fail,
    Inconclusive,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationStage {
    Property,
    Mutation,
    Fuzz,
    RuntimeCanary,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StageSubmission {
    pub stage: VerificationStage,
    pub producer: String,
    pub verdict: ReviewVerdict,
    pub summary: String,
    pub artifact_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl StageSubmission {
    pub fn validate(&self) -> Result<(), VerificationError> {
        if self.producer.trim().is_empty()
            || self.producer.len() > 256
            || self.summary.trim().is_empty()
            || self.summary.chars().count() > 2_000
            || self.artifact_digest.trim().is_empty()
            || self.artifact_digest.len() > 512
            || self
                .model
                .as_ref()
                .is_some_and(|model| model.trim().is_empty() || model.len() > 256)
        {
            return Err(VerificationError::InvalidSubmission);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationJobStatus {
    Queued,
    Claimed,
    Submitted,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewSubmission {
    pub verdict: ReviewVerdict,
    pub summary: String,
    #[serde(default)]
    pub claims: Vec<String>,
    #[serde(default)]
    pub risks: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl ReviewSubmission {
    pub fn validate(&self) -> Result<(), VerificationError> {
        if self.summary.trim().is_empty()
            || self.summary.chars().count() > 2_000
            || self.claims.len() > 32
            || self.risks.len() > 32
            || self
                .claims
                .iter()
                .chain(&self.risks)
                .any(|value| value.trim().is_empty() || value.chars().count() > 1_000)
            || self
                .model
                .as_ref()
                .is_some_and(|model| model.trim().is_empty() || model.len() > 256)
        {
            return Err(VerificationError::InvalidSubmission);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VerificationJob {
    pub id: String,
    pub plan_id: String,
    pub workspace: String,
    pub subject: NodeId,
    pub role: ReviewerRole,
    pub required_capabilities: Vec<String>,
    #[serde(default)]
    pub guidance: Vec<String>,
    pub blind: bool,
    pub status: VerificationJobStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub claimed_by: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub submission: Option<ReviewSubmission>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct VerificationPlan {
    pub id: String,
    pub workspace: String,
    pub subject: NodeId,
    pub risk_level: RiskLevel,
    pub policy: String,
    pub deterministic_level: String,
    pub deterministic_checks: Vec<String>,
    pub reviewer_roles: Vec<ReviewerRole>,
    pub require_property: bool,
    pub require_mutation: bool,
    pub require_fuzz: bool,
    pub require_human_approval: bool,
    #[serde(default)]
    pub automation_gaps: Vec<String>,
    pub job_ids: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct VerificationStatus {
    pub plan: VerificationPlan,
    pub queued: usize,
    pub claimed: usize,
    pub submitted: usize,
    pub reviewer_failures: usize,
    pub reviewer_inconclusive: usize,
    pub disagreements: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deterministic_result: Option<EvidenceResult>,
    pub stage_results: BTreeMap<String, EvidenceResult>,
    pub stage_producer_results: BTreeMap<String, BTreeMap<String, EvidenceResult>>,
    pub human_approval: bool,
    pub ready: bool,
    pub blockers: Vec<String>,
    pub jobs: Vec<VerificationJob>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct VerificationState {
    plans: BTreeMap<String, VerificationPlan>,
    jobs: BTreeMap<String, VerificationJob>,
}

impl VerificationState {
    pub fn plans_for_workspace(&self, workspace: &str) -> Vec<VerificationPlan> {
        let mut plans = self
            .plans
            .values()
            .filter(|plan| plan.workspace == workspace)
            .cloned()
            .collect::<Vec<_>>();
        plans.sort_by(|left, right| left.id.cmp(&right.id));
        plans
    }

    pub fn workspace_snapshot(&self, workspace: &str) -> Self {
        let plans = self
            .plans
            .iter()
            .filter(|(_, plan)| plan.workspace == workspace)
            .map(|(id, plan)| (id.clone(), plan.clone()))
            .collect::<BTreeMap<_, _>>();
        let jobs = self
            .jobs
            .iter()
            .filter(|(_, job)| job.workspace == workspace)
            .map(|(id, job)| (id.clone(), job.clone()))
            .collect::<BTreeMap<_, _>>();
        Self { plans, jobs }
    }

    pub fn restore_workspace(&mut self, snapshot: Self) -> Result<(), VerificationError> {
        if snapshot.plans.len() > MAX_VERIFICATION_JOBS
            || snapshot.jobs.len() > MAX_VERIFICATION_JOBS
        {
            return Err(VerificationError::CapacityExceeded);
        }
        for plan in snapshot.plans.values() {
            if plan.id.trim().is_empty()
                || plan.workspace.trim().is_empty()
                || plan.subject.trim().is_empty()
                || plan.job_ids.len() > MAX_VERIFICATION_JOBS
            {
                return Err(VerificationError::InvalidPersistedState);
            }
        }
        for job in snapshot.jobs.values() {
            if job.id.trim().is_empty()
                || job.plan_id.trim().is_empty()
                || job.workspace.trim().is_empty()
                || job.guidance.len() > MAX_REVIEW_GUIDANCE_ITEMS
                || job.guidance.iter().any(|item| {
                    item.trim().is_empty() || item.chars().count() > MAX_REVIEW_GUIDANCE_CHARS
                })
                || !snapshot.plans.contains_key(&job.plan_id)
            {
                return Err(VerificationError::InvalidPersistedState);
            }
        }
        self.plans.extend(snapshot.plans);
        self.jobs.extend(snapshot.jobs);
        Ok(())
    }

    pub fn create_plan(
        &mut self,
        plan_id: String,
        workspace: String,
        subject: NodeId,
        risk_level: RiskLevel,
        job_ids: impl Iterator<Item = String>,
    ) -> Result<VerificationPlan, VerificationError> {
        if self.plans.contains_key(&plan_id) {
            return Err(VerificationError::DuplicatePlan);
        }
        let profile = VerificationProfile::for_risk(risk_level);
        let roles = reviewer_roles(&profile);
        let ids = job_ids.take(roles.len()).collect::<Vec<_>>();
        if ids.len() != roles.len()
            || self.jobs.len().saturating_add(ids.len()) > MAX_VERIFICATION_JOBS
        {
            return Err(VerificationError::CapacityExceeded);
        }
        let deterministic_level = if risk_level >= RiskLevel::Medium {
            "full"
        } else {
            "quick"
        };
        // Retained for wire compatibility with older clients. Required stage work is now
        // represented by explicit require_* flags and satisfied by verification_stage_submit
        // Evidence, so this legacy field must not advertise phantom missing executors.
        let automation_gaps = Vec::new();
        let plan = VerificationPlan {
            id: plan_id.clone(),
            workspace: workspace.clone(),
            subject: subject.clone(),
            risk_level,
            policy: format!("risk-adaptive/v1/{risk_level:?}").to_ascii_lowercase(),
            deterministic_level: deterministic_level.to_owned(),
            deterministic_checks: profile.deterministic_checks.clone(),
            reviewer_roles: roles.clone(),
            require_property: profile.require_property,
            require_mutation: profile.require_mutation,
            require_fuzz: profile.require_fuzz,
            require_human_approval: profile.require_human_approval,
            automation_gaps,
            job_ids: ids.clone(),
        };
        for (id, role) in ids.into_iter().zip(roles) {
            self.jobs.insert(
                id.clone(),
                VerificationJob {
                    id,
                    plan_id: plan_id.clone(),
                    workspace: workspace.clone(),
                    subject: subject.clone(),
                    role,
                    required_capabilities: role_capabilities(role),
                    guidance: role_guidance(role),
                    blind: true,
                    status: VerificationJobStatus::Queued,
                    claimed_by: None,
                    submission: None,
                },
            );
        }
        self.plans.insert(plan_id, plan.clone());
        Ok(plan)
    }

    pub fn claim(
        &mut self,
        workspace: &str,
        reviewer: &str,
        capabilities: &BTreeSet<String>,
        requested_role: Option<ReviewerRole>,
    ) -> Result<VerificationJob, VerificationError> {
        if reviewer.trim().is_empty() || reviewer.len() > 256 {
            return Err(VerificationError::InvalidReviewer);
        }
        let Some(job) = self.jobs.values_mut().find(|job| {
            job.workspace == workspace
                && job.status == VerificationJobStatus::Queued
                && requested_role.is_none_or(|role| role == job.role)
                && job
                    .required_capabilities
                    .iter()
                    .all(|required| capabilities.contains(required))
        }) else {
            return Err(VerificationError::NoMatchingJob);
        };
        job.status = VerificationJobStatus::Claimed;
        job.claimed_by = Some(reviewer.to_owned());
        Ok(job.clone())
    }

    pub fn submit(
        &mut self,
        workspace: &str,
        job_id: &str,
        reviewer: &str,
        submission: ReviewSubmission,
    ) -> Result<VerificationJob, VerificationError> {
        submission.validate()?;
        let job = self
            .jobs
            .get_mut(job_id)
            .ok_or(VerificationError::UnknownJob)?;
        if job.workspace != workspace
            || job.status != VerificationJobStatus::Claimed
            || job.claimed_by.as_deref() != Some(reviewer)
        {
            return Err(VerificationError::InvalidJobState);
        }
        job.status = VerificationJobStatus::Submitted;
        job.submission = Some(submission);
        Ok(job.clone())
    }

    pub fn status(&self, plan_id: &str) -> Result<VerificationStatus, VerificationError> {
        let plan = self
            .plans
            .get(plan_id)
            .cloned()
            .ok_or(VerificationError::UnknownPlan)?;
        let jobs = plan
            .job_ids
            .iter()
            .filter_map(|id| self.jobs.get(id).cloned())
            .collect::<Vec<_>>();
        let queued = jobs
            .iter()
            .filter(|job| job.status == VerificationJobStatus::Queued)
            .count();
        let claimed = jobs
            .iter()
            .filter(|job| job.status == VerificationJobStatus::Claimed)
            .count();
        let submitted = jobs
            .iter()
            .filter(|job| job.status == VerificationJobStatus::Submitted)
            .count();
        let verdicts = jobs
            .iter()
            .filter_map(|job| job.submission.as_ref().map(|submission| submission.verdict))
            .collect::<Vec<_>>();
        let reviewer_failures = verdicts
            .iter()
            .filter(|verdict| **verdict == ReviewVerdict::Fail)
            .count();
        let reviewer_inconclusive = verdicts
            .iter()
            .filter(|verdict| **verdict == ReviewVerdict::Inconclusive)
            .count();
        let disagreements =
            usize::from(verdicts.len() > 1 && verdicts.windows(2).any(|pair| pair[0] != pair[1]));
        let mut blockers = Vec::new();
        if queued > 0 || claimed > 0 {
            blockers.push("reviewer-jobs-incomplete".to_owned());
        }
        if reviewer_failures > 0 {
            blockers.push("reviewer-failure".to_owned());
        }
        if reviewer_inconclusive > 0 {
            blockers.push("reviewer-inconclusive".to_owned());
        }
        if disagreements > 0 {
            blockers.push("reviewer-disagreement".to_owned());
        }
        Ok(VerificationStatus {
            plan,
            queued,
            claimed,
            submitted,
            reviewer_failures,
            reviewer_inconclusive,
            disagreements,
            deterministic_result: None,
            stage_results: BTreeMap::new(),
            stage_producer_results: BTreeMap::new(),
            human_approval: false,
            ready: false,
            blockers,
            jobs,
        })
    }

    pub fn evidence_for_submission(
        job: &VerificationJob,
        evidence_id: String,
        revision: Revision,
        policy: String,
    ) -> Result<Evidence, VerificationError> {
        let submission = job
            .submission
            .as_ref()
            .ok_or(VerificationError::InvalidJobState)?;
        let result = match submission.verdict {
            ReviewVerdict::Pass => EvidenceResult::Pass,
            ReviewVerdict::Fail => EvidenceResult::Fail,
            ReviewVerdict::Inconclusive => EvidenceResult::Inconclusive,
        };
        let mut evidence = Evidence::new(
            evidence_id,
            job.subject.clone(),
            crate::evidence::EvidenceKind::ModelReview,
            job.claimed_by
                .clone()
                .unwrap_or_else(|| "model-reviewer".to_owned()),
            revision,
            result,
            Confidence::High,
        )
        .map_err(|_| VerificationError::InvalidEvidence)?;
        evidence.model = submission.model.clone();
        evidence.policy = Some(policy);
        evidence.summary = Some(submission.summary.clone());
        evidence.claims = submission.claims.clone();
        evidence.risks = submission.risks.clone();
        evidence
            .validate()
            .map_err(|_| VerificationError::InvalidEvidence)?;
        Ok(evidence)
    }
}

fn reviewer_roles(profile: &VerificationProfile) -> Vec<ReviewerRole> {
    let mut roles = vec![ReviewerRole::Correctness];
    if profile.independent_reviewers >= 2 {
        roles.push(ReviewerRole::Maintainability);
    }
    if profile.independent_reviewers >= 3 {
        roles.push(ReviewerRole::Architecture);
    }
    if profile.level >= RiskLevel::High {
        roles.push(ReviewerRole::Security);
    }
    if profile.require_fuzz {
        roles.push(ReviewerRole::Adversarial);
    }
    roles
}

fn role_guidance(role: ReviewerRole) -> Vec<String> {
    match role {
        ReviewerRole::Maintainability => vec![
            "Do not approve only because behavior is correct; reject clear structural regressions.".to_owned(),
            "Look first for a code-judo restructuring that deletes branches, helpers, modes, or layers instead of rearranging them.".to_owned(),
            "Treat new ad-hoc special cases and scattered conditionals as design smells; prefer a canonical model, policy, state machine, helper, or module.".to_owned(),
            "Challenge wrappers, casts, optionality, and loosely shaped contracts that add indirection without clarifying the invariant.".to_owned(),
            "Keep feature logic in its canonical layer and reuse existing helpers rather than introducing bespoke duplicates.".to_owned(),
            "Treat a change that pushes a file from below 1,000 lines to above 1,000 as a presumptive decomposition blocker unless strongly justified.".to_owned(),
            "Prefer parallel orchestration for genuinely independent work and atomic flows for related state updates when that simplifies reasoning.".to_owned(),
            "Prioritize a small number of high-conviction structural findings over cosmetic nits.".to_owned(),
        ],
        _ => Vec::new(),
    }
}

fn role_capabilities(role: ReviewerRole) -> Vec<String> {
    let capability = match role {
        ReviewerRole::DesignCompliance => "design_review",
        ReviewerRole::Correctness => "correctness_review",
        ReviewerRole::Maintainability => "maintainability_review",
        ReviewerRole::Architecture => "architecture_review",
        ReviewerRole::Security => "security_review",
        ReviewerRole::Performance => "performance_review",
        ReviewerRole::Compatibility => "compatibility_review",
        ReviewerRole::Adversarial => "adversarial_review",
        ReviewerRole::TestSynthesis => "test_synthesis",
    };
    vec![capability.to_owned()]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum VerificationError {
    DuplicatePlan,
    CapacityExceeded,
    InvalidReviewer,
    NoMatchingJob,
    UnknownJob,
    UnknownPlan,
    InvalidJobState,
    InvalidSubmission,
    InvalidEvidence,
    InvalidPersistedState,
}

impl std::fmt::Display for VerificationError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let message = match self {
            Self::DuplicatePlan => "verification plan already exists",
            Self::CapacityExceeded => "verification job capacity exceeded",
            Self::InvalidReviewer => "reviewer identity is invalid",
            Self::NoMatchingJob => "no queued verification job matches the requested capabilities",
            Self::UnknownJob => "verification job does not exist",
            Self::UnknownPlan => "verification plan does not exist",
            Self::InvalidJobState => "verification job is not in the required state",
            Self::InvalidSubmission => "verification submission is invalid",
            Self::InvalidEvidence => "verification evidence could not be produced",
            Self::InvalidPersistedState => "persisted verification state is invalid",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for VerificationError {}

#[cfg(test)]
#[path = "../../tests/unit/verification/mod.rs"]
mod tests;
