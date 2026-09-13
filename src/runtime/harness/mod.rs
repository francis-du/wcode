use crate::code_index::CodeIndex;
use crate::conventions::{self, ConventionReport};
use crate::design::{self, CodeRef, VerificationRef};
use crate::evidence_store;
use crate::graph::{
    EdgeKind, GraphEdge, GraphNode, GraphPrecision, GraphProvenance, GraphProviderImport, NodeKind,
    SoftwareGraphSnapshot,
};
use crate::graph_provider_store::{self, GraphProviderSummary, StoredGraphProvider};
use crate::graph_store::{
    self, GraphDiffInput, GraphDiffResult, GraphHistoryEntry, GraphQueryInput, GraphQueryResult,
};
use crate::intelligence::{
    DesignStatus, DriftStatus, EvidenceStatus, RiskStatus, SemanticStatusView, SoftwareContext,
    SoftwareContextRequest, SoftwareIntelligenceRuntime, TraceabilityStatus,
};
use crate::monitor::TaskMonitor;
use crate::quality_provider::{self, LanguageQualityRegistry, LanguageQualityRun};
use crate::reconcile::{
    ImpactAnalysis, ReconciliationExecutionStatus, ReconciliationPlan, ReconciliationTaskKind,
    ReconciliationTaskRun, ReconciliationTaskSubmission,
};
use crate::reconciliation_execution_store;
use crate::reconciliation_store;
use crate::scopes::{self, ProductScopeDescriptor};
use crate::semantic::{SemanticCandidateInput, SemanticFact, SemanticMatch};
use crate::semantic_provider::{
    self, SemanticNavigationIntent, SemanticProviderRefresh, SemanticProviderStatus,
    SemanticSessionPool, SemanticSessionPoolStatus,
};
use crate::semantic_store;
use crate::stage_executor::{self, StageExecutionResult, StageExecutorRegistry};
use crate::verification::{
    ReviewSubmission, ReviewerRole, StageSubmission, VerificationJob, VerificationPlan,
    VerificationStatus,
};
use crate::verification_store;
use crate::workspace::{CommandResult, Workspace};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinSet;

const MAX_PARALLEL_TOOLS: usize = 256;
pub(crate) const REPO_MAP_MAX_FILES: usize = 600;
const MAX_OBSERVATORY_FILES: usize = 1_500;
const MAX_GUIDANCE_LINES_PER_FILE: usize = 160;
const MAX_GUIDANCE_CHARS_PER_FILE: usize = 12_000;
const MAX_GUIDANCE_CHARS_TOTAL: usize = 32_000;
const MAX_PROFILE_SOURCE_BYTES: u64 = 1024 * 1024;
const MAX_CHECK_OUTPUT_CHARS: usize = 12_000;
// A mixed Rust/Node/Python/Go/Make project can infer more than eight checks.
// This is a total-plan bound, not a silent truncation or a concurrency target.
const MAX_VERIFICATION_CHECKS: usize = 32;
const MAX_REVIEW_FILES: usize = 500;
const MAX_REVIEW_FINDINGS: usize = 64;
const QUALITY_HARNESS_TOOLS: &[&str] = &["project_context", "review_changes", "verify_project"];
const GUIDANCE_FILES: &[&str] = &[
    "AGENTS.md",
    ".github/copilot-instructions.md",
    "CLAUDE.md",
    "CONTRIBUTING.md",
    "docs/manual/development.md",
    "README.md",
];

const MANIFEST_FILES: &[&str] = &[
    "Cargo.toml",
    "package.json",
    "tsconfig.json",
    "pyproject.toml",
    "requirements.txt",
    "go.mod",
    "pom.xml",
    "build.gradle",
    "build.gradle.kts",
    "Package.swift",
    "pubspec.yaml",
    "mix.exs",
    "Gemfile",
    "composer.json",
    "dune-project",
    "DESCRIPTION",
    "CMakeLists.txt",
    "Makefile",
];

const PROFILE_FILES: &[&str] = &[
    "AGENTS.md",
    ".github/copilot-instructions.md",
    "CLAUDE.md",
    "CONTRIBUTING.md",
    "docs/manual/development.md",
    "README.md",
    "Cargo.toml",
    "Cargo.lock",
    ".config/nextest.toml",
    "nextest.toml",
    "package.json",
    "package-lock.json",
    "pnpm-lock.yaml",
    "yarn.lock",
    "bun.lock",
    "bun.lockb",
    "pyproject.toml",
    "requirements.txt",
    "pytest.ini",
    "go.mod",
    "go.sum",
    "Makefile",
];

#[derive(Clone, Debug)]
pub(crate) struct SemanticNavigationRequest {
    pub path: String,
    pub symbol: Option<String>,
    pub line: Option<usize>,
    pub character: Option<usize>,
    pub intent: SemanticNavigationIntent,
    pub max_results: usize,
}

