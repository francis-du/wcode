use crate::code_index::CodeIndex;
use crate::conventions::{self, ConventionReport};
use crate::design::{self, CodeRef, VerificationRef};
use crate::evidence_store;
use crate::graph::{
    EdgeKind, GraphEdge, GraphNode, GraphPrecision, GraphProvenance, GraphProviderImport, NodeKind,
    SoftwareGraphSnapshot,
};
use crate::graph_explorer::{
    GraphOverviewInput, GraphOverviewResult, GraphSearchInput, GraphSearchResult,
};
use crate::graph_provider_store::{self, GraphProviderSummary, StoredGraphProvider};
use crate::graph_store::{
    self, GraphChainInput, GraphChainResult, GraphDiffInput, GraphDiffResult, GraphHistoryEntry,
    GraphQueryInput, GraphQueryResult,
};
use crate::intelligence::{
    DesignStatus, DriftStatus, EvidenceStatus, RiskStatus, SemanticStatusView, SoftwareContext,
    SoftwareContextRequest, SoftwareIntelligenceRuntime, TraceabilityStatus,
};
use crate::quality_provider::{self, LanguageQualityRegistry, LanguageQualityRun};
use crate::reconcile::{
    ImpactAnalysis, ReconciliationExecutionStatus, ReconciliationPlan, ReconciliationTaskKind,
    ReconciliationTaskRun, ReconciliationTaskSubmission,
};
use crate::reconciliation_execution_store;
use crate::reconciliation_store;
use crate::runtime_telemetry::{TaskTelemetry, TaskTelemetryTicket};
use crate::scopes::{self, ProductScopeDescriptor};
use crate::semantic::{SemanticCandidateInput, SemanticFact, SemanticMatch};
use crate::semantic_provider::install::SemanticProviderInstallResult;
use crate::semantic_provider::{
    self, SemanticLanguage, SemanticNavigationIntent, SemanticProviderRefresh,
    SemanticProviderStatus, SemanticSessionPool, SemanticSessionPoolStatus,
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
pub use harness_profile::ProfileDiscoveryCompleteness;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinSet;

const MAX_PARALLEL_TOOLS: usize = 256;
pub(crate) const TOOL_SLOT_WAIT_CAP: Duration = Duration::from_secs(10);
pub(crate) const REPO_MAP_MAX_FILES: usize = 600;
pub(crate) const REPO_MAP_MAX_SYMBOLS: usize = 5_000;
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
    "deno.json",
    "deno.jsonc",
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
    pub new_name: Option<String>,
    pub max_files: usize,
}

type RepoMapCacheKey = (PathBuf, String);
type RepoMapCache = HashMap<RepoMapCacheKey, CachedRepoMapGraph>;
type ValidationFlights<K> = HashMap<K, Weak<ValidationFlight>>;

