use super::{ExecutionGitBinding, RequiredVerificationCheck, VerificationCheckExecution};
use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct VerificationReport {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub execution_git_binding: Option<ExecutionGitBinding>,
    pub workspace: String,
    pub level: String,
    pub execution: String,
    pub phases_run: usize,
    pub passed: bool,
    pub checks_run: usize,
    pub checks_reused: usize,
    pub checks_failed: usize,
    pub skipped_checks: Vec<String>,
    pub elapsed_ms: u128,
    pub summary: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impact: Option<ProjectVerificationImpact>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_model: Option<VerificationCostDecision>,
    pub checks: Vec<VerificationCheck>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub required_checks: Option<Vec<RequiredVerificationCheck>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct VerificationCostDecision {
    pub model: &'static str,
    pub provider: &'static str,
    pub precision: &'static str,
    pub sentinel_check: String,
    pub sentinel_command: String,
    pub sentinel_island: String,
    pub samples: usize,
    pub failures: usize,
    pub failure_rate_percent: f64,
    pub median_elapsed_ms: u128,
    pub estimated_savings_ms: u128,
    pub estimated_total_savings_ms: u128,
    pub evidence_records_scanned: usize,
    pub frontier: Vec<VerificationCostFrontierEntry>,
}

#[derive(Clone, Debug, Serialize)]
pub struct VerificationCostFrontierEntry {
    pub order: usize,
    pub check_id: String,
    pub command: String,
    pub island: String,
    pub samples: usize,
    pub failures: usize,
    pub failure_rate_percent: f64,
    pub median_elapsed_ms: u128,
    pub marginal_samples: usize,
    pub marginal_failures: usize,
    pub marginal_failure_rate_percent: f64,
    pub estimated_incremental_savings_ms: u128,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectVerificationImpact {
    pub selective: bool,
    pub affected_islands: Vec<String>,
    pub reasons: Vec<ProjectVerificationImpactReason>,
    pub truncated: bool,
    pub provider: &'static str,
    pub precision: &'static str,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize)]
pub struct ProjectVerificationImpactReason {
    pub island: String,
    pub kind: &'static str,
    pub source: String,
    pub relationship: String,
    pub evidence: String,
    pub provider: &'static str,
    pub precision: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct VerificationCheck {
    pub id: String,
    pub phase: u8,
    pub command: String,
    pub reason: String,
    pub success: bool,
    pub reused: bool,
    pub execution: VerificationCheckExecution,
    pub exit_code: Option<i32>,
    pub elapsed_ms: u128,
    pub queue_wait_ms: u64,
    pub execution_ms: u128,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub output_truncated: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub evidence_id: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChangeReviewReport {
    pub workspace: String,
    pub execution: String,
    pub clean: bool,
    pub files_changed: usize,
    pub staged_files: usize,
    pub unstaged_files: usize,
    pub untracked_files: usize,
    pub additions: u64,
    pub deletions: u64,
    pub binary_files: usize,
    pub source_changed: bool,
    pub tests_changed: bool,
    pub docs_only: bool,
    pub risk_level: String,
    pub recommended_verification: String,
    pub recommended_checks: Vec<String>,
    pub summary: String,
    pub files: Vec<ChangedFileReview>,
    pub findings: Vec<ReviewFinding>,
    pub probes: Vec<ReviewProbeSummary>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChangedFileReview {
    pub path: String,
    pub status: String,
    pub staged: bool,
    pub unstaged: bool,
    pub untracked: bool,
    pub category: String,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub binary: bool,
    pub risk_reasons: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReviewFinding {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub paths: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ReviewProbeSummary {
    pub id: String,
    pub success: bool,
    pub elapsed_ms: u128,
    pub queue_wait_ms: u64,
    pub execution_ms: u128,
    pub error: Option<String>,
}

pub fn verification_metrics_summary(elapsed_ms: u128, phase: u8) -> String {
    format!("verification-metrics-v1;elapsed_ms={elapsed_ms};phase={phase}")
}
