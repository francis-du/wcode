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
const MAX_GUIDANCE_LINES_PER_FILE: usize = 160;
const MAX_GUIDANCE_CHARS_PER_FILE: usize = 12_000;
const MAX_GUIDANCE_CHARS_TOTAL: usize = 32_000;
const MAX_PROFILE_SOURCE_BYTES: u64 = 1024 * 1024;
const MAX_CHECK_OUTPUT_CHARS: usize = 12_000;
const MAX_VERIFICATION_CHECKS: usize = 8;
const MAX_REVIEW_FILES: usize = 500;
const MAX_REVIEW_FINDINGS: usize = 64;
const QUALITY_HARNESS_TOOLS: &[&str] = &["project_context", "review_changes", "verify_project"];
const SOFTWARE_INTELLIGENCE_CAPABILITIES: &[&str] = &[
    "design_state",
    "software_graph",
    "graph_history",
    "semantic_registry",
    "semantic_providers",
    "traceability",
    "software_context",
    "drift",
    "impact_analysis",
    "risk",
    "reconciliation",
    "reconciliation_execution",
    "verification_mesh",
    "stage_executors",
    "persistent_evidence",
    "cli_intelligence",
    "tui_intelligence",
    "web_intelligence",
];

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
    "pyproject.toml",
    "requirements.txt",
    "go.mod",
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
    max_parallel: usize,
    project_cache: Arc<Mutex<HashMap<PathBuf, CachedProjectProfile>>>,
    repo_map_cache: Arc<Mutex<HashMap<(PathBuf, String), CachedRepoMapGraph>>>,
    code_index: CodeIndex,
    semantic_sessions: SemanticSessionPool,
    intelligence: SoftwareIntelligenceRuntime,
}

#[derive(Clone)]
struct CachedProjectProfile {
    fingerprint: u64,
    profile: Arc<ProjectProfile>,
}

#[derive(Clone)]
struct CachedRepoMapGraph {
    fingerprint: u64,
    snapshot: Arc<SoftwareGraphSnapshot>,
}

#[derive(Clone, Debug, Serialize)]
struct ProjectProfile {
    root: String,
    project_types: Vec<String>,
    manifests: Vec<String>,
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
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct VerificationReport {
    pub workspace: String,
    pub level: String,
    pub execution: String,
    pub phases_run: usize,
    pub passed: bool,
    pub checks_run: usize,
    pub checks_failed: usize,
    pub elapsed_ms: u128,
    pub summary: String,
    pub checks: Vec<VerificationCheck>,
}

#[derive(Debug, Serialize)]
pub struct VerificationCheck {
    pub id: String,
    pub phase: u8,
    pub command: String,
    pub reason: String,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub elapsed_ms: u128,
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

#[path = "quality.rs"]
mod harness_quality;

#[path = "graph.rs"]
mod harness_graph;
use harness_graph::{design_product_id, overlay_design_graph};

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

#[path = "repo_map.rs"]
mod harness_repo_map;

#[path = "review.rs"]
mod harness_review;
use harness_review::*;

#[path = "verification.rs"]
mod harness_verification;
use harness_verification::run_verification_check;
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
    let (stdout_tail, stdout_cut) = tail_chars(&result.stdout, MAX_CHECK_OUTPUT_CHARS);
    let (stderr_tail, stderr_cut) = tail_chars(&result.stderr, MAX_CHECK_OUTPUT_CHARS);
    VerificationCheck {
        id: check.id,
        phase: check.phase,
        command: command_text(&result.program, &result.args),
        reason: check.reason,
        success: result.success,
        exit_code: result.exit_code,
        elapsed_ms,
        stdout_tail,
        stderr_tail,
        output_truncated: result.truncated || stdout_cut || stderr_cut,
    }
}

#[path = "text.rs"]
mod harness_text;
use harness_text::{command_text, tail_chars, truncate_chars};

#[cfg(test)]
#[path = "../../../tests/unit/runtime/harness.rs"]
mod tests;