#[derive(Clone)]
pub struct ToolHarness {
    slots: Arc<Semaphore>,
    execution_slots: Arc<Semaphore>,
    admission_waiters: Arc<harness_admission::AdmissionWaiters>,
    max_parallel: usize,
    project_cache: Arc<Mutex<HashMap<PathBuf, CachedProjectProfile>>>,
    project_flights: Arc<Mutex<ValidationFlights<PathBuf>>>,
    observatory_cache: Arc<Mutex<HashMap<PathBuf, CachedProjectObservatory>>>,
    observatory_refreshes: Arc<Mutex<HashSet<PathBuf>>>,
    convention_cache: Arc<Mutex<HashMap<PathBuf, CachedConventionReport>>>,
    convention_flights: Arc<Mutex<ValidationFlights<PathBuf>>>,
    repo_map_cache: Arc<Mutex<RepoMapCache>>,
    repo_map_flights: Arc<Mutex<ValidationFlights<RepoMapCacheKey>>>,
    verification_cache: Arc<Mutex<harness_verification_cache::VerificationCache>>,
    verification_run_flights: Arc<Mutex<harness_verification_cache::VerificationRunFlights>>,
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

#[path = "admission.rs"]
mod harness_admission;

#[derive(Clone)]
struct CachedProjectProfile {
    fingerprint: u64,
    last_used: Instant,
    profile: Arc<ProjectProfile>,
}

pub(crate) struct ObservatoryRefreshGuard {
    refreshes: Arc<Mutex<HashSet<PathBuf>>>,
    root: PathBuf,
}

impl Drop for ObservatoryRefreshGuard {
    fn drop(&mut self) {
        if let Ok(mut refreshes) = self.refreshes.lock() {
            refreshes.remove(&self.root);
        }
    }
}

#[derive(Clone)]
struct CachedProjectObservatory {
    last_used: Instant,
    snapshot: Arc<crate::intelligence_types::ProjectObservatory>,
    revision_key: Option<String>,
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

#[derive(Default)]
struct ValidationFlight {
    gate: Mutex<()>,
    generation: AtomicU64,
    successful_generation: AtomicU64,
    invalidation_revision: AtomicU64,
    successful_revision: AtomicU64,
    coalescible_callers: AtomicUsize,
    #[cfg(test)]
    entrants: AtomicU64,
}

#[derive(Clone, Debug, Serialize)]
struct ProjectProfile {
    #[serde(default)]
    discovery: ProfileDiscoveryCompleteness,
    #[serde(skip)]
    policy_sources: Vec<crate::verification::policy::PolicySourceDigest>,
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
    #[serde(default)]
    pub discovery: ProfileDiscoveryCompleteness,
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

pub use crate::report_types::{
    ChangeReviewReport, ChangedFileReview, ProjectVerificationImpact,
    ProjectVerificationImpactReason, ReviewFinding, ReviewProbeSummary, VerificationCheck,
    VerificationCostDecision, VerificationCostFrontierEntry, VerificationReport,
};

pub use harness_quality::counterexamples::CounterexampleSearch;

#[derive(Clone, Debug, Serialize)]
pub struct CounterexampleExperiment {
    pub kind: String,
    pub targets: Vec<String>,
    pub targets_total: usize,
    pub targets_truncated: bool,
    pub hypothesis: String,
    pub falsifying_condition: String,
    pub execution: Vec<String>,
    pub closes_with: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AdversarialQuestion {
    pub id: String,
    pub category: String,
    pub claim: String,
    pub challenge: String,
    pub current_signal: String,
    pub required_evidence: Vec<String>,
    pub suggested_tools: Vec<String>,
    pub counterexample_experiment: CounterexampleExperiment,
}

#[derive(Clone, Debug, Serialize)]
pub struct AdversarialReviewReport {
    pub workspace: String,
    pub provider: &'static str,
    pub precision: &'static str,
    pub policy: &'static str,
    pub review_risk_level: String,
    pub questions: Vec<AdversarialQuestion>,
    pub truncated: bool,
    pub recommended_next_actions: Vec<String>,
    pub reviewer_role: &'static str,
    pub reviewer_bridge: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_search: Option<CounterexampleSearch>,
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

#[path = "capabilities.rs"]
mod harness_capabilities;
#[cfg(test)]
pub(crate) use crate::model_tools::{model_tools_for_group, promote_model_tools};
pub(crate) use harness_capabilities::{
    default_coding_tools, model_tool_group, model_tool_preload_recommended, prioritize_model_tools,
};
#[path = "acceptance.rs"]
mod harness_acceptance;
#[path = "acceptance_context.rs"]
mod harness_acceptance_context;
#[path = "core.rs"]
mod harness_core;
#[path = "policy.rs"]
mod harness_policy;
#[path = "policy_select.rs"]
mod harness_policy_select;
#[path = "reconciliation.rs"]
mod harness_reconciliation;
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
pub(crate) use harness_agent_context::diagnostic_locations;

#[path = "retrieval.rs"]
mod harness_retrieval;

#[path = "cache_flight.rs"]
mod harness_cache_flight;

#[path = "convention_cache.rs"]
mod harness_convention_cache;

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
    let signature = verification_check_binding(&check).signature;
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
        execution: if result.timed_out {
            crate::evidence::VerificationCheckExecution::TimedOut
        } else if result.exit_code.is_some() {
            crate::evidence::VerificationCheckExecution::Executed
        } else {
            crate::evidence::VerificationCheckExecution::Unavailable
        },
        exit_code: result.exit_code,
        elapsed_ms,
        queue_wait_ms,
        execution_ms,
        stdout_tail,
        stderr_tail,
        output_truncated: result.truncated || stdout_cut || stderr_cut,
        signature: Some(signature),
        evidence_id: None,
    }
}

pub(crate) fn verification_check_binding(
    check: &CheckSpec,
) -> crate::evidence::RequiredVerificationCheck {
    crate::evidence::RequiredVerificationCheck::from_command(
        &check.id,
        &check.program,
        &check.args,
        &check.cwd,
        &check.island,
    )
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