#[derive(Clone)]
pub struct ToolHarness {
    slots: Arc<Semaphore>,
    execution_slots: Arc<Semaphore>,
    max_parallel: usize,
    project_cache: Arc<Mutex<HashMap<PathBuf, CachedProjectProfile>>>,
    convention_cache: Arc<Mutex<HashMap<PathBuf, CachedConventionReport>>>,
    repo_map_cache: Arc<Mutex<HashMap<(PathBuf, String), CachedRepoMapGraph>>>,
    verification_cache: Arc<Mutex<harness_verification_cache::VerificationCache>>,
    code_index: CodeIndex,
    semantic_sessions: SemanticSessionPool,
    intelligence: SoftwareIntelligenceRuntime,
}

// Both permits follow the real work, including a blocking worker that outlives
// its cancelled async caller. Neither changes authorization or the total cap.
pub(crate) struct ToolPermit {
    _slot: OwnedSemaphorePermit,
    _execution: Option<OwnedSemaphorePermit>,
}

impl From<OwnedSemaphorePermit> for ToolPermit {
    fn from(slot: OwnedSemaphorePermit) -> Self {
        Self {
            _slot: slot,
            _execution: None,
        }
    }
}

impl ToolHarness {
    fn execution_limit(max_parallel: usize) -> usize {
        max_parallel
            .saturating_sub(max_parallel.div_ceil(8).min(4))
            .max(1)
    }

    pub(crate) async fn acquire_tool(&self, executes_process: bool) -> Result<ToolPermit, String> {
        // Queue command traffic before it can consume every global slot. All
        // acquisitions use this order; no parent holds a slot waiting for it.
        let execution = if executes_process {
            Some(
                self.execution_slots
                    .clone()
                    .acquire_owned()
                    .await
                    .map_err(|_| "execution admission is shutting down".to_owned())?,
            )
        } else {
            None
        };
        let slot = self.acquire().await?;
        Ok(ToolPermit {
            _slot: slot,
            _execution: execution,
        })
    }

    pub async fn acquire(&self) -> Result<OwnedSemaphorePermit, String> {
        let permit = self
            .slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "tool harness is shutting down".to_owned())?;
        crate::resource::global().admit_tool().await?;
        Ok(permit)
    }
}

#[derive(Clone)]
struct CachedProjectProfile {
    fingerprint: u64,
    last_used: Instant,
    profile: Arc<ProjectProfile>,
}

#[derive(Clone)]
struct CachedConventionReport {
    fingerprint: u64,
    last_used: Instant,
    report: Arc<ConventionReport>,
}

#[derive(Clone)]
struct CachedRepoMapGraph {
    fingerprint: u64,
    last_used: Instant,
    snapshot: Arc<SoftwareGraphSnapshot>,
}

