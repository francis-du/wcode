//! Deterministic OSS Change Acceptance from complete, trusted native snapshots.
//!
//! This evaluator neither executes checks nor authenticates imported Evidence.
//! Callers must supply the selected workspace's complete native state, Evidence,
//! approved Policy selection and fresh Git/Code/Design identities. Authority
//! labels and hashes in imported JSON do not establish trust. Only the separate
//! native capture boundary may mint an opaque publishable acceptance receipt.

use super::{change::GitChangeSnapshot, VerificationState, VerificationStatus};
use crate::design::{PolicyLevel, PolicySelection};
use crate::evidence::{Evidence, EvidenceResult, RequiredVerificationCheck, Revision};
use crate::risk::RiskLevel;
use anyhow::{ensure, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[path = "acceptance_checks.rs"]
mod checks;
pub use checks::AcceptanceEvidenceSummary;
#[path = "acceptance_timing.rs"]
mod timing;
pub use timing::{execution_duration_metrics, AcceptanceCheckTiming};
#[path = "acceptance_rules.rs"]
mod rules;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceState {
    Ready,
    Blocked,
    NeedsReview,
    Incomplete,
    Stale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptancePolicyState {
    Active,
    Inactive,
    Revoked,
    Expired,
    StaleDefinition,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceAction {
    CaptureContext,
    ActivatePolicy,
    RefreshPolicy,
    CaptureGit,
    PlanVerification,
    RunVerification,
    InspectFailure,
    RequestReview,
    RequestHumanApproval,
    RefreshRevision,
    ResolveDiscovery,
}

/// An approved native Policy capture, not an approval supplied by a model.
/// Required bindings include the native minimum-level matrix, not just IDs
/// named explicitly by a Policy branch. Current values are independently
/// captured by the caller, never copied from the approved historical snapshot.
#[derive(Clone, Debug, Serialize)]
pub struct AcceptancePolicyBinding {
    pub workspace: String,
    pub root_digest: String,
    pub generation: u64,
    pub current_generation: u64,
    pub snapshot_digest: String,
    pub current_snapshot_digest: String,
    pub source_seal_digest: String,
    pub current_source_seal_digest: String,
    pub selection: PolicySelection,
    pub required_checks: Vec<RequiredVerificationCheck>,
    pub expires_at_ms: Option<u64>,
}

impl AcceptancePolicyBinding {
    pub fn plan_policy(&self) -> String {
        format!(
            "project-policy/v1/{}/{}",
            self.generation, self.snapshot_digest
        )
    }
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct AcceptanceDiscovery {
    pub complete: bool,
    pub mappings_complete: bool,
    pub checks: Vec<RequiredVerificationCheck>,
}

/// Complete trusted input; no pre-filled readiness or client approval fields.
/// The evaluator is read-only. Missing authority/plan is an inspectable
/// incomplete Record; malformed or over-budget snapshots return an error.
pub struct AcceptanceInput<'a> {
    pub workspace: &'a str,
    pub current_root_digest: &'a str,
    pub revision: &'a Revision,
    pub git: &'a GitChangeSnapshot,
    pub policy_state: AcceptancePolicyState,
    pub policy: Option<&'a AcceptancePolicyBinding>,
    pub verification: &'a VerificationState,
    pub plan_id: Option<&'a str>,
    pub evidence: &'a [Evidence],
    pub discovery: &'a AcceptanceDiscovery,
    pub risk_level: RiskLevel,
    pub now_ms: u64,
}

#[derive(Clone, Debug, Serialize)]
pub struct AcceptanceReason {
    pub code: String,
    pub subject: Option<String>,
    pub action: AcceptanceAction,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceExecution {
    Unknown,
    Executed,
    Skipped,
    Unavailable,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceOutcome {
    Unknown,
    Pass,
    Fail,
    Inconclusive,
    Disagree,
}

impl From<EvidenceResult> for AcceptanceOutcome {
    fn from(result: EvidenceResult) -> Self {
        match result {
            EvidenceResult::Pass => Self::Pass,
            EvidenceResult::Fail => Self::Fail,
            EvidenceResult::Inconclusive => Self::Inconclusive,
            EvidenceResult::Disagree => Self::Disagree,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceFreshness {
    Current,
    Stale,
    Unbound,
    Missing,
}

#[derive(Clone, Debug, Serialize)]
pub struct AcceptanceCheck {
    pub id: String,
    pub signature: Option<String>,
    pub required: bool,
    pub discovered: bool,
    pub mapped: bool,
    pub execution: AcceptanceExecution,
    pub outcome: AcceptanceOutcome,
    pub freshness: AcceptanceFreshness,
    pub level: Option<String>,
    pub required_level: String,
    pub evidence_ids: Vec<String>,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct AcceptanceSummary {
    pub required: usize,
    pub discovered: usize,
    pub mapped: usize,
    pub executed: usize,
    pub passed: usize,
    pub failed: usize,
    pub skipped: usize,
    pub unavailable: usize,
    pub stale: usize,
    pub unknown: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct AcceptancePlanSummary {
    pub id: String,
    pub verification_generation: u64,
    pub policy: String,
    pub deterministic_level: String,
    pub revision: Option<Revision>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AcceptanceVerificationSummary {
    pub deterministic_result: Option<EvidenceResult>,
    pub stage_results: BTreeMap<String, EvidenceResult>,
    pub human_approval: Option<EvidenceResult>,
    pub queued: usize,
    pub claimed: usize,
    pub submitted: usize,
    pub reviewer_failures: usize,
    pub reviewer_inconclusive: usize,
    pub disagreements: usize,
}

/// Bounded native inspection metadata, never an execution or acceptance verdict.
/// Symbols are selected by membership in candidate paths; their bodies have not
/// been compared with the baseline. Counts describe the captured graph/trace
/// inventory and may be lower bounds when the underlying scan is incomplete.
#[derive(Clone, Debug, Serialize)]
pub struct AcceptanceChangeInspection {
    pub workspace: String,
    pub revision: Revision,
    pub base_sha: Option<String>,
    pub target_sha: Option<String>,
    pub git_binding: Option<super::change::ExecutionGitBinding>,
    pub producer: String,
    pub symbols: Vec<AcceptanceInspectedSymbol>,
    pub impacted_components: Vec<String>,
    pub impacted_requirements: Vec<String>,
    pub impacted_acceptance: Vec<String>,
    pub mapped_verification: Vec<AcceptanceMappedVerification>,
    pub impact: AcceptanceImpactSummary,
    pub risk: AcceptanceRiskSummary,
    pub coverage: AcceptanceInspectionCoverage,
    pub unknown_reasons: Vec<String>,
    pub recommended_actions: Vec<AcceptanceAction>,
    pub truncated: bool,
    pub complete: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceSymbolSelection {
    FileMembership,
}

#[derive(Clone, Debug, Serialize)]
pub struct AcceptanceInspectedSymbol {
    pub id: String,
    pub path: String,
    pub name: String,
    pub provider: String,
    pub precision: crate::graph::GraphPrecision,
    pub selection: AcceptanceSymbolSelection,
    pub freshness: AcceptanceFreshness,
    pub start_line: Option<usize>,
    pub end_line: Option<usize>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AcceptanceMappingKind {
    Test,
    Check,
}

#[derive(Clone, Debug, Serialize)]
pub struct AcceptanceMappedVerification {
    pub owner: String,
    pub target: String,
    pub kind: AcceptanceMappingKind,
    pub resolved: bool,
    pub provider: String,
    pub precision: String,
    /// Always declared_verification; resolution is not execution or Pass.
    pub relation: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct AcceptanceImpactSummary {
    pub graph_provider: String,
    pub graph_precision: String,
    pub graph_truncated: bool,
    pub transitive_callers: usize,
    pub public_api: bool,
    pub security_boundary: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct AcceptanceRiskFinding {
    pub id: String,
    pub category: crate::risk::RiskCategory,
    pub level: RiskLevel,
}

#[derive(Clone, Debug, Serialize)]
pub struct AcceptanceRiskSummary {
    /// Native assessment level, before an execution Policy raises its floor.
    pub level: RiskLevel,
    pub precision: String,
    pub findings_total: usize,
    pub findings: Vec<AcceptanceRiskFinding>,
    pub bug_pattern_matches: usize,
    pub drift_findings: usize,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct AcceptanceInspectionCoverage {
    pub changed_paths_total: usize,
    pub graph_files_indexed: usize,
    pub symbols_observed: usize,
    pub symbols_returned: usize,
    pub mapped_paths_total: usize,
    pub unmapped_paths_total: usize,
    pub unmapped_paths: Vec<String>,
    pub uncovered_paths_total: usize,
    pub uncovered_paths: Vec<String>,
    pub verification_observed: usize,
    pub verification_returned: usize,
    pub components_observed: usize,
    pub requirements_observed: usize,
    pub acceptance_observed: usize,
    pub mappings_complete: bool,
    pub graph_complete: bool,
    pub totals_complete: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChangeAcceptanceRecord {
    pub schema_version: u32,
    pub producer: String,
    pub captured_at_ms: u64,
    pub id: String,
    pub record_digest: String,
    pub workspace: String,
    pub revision: Revision,
    pub git: GitChangeSnapshot,
    pub policy: Option<AcceptancePolicyBinding>,
    pub plan: Option<AcceptancePlanSummary>,
    pub risk_level: RiskLevel,
    pub state: AcceptanceState,
    pub partial: bool,
    pub reasons: Vec<AcceptanceReason>,
    pub actions: Vec<AcceptanceAction>,
    pub checks: Vec<AcceptanceCheck>,
    pub summary: AcceptanceSummary,
    pub evidence_ids: Vec<String>,
    pub evidence: Vec<AcceptanceEvidenceSummary>,
    pub verification: Option<AcceptanceVerificationSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inspection: Option<AcceptanceChangeInspection>,
}

impl ChangeAcceptanceRecord {
    /// Stable content identity, not a signature, authorization or trusted verdict.
    pub fn digest(&self) -> &str {
        &self.record_digest
    }

    /// Attach revision-bound native metadata atomically. Inspection does not
    /// change required checks, gate outcomes, partial status or readiness.
    pub(crate) fn supplement_inspection(
        &mut self,
        inspection: AcceptanceChangeInspection,
    ) -> Result<()> {
        ensure!(
            inspection.workspace == self.workspace
                && inspection.revision == self.revision
                && inspection.base_sha == self.git.base_sha
                && inspection.target_sha == self.git.target_sha
                && inspection.git_binding == self.git.binding,
            "acceptance inspection identity does not match Record"
        );
        ensure!(
            inspection.producer == "wcode/native-change-inspection/v1",
            "acceptance inspection producer is invalid"
        );
        ensure!(
            inspection.symbols.len() <= 128
                && inspection.impacted_components.len() <= 128
                && inspection.impacted_requirements.len() <= 128
                && inspection.impacted_acceptance.len() <= 128
                && inspection.mapped_verification.len() <= 128
                && inspection.risk.findings.len() <= 32
                && inspection.coverage.unmapped_paths.len() <= 128
                && inspection.coverage.uncovered_paths.len() <= 128
                && inspection.unknown_reasons.len() <= 32
                && inspection.recommended_actions.len() <= 16,
            "acceptance inspection item budget exceeded"
        );
        ensure!(
            serde_json::to_vec(&inspection)?.len() <= 256 * 1024,
            "acceptance inspection byte budget exceeded"
        );
        let mut candidate = self.clone();
        candidate.inspection = Some(inspection);
        candidate.refresh_digest()?;
        *self = candidate;
        Ok(())
    }

    fn refresh_digest(&mut self) -> Result<()> {
        let mut facts = self.clone();
        // Observation time and prior computed identities are not native facts.
        facts.captured_at_ms = 0;
        facts.id.clear();
        facts.record_digest.clear();
        let bytes = serde_json::to_vec(&facts)?;
        ensure!(
            bytes.len() <= 2 * 1024 * 1024,
            "acceptance Record byte budget exceeded"
        );
        let mut hash = Sha256::new();
        hash.update(b"wcode-change-acceptance-v1\0");
        hash.update(bytes);
        let digest = format!("{:x}", hash.finalize());
        self.id = format!("CAR-{digest}");
        self.record_digest = format!("sha256:{digest}");
        Ok(())
    }
}

/// Evaluate the same Verification engine plus approved Policy/Git/scope guards.
/// Native Fail wins over model review and operator approval. A skipped,
/// unavailable, stale, unmapped or weak-level check is never a passing gate.
pub fn evaluate_acceptance(input: AcceptanceInput<'_>) -> Result<ChangeAcceptanceRecord> {
    rules::validate_input(&input)?;
    let mut reasons = Vec::new();
    rules::context_reasons(&input, &mut reasons);
    let usable = input
        .evidence
        .iter()
        .filter(|record| checks::usable_for_git(&input, record))
        .cloned()
        .collect::<Vec<_>>();
    let status = match input.plan_id {
        Some(id) => {
            ensure!(
                input.verification.plans.contains_key(id),
                "acceptance plan does not exist"
            );
            Some(super::evaluate_snapshot(
                input.verification,
                input.workspace,
                id,
                input.revision,
                &usable,
            )?)
        }
        None => {
            rules::reason(
                &mut reasons,
                "verification_plan_missing",
                None,
                AcceptanceAction::PlanVerification,
            );
            None
        }
    };
    if let Some(status) = &status {
        rules::plan_reasons(&input, status, &mut reasons);
    }
    let required = checks::required_bindings(&input, status.as_ref())?;
    let rows = checks::project_checks(&input, status.as_ref(), &required, &mut reasons)?;
    if let Some(status) = &status {
        rules::verification_reasons(&input, status, &mut reasons)?;
    }
    reasons.sort_by(|a, b| (&a.code, &a.subject, a.action).cmp(&(&b.code, &b.subject, b.action)));
    reasons.dedup_by(|a, b| a.code == b.code && a.subject == b.subject && a.action == b.action);
    ensure!(reasons.len() <= 256, "acceptance reason budget exceeded");
    let state = rules::classify(&reasons);
    let partial = reasons.iter().any(|reason| {
        matches!(
            reason.code.as_str(),
            "revision_incomplete"
                | "git_capture_incomplete"
                | "discovery_incomplete"
                | "mapping_incomplete"
                | "policy_unavailable"
        )
    });
    let summary = checks::summarize(&rows);
    let mut evidence_ids = rows
        .iter()
        .flat_map(|row| row.evidence_ids.iter().cloned())
        .collect::<BTreeSet<_>>();
    evidence_ids.extend(rules::gate_evidence_ids(&input, status.as_ref()));
    ensure!(
        evidence_ids.len() <= 4096,
        "acceptance Evidence identity budget exceeded"
    );
    let mut record = ChangeAcceptanceRecord {
        schema_version: 1,
        producer: "wcode/native-acceptance/v1".into(),
        captured_at_ms: input.now_ms,
        id: String::new(),
        record_digest: String::new(),
        workspace: input.workspace.into(),
        revision: input.revision.clone(),
        git: input.git.clone(),
        policy: input.policy.cloned(),
        plan: status.as_ref().map(|status| AcceptancePlanSummary {
            id: status.plan.id.clone(),
            verification_generation: input.verification.persistence_generation(),
            policy: status.plan.policy.clone(),
            deterministic_level: status.plan.deterministic_level.clone(),
            revision: status.plan.revision.clone(),
        }),
        risk_level: input.risk_level,
        state,
        partial,
        reasons,
        actions: Vec::new(),
        checks: rows,
        summary,
        evidence: checks::evidence_summary(&input, &evidence_ids),
        evidence_ids: evidence_ids.into_iter().collect(),
        verification: status
            .as_ref()
            .map(|status| checks::verification_summary(&input, status)),
        inspection: None,
    };
    record.actions = record
        .reasons
        .iter()
        .map(|reason| reason.action)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    record.refresh_digest()?;
    Ok(record)
}

fn minimum_level(input: &AcceptanceInput<'_>) -> &'static str {
    if input.risk_level >= RiskLevel::Medium
        || input
            .policy
            .is_some_and(|policy| policy.selection.requirements.minimum_level == PolicyLevel::Full)
    {
        "full"
    } else {
        "quick"
    }
}

#[cfg(test)]
#[path = "../../tests/unit/verification/acceptance.rs"]
mod tests;