#[derive(Clone, Debug, Serialize)]
struct ProjectProfile {
    root: String,
    project_types: Vec<String>,
    manifests: Vec<String>,
    islands: Vec<ProjectIsland>,
    contracts: ProjectContractTopology,
    guidance: Vec<GuidanceDocument>,
    recommended_checks: Vec<CheckSpec>,
    workflow: Vec<String>,
    write_enabled: bool,
    exec_enabled: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectContext {
    pub workspace: String,
    pub cache_hit: bool,
    pub root: String,
    pub project_types: Vec<String>,
    pub manifests: Vec<String>,
    pub islands: Vec<ProjectIsland>,
    pub contracts: ProjectContractTopology,
    pub guidance: Vec<GuidanceDocument>,
    pub recommended_checks: Vec<CheckSpec>,
    pub workflow: Vec<String>,
    pub write_enabled: bool,
    pub exec_enabled: bool,
    pub product_scopes: Vec<ProductScopeDescriptor>,
    pub conventions: ConventionReport,
    pub language_quality: LanguageQualityRegistry,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectIsland {
    pub id: String,
    pub root: String,
    pub project_types: Vec<String>,
    pub languages: Vec<String>,
    pub manifests: Vec<String>,
    pub check_ids: Vec<String>,
    pub dependencies: Vec<ProjectIslandDependency>,
    pub verification_status: &'static str,
    pub verification_gaps: Vec<String>,
    pub provider: &'static str,
    pub precision: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectIslandDependency {
    pub island: String,
    pub kind: String,
    pub evidence: String,
    pub provider: &'static str,
    pub precision: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectContractTopology {
    pub bridges: Vec<ProjectContractBridge>,
    pub diagnostics: Vec<ProjectContractDiagnostic>,
    pub truncated: bool,
    pub provider: &'static str,
    pub precision: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectContractBridge {
    pub source: String,
    pub source_kind: &'static str,
    pub output: String,
    pub consumer_island: String,
    pub kind: &'static str,
    pub evidence: String,
    pub provider: &'static str,
    pub precision: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectContractDiagnostic {
    pub path: String,
    pub reason: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct GuidanceDocument {
    pub path: String,
    pub excerpt: String,
    pub included_lines: usize,
    pub total_lines: usize,
    pub truncated: bool,
    pub redacted: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct CheckSpec {
    pub id: String,
    pub level: String,
    pub phase: u8,
    pub program: String,
    pub args: Vec<String>,
    pub cwd: String,
    pub island: String,
    pub languages: Vec<String>,
    pub reason: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct VerificationReport {
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
    pub exit_code: Option<i32>,
    pub elapsed_ms: u128,
    pub queue_wait_ms: u64,
    pub execution_ms: u128,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub output_truncated: bool,
}

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
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

#[derive(Debug, Serialize)]
pub struct ReviewFinding {
    pub severity: String,
    pub code: String,
    pub message: String,
    pub paths: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct ReviewProbeSummary {
    pub id: String,
    pub success: bool,
    pub elapsed_ms: u128,
    pub queue_wait_ms: u64,
    pub execution_ms: u128,
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct ObservatoryRevisionSignal {
    pub fingerprint: Option<String>,
    pub changed_files: usize,
    pub truncated: bool,
    pub full_refresh_required: bool,
}

#[derive(Clone)]
struct ReviewProbeSpec {
    id: &'static str,
    args: Vec<String>,
}

struct ReviewProbeOutput {
    id: String,
    result: Option<CommandResult>,
    elapsed_ms: u128,
    error: Option<String>,
}

#[derive(Default)]
struct ChangedFileBuilder {
    status: String,
    staged: bool,
    unstaged: bool,
    untracked: bool,
    additions: u64,
    deletions: u64,
    has_numstat: bool,
    binary: bool,
}

#[path = "core.rs"]
mod harness_core;
#[path = "semantic_provider.rs"]
mod harness_semantic_provider;

#[path = "quality.rs"]
mod harness_quality;

#[path = "graph.rs"]
mod harness_graph;
use harness_graph::design_product_id;

#[path = "scope.rs"]
mod harness_scope;

#[path = "project.rs"]
mod harness_project;

#[path = "profile.rs"]
mod harness_profile;

#[path = "memory.rs"]
mod harness_memory;

#[path = "agent_context.rs"]
mod harness_agent_context;

#[path = "retrieval.rs"]
mod harness_retrieval;

#[path = "repo_map.rs"]
mod harness_repo_map;

#[path = "review.rs"]
mod harness_review;
use harness_review::*;

#[path = "verification.rs"]
mod harness_verification;
use harness_verification::run_verification_check;
#[path = "verification_cache.rs"]
mod harness_verification_cache;

#[path = "cost.rs"]
mod harness_cost;
#[path = "test_focus.rs"]
mod harness_test_focus;

pub(crate) fn verification_metrics_summary(elapsed_ms: u128, phase: u8) -> String {
    harness_cost::metrics_summary(elapsed_ms, phase)
}

fn sort_checks(checks: &mut [CheckSpec]) {
    checks.sort_by(|left, right| {
        left.phase
            .cmp(&right.phase)
            .then_with(|| check_rank(&left.level).cmp(&check_rank(&right.level)))
            .then_with(|| left.id.cmp(&right.id))
    });
}

fn verification_phase(id: &str) -> u8 {
    if id.contains("build") {
        3
    } else if id == "rust-clippy" {
        2
    } else if id.contains("test") {
        1
    } else {
        0
    }
}

fn check_rank(level: &str) -> u8 {
    if level == "quick" {
        0
    } else {
        1
    }
}

fn verification_check(
    check: CheckSpec,
    result: CommandResult,
    elapsed_ms: u128,
) -> VerificationCheck {
    let command = verification_command_text(&check);
    let (stdout_tail, stdout_cut) =
        harness_verification::verification_output(&result.stdout, result.success);
    let (stderr_tail, stderr_cut) =
        harness_verification::verification_output(&result.stderr, result.success);
    let queue_wait_ms = result.process_queue_wait_ms;
    let execution_ms = elapsed_ms.saturating_sub(u128::from(queue_wait_ms));
    VerificationCheck {
        id: check.id,
        phase: check.phase,
        command,
        reason: check.reason,
        success: result.success,
        reused: false,
        exit_code: result.exit_code,
        elapsed_ms,
        queue_wait_ms,
        execution_ms,
        stdout_tail,
        stderr_tail,
        output_truncated: result.truncated || stdout_cut || stderr_cut,
    }
}

fn verification_command_text(check: &CheckSpec) -> String {
    let command = command_text(&check.program, &check.args);
    if check.cwd == "." {
        command
    } else {
        format!("(cd {} && {command})", check.cwd)
    }
}

#[path = "text.rs"]
mod harness_text;
use harness_text::{command_text, tail_chars, truncate_chars};

#[cfg(test)]
#[path = "../../../tests/unit/runtime/harness.rs"]
mod tests;
