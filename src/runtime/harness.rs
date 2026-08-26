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
use crate::semantic_provider::{self, SemanticProviderRefresh, SemanticProviderStatus};
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
use std::fs;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinSet;

const MAX_PARALLEL_TOOLS: usize = 256;
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
    "docs/wiki/development.md",
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
    "docs/wiki/development.md",
    "README.md",
    "Cargo.toml",
    "Cargo.lock",
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

#[derive(Clone)]
pub struct ToolHarness {
    slots: Arc<Semaphore>,
    max_parallel: usize,
    project_cache: Arc<Mutex<HashMap<PathBuf, CachedProjectProfile>>>,
    code_index: CodeIndex,
    intelligence: SoftwareIntelligenceRuntime,
}

#[derive(Clone)]
struct CachedProjectProfile {
    fingerprint: u64,
    profile: Arc<ProjectProfile>,
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

impl ToolHarness {
    pub fn new(max_parallel: usize) -> Result<Self> {
        if !(1..=MAX_PARALLEL_TOOLS).contains(&max_parallel) {
            bail!("max parallel tools must be between 1 and {MAX_PARALLEL_TOOLS}");
        }
        Ok(Self {
            slots: Arc::new(Semaphore::new(max_parallel)),
            max_parallel,
            project_cache: Default::default(),
            code_index: CodeIndex::new()?,
            intelligence: SoftwareIntelligenceRuntime::default(),
        })
    }

    pub fn max_parallel(&self) -> usize {
        self.max_parallel
    }

    pub fn intelligence_capability_count(&self) -> usize {
        SOFTWARE_INTELLIGENCE_CAPABILITIES.len()
    }

    pub fn capabilities(&self) -> Value {
        json!({
            "tools": QUALITY_HARNESS_TOOLS,
            "project_context": true,
            "context_cache": true,
            "review_changes": true,
            "parallel_change_review": true,
            "verify_project": true,
            "phased_parallel_verification": true,
            "verification_exec_without_risky_flag": true,
            "verification_levels": ["quick", "full"],
            "max_verification_checks": MAX_VERIFICATION_CHECKS,
            "max_review_files": MAX_REVIEW_FILES,
            "max_parallel_tools": self.max_parallel,
            "software_intelligence": {
                "design_state": true,
                "software_graph": "composite-declared-syntax-external",
                "graph_history": graph_store::capabilities(),
                "graph_providers": graph_provider_store::capabilities(),
                "semantic_providers": {
                    "languages": 22,
                    "adapter": "lsp-document-symbol-call-hierarchy",
                    "precision": "semantic-when-provider-runs-syntax-fallback-otherwise",
                    "requires_risky_exec": true
                },
                "traceability": true,
                "software_context": true,
                "drift": true,
                "impact_analysis": true,
                "risk": true,
                "reconciliation_plan": true,
                "verification_mesh": verification_store::capabilities(),
                "stage_executors": {
                    "builtin_discovery": true,
                    "config": ".wcode/executors.yaml",
                    "no_shell": true,
                    "languages": 22,
                    "stages": ["property", "mutation", "fuzz", "runtime_canary"],
                    "requires_risky_exec": true
                },
                "evidence": evidence_store::capabilities(),
                "semantics": semantic_store::capabilities(),
                "reconciliation": reconciliation_store::capabilities(),
                "reconciliation_execution": reconciliation_execution_store::capabilities(),
                "persistent_store": ["verification-state", "evidence", "semantics", "graph-providers", "graph-history", "reconciliation-plans", "reconciliation-execution"],
                "automatic_reconciliation": "orchestrated-safe-task-execution"
            },
            "code_index": self.code_index.capabilities(),
        })
    }

    pub fn convention_status(&self, workspace: &Workspace) -> Result<ConventionReport> {
        conventions::status(workspace)
    }

    pub fn design_status(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
    ) -> Result<DesignStatus> {
        self.intelligence.design_status(workspace_id, workspace)
    }

    pub fn design_init(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        name: &str,
        description: &str,
    ) -> Result<DesignStatus> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 200 {
            bail!("design project name must contain between 1 and 200 characters");
        }
        let existing = design::load_design(workspace)?;
        if existing.initialized {
            bail!("Design State is already initialized for this workspace");
        }
        let reserved_paths = [
            design::PROJECT_FILE,
            ".wcode/design/product.yaml",
            ".wcode/design/requirements.yaml",
            ".wcode/design/components.yaml",
            ".wcode/design/constraints.yaml",
            ".wcode/design/acceptance.yaml",
            ".wcode/design/decisions.yaml",
        ];
        if let Some(path) = reserved_paths
            .iter()
            .find(|path| workspace.root().join(path).exists())
        {
            bail!("cannot initialize Design State because {path} already exists");
        }
        workspace.ensure_directory(".wcode")?;
        workspace.ensure_directory(design::DESIGN_ROOT)?;
        let project = design::ProjectDesign {
            schema_version: 1,
            name: name.to_owned(),
            description: description.trim().to_owned(),
        };
        let product = design::ProductDesign {
            schema_version: 1,
            id: design_product_id(name),
            name: format!("{name} Software Intelligence"),
            vision:
                "Software continuously converges toward intended design with verifiable evidence."
                    .into(),
            principles: vec![
                "Design State is the desired software state.".into(),
                "Models are replaceable executors, not the source of truth.".into(),
                "Deterministic evidence outranks model consensus.".into(),
            ],
        };
        workspace.create_file(
            design::PROJECT_FILE,
            &serde_yaml::to_string(&project).context("cannot encode project Design State")?,
        )?;
        workspace.create_file(
            ".wcode/design/product.yaml",
            &serde_yaml::to_string(&product).context("cannot encode product Design State")?,
        )?;
        // Empty collection documents are intentionally not materialized. Design State is
        // sparse desired state: requirements/components/constraints/acceptance/decisions
        // appear only when the project has something meaningful to declare in that domain.
        self.design_status(workspace_id, workspace)
    }

    pub fn software_graph(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        path: &str,
        max_files: usize,
        max_symbols: usize,
    ) -> Result<SoftwareGraphSnapshot> {
        let mut snapshot = self.code_index.software_graph(
            workspace_id,
            workspace,
            path,
            max_files,
            max_symbols,
        )?;
        let load = design::load_design(workspace)?;
        let mut composite = false;
        if load.initialized {
            overlay_design_graph(&mut snapshot, &load.state, &self.code_index, workspace)?;
            composite = true;
        }
        if graph_provider_store::overlay_latest(workspace, &mut snapshot)? > 0 {
            composite = true;
        }
        if composite {
            snapshot.provider = "wcode-composite".to_owned();
            snapshot.precision = GraphPrecision::Mixed;
        }
        snapshot.node_count = snapshot.graph.nodes.len();
        snapshot.edge_count = snapshot.graph.edges.len();
        snapshot.graph.validate()?;
        graph_store::persist(workspace, &snapshot)?;
        Ok(snapshot)
    }

    pub fn graph_provider_import(
        &self,
        workspace: &Workspace,
        import: GraphProviderImport,
    ) -> Result<StoredGraphProvider> {
        graph_provider_store::persist(workspace, &import)
    }

    pub fn graph_provider_status(
        &self,
        workspace: &Workspace,
    ) -> Result<Vec<GraphProviderSummary>> {
        graph_provider_store::summaries(workspace)
    }

    pub fn semantic_provider_status(
        &self,
        workspace: &Workspace,
    ) -> Result<Vec<SemanticProviderStatus>> {
        semantic_provider::status(workspace)
    }

    pub async fn semantic_provider_refresh(
        &self,
        workspace: &Workspace,
        path: &str,
        max_files: usize,
        max_symbols: usize,
    ) -> Result<SemanticProviderRefresh> {
        let existing = graph_provider_store::load_latest(workspace)?
            .into_iter()
            .map(|stored| (stored.import.provider.clone(), stored.import))
            .collect::<BTreeMap<_, _>>();
        let refresh =
            semantic_provider::refresh(workspace, path, max_files, max_symbols, &existing).await?;
        for import in &refresh.imports {
            graph_provider_store::persist(workspace, import)?;
        }
        Ok(refresh)
    }

    pub fn graph_history(
        &self,
        workspace: &Workspace,
        limit: usize,
    ) -> Result<Vec<GraphHistoryEntry>> {
        graph_store::history(workspace, limit)
    }

    pub fn graph_query(
        &self,
        workspace: &Workspace,
        input: &GraphQueryInput,
    ) -> Result<GraphQueryResult> {
        graph_store::query(workspace, input)
    }

    pub fn graph_diff(
        &self,
        workspace: &Workspace,
        input: &GraphDiffInput,
    ) -> Result<GraphDiffResult> {
        graph_store::diff(workspace, input)
    }

    pub fn traceability_status(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
    ) -> Result<TraceabilityStatus> {
        let known_checks = self.known_checks(workspace)?;
        self.intelligence.traceability_status(
            workspace_id,
            workspace,
            &self.code_index,
            &known_checks,
        )
    }

    pub fn drift_status(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        review: &ChangeReviewReport,
    ) -> Result<DriftStatus> {
        let known_checks = self.known_checks(workspace)?;
        self.intelligence.drift_status(
            workspace_id,
            workspace,
            &self.code_index,
            &known_checks,
            review,
        )
    }

    pub fn risk_status(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        review: &ChangeReviewReport,
    ) -> Result<RiskStatus> {
        let known_checks = self.known_checks(workspace)?;
        self.intelligence.risk_status(
            workspace_id,
            workspace,
            &self.code_index,
            &known_checks,
            review,
        )
    }

    pub fn impact_analysis(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        review: &ChangeReviewReport,
    ) -> Result<ImpactAnalysis> {
        let known_checks = self.known_checks(workspace)?;
        self.intelligence.impact_analysis(
            workspace_id,
            workspace,
            &self.code_index,
            &known_checks,
            review,
        )
    }

    pub fn verification_plan(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        review: &ChangeReviewReport,
    ) -> Result<VerificationPlan> {
        let known_checks = self.known_checks(workspace)?;
        self.intelligence.create_verification_plan(
            workspace_id,
            workspace,
            &self.code_index,
            &known_checks,
            review,
        )
    }

    pub fn software_context(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        query: &str,
        intent: &str,
        budget: usize,
        requested_scopes: &[String],
    ) -> Result<SoftwareContext> {
        let known_checks = self.known_checks(workspace)?;
        let request = SoftwareContextRequest {
            query: query.to_owned(),
            intent: intent.to_owned(),
            budget,
            scopes: requested_scopes.to_vec(),
        };
        self.intelligence.software_context(
            workspace_id,
            workspace,
            &self.code_index,
            &known_checks,
            &request,
        )
    }

    pub fn semantic_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        limit: usize,
    ) -> Result<SemanticStatusView> {
        self.intelligence
            .semantic_status(workspace_id, workspace, limit)
    }

    pub fn semantic_query(
        &self,
        workspace: &Workspace,
        query: &str,
        requested_scopes: &[String],
        include_candidates: bool,
        limit: usize,
    ) -> Result<Vec<SemanticMatch>> {
        self.intelligence.semantic_query(
            workspace,
            query,
            requested_scopes,
            include_candidates,
            limit,
        )
    }

    pub fn semantic_record_candidate(
        &self,
        workspace: &Workspace,
        input: SemanticCandidateInput,
    ) -> Result<SemanticFact> {
        self.intelligence
            .semantic_record_candidate(workspace, input)
    }

    pub fn semantic_confirm(
        &self,
        workspace: &Workspace,
        fact_id: &str,
        attested_by: &str,
    ) -> Result<SemanticFact> {
        self.intelligence
            .semantic_confirm(workspace, fact_id, attested_by)
    }

    pub fn semantic_retire(
        &self,
        workspace: &Workspace,
        fact_id: &str,
        attested_by: &str,
    ) -> Result<SemanticFact> {
        self.intelligence
            .semantic_retire(workspace, fact_id, attested_by)
    }

    pub fn reconciliation_plan(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        review: &ChangeReviewReport,
    ) -> Result<ReconciliationPlan> {
        let known_checks = self.known_checks(workspace)?;
        self.intelligence.reconciliation_plan(
            workspace_id,
            workspace,
            &self.code_index,
            &known_checks,
            review,
        )
    }

    pub fn reconciliation_status(
        &self,
        workspace: &Workspace,
        plan_id: &str,
    ) -> Result<ReconciliationPlan> {
        self.intelligence.reconciliation_status(workspace, plan_id)
    }

    pub fn reconciliation_history(
        &self,
        workspace: &Workspace,
        limit: usize,
    ) -> Result<Vec<ReconciliationPlan>> {
        self.intelligence.reconciliation_history(workspace, limit)
    }

    pub fn reconciliation_execution_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
    ) -> Result<ReconciliationExecutionStatus> {
        self.intelligence
            .reconciliation_execution_status(workspace_id, workspace, plan_id)
    }

    pub fn reconciliation_claim(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        executor: &str,
        kinds: &[ReconciliationTaskKind],
    ) -> Result<ReconciliationTaskRun> {
        self.intelligence
            .reconciliation_claim(workspace_id, workspace, plan_id, executor, kinds)
    }

    pub fn reconciliation_submit(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        task_id: &str,
        executor: &str,
        submission: ReconciliationTaskSubmission,
    ) -> Result<ReconciliationTaskRun> {
        self.intelligence.reconciliation_submit(
            workspace_id,
            workspace,
            plan_id,
            task_id,
            executor,
            submission,
        )
    }

    pub fn reconciliation_retry(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        task_id: &str,
    ) -> Result<ReconciliationTaskRun> {
        self.intelligence
            .reconciliation_retry(workspace_id, workspace, plan_id, task_id)
    }

    pub fn evidence_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        subject: Option<&str>,
        limit: usize,
    ) -> Result<EvidenceStatus> {
        self.intelligence
            .evidence_status(workspace_id, workspace, subject, limit)
    }

    pub fn verification_claim(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        reviewer: &str,
        capabilities: &[String],
        role: Option<ReviewerRole>,
    ) -> Result<VerificationJob> {
        self.intelligence
            .verification_claim(workspace_id, workspace, reviewer, capabilities, role)
    }

    pub fn verification_submit(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        job_id: &str,
        reviewer: &str,
        submission: ReviewSubmission,
    ) -> Result<VerificationJob> {
        self.intelligence
            .verification_submit(workspace_id, workspace, job_id, reviewer, submission)
    }

    pub fn verification_executor_status(
        &self,
        workspace: &Workspace,
    ) -> Result<StageExecutorRegistry> {
        stage_executor::registry(workspace)
    }

    pub fn language_quality_status(
        &self,
        workspace: &Workspace,
    ) -> Result<LanguageQualityRegistry> {
        quality_provider::registry(workspace)
    }

    pub async fn language_quality_run(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        language: crate::semantic_provider::SemanticLanguage,
        provider_id: &str,
        timeout_seconds: u64,
    ) -> Result<LanguageQualityRun> {
        let started = Instant::now();
        let mut run =
            quality_provider::execute(workspace, language, provider_id, timeout_seconds).await?;
        let elapsed_ms = started.elapsed().as_millis();
        let check = VerificationCheck {
            id: format!("quality-{}-{}", run.capability.as_str(), run.provider_id),
            phase: 0,
            command: command_text(&run.command.program, &run.command.args),
            reason: format!(
                "Run the repository-declared {} provider for {}.",
                run.capability.as_str(),
                language.as_str()
            ),
            success: run.success,
            exit_code: run.command.exit_code,
            elapsed_ms,
            stdout_tail: tail_chars(&run.command.stdout, MAX_CHECK_OUTPUT_CHARS).0,
            stderr_tail: tail_chars(&run.command.stderr, MAX_CHECK_OUTPUT_CHARS).0,
            output_truncated: run.command.truncated,
        };
        let report = VerificationReport {
            workspace: workspace_id.to_owned(),
            level: "language-quality".to_owned(),
            execution: "repository-declared-check-only-provider".to_owned(),
            phases_run: 1,
            passed: run.success,
            checks_run: 1,
            checks_failed: usize::from(!run.success),
            elapsed_ms,
            summary: run.summary.clone(),
            checks: vec![check],
        };
        run.evidence_records = self
            .intelligence
            .record_verification_report(workspace_id, workspace, &report)?
            .len();
        Ok(run)
    }

    pub async fn verification_execute_stages(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
    ) -> Result<Value> {
        let before = self.verification_status(workspace_id, workspace, plan_id)?;
        let registry = stage_executor::registry(workspace)?;
        let mut required = Vec::new();
        if before.plan.require_property {
            required.push(crate::verification::VerificationStage::Property);
        }
        if before.plan.require_mutation {
            required.push(crate::verification::VerificationStage::Mutation);
        }
        if before.plan.require_fuzz {
            required.push(crate::verification::VerificationStage::Fuzz);
        }
        if before
            .plan
            .deterministic_checks
            .iter()
            .any(|check| check == "runtime-gate")
        {
            required.push(crate::verification::VerificationStage::RuntimeCanary);
        }

        let detected = registry
            .detected_languages
            .iter()
            .copied()
            .collect::<BTreeSet<_>>();
        let mut results = Vec::<StageExecutionResult>::new();
        let mut missing = Vec::new();
        let mut skipped_passing = Vec::new();
        let mut execution_errors = Vec::new();
        for stage in required {
            let key = format!("{stage:?}").to_ascii_lowercase();
            let stage_already_passed = before
                .stage_results
                .get(&key)
                .is_some_and(|result| *result == crate::evidence::EvidenceResult::Pass);
            let executors = registry
                .executors
                .iter()
                .filter(|executor| {
                    executor.available
                        && executor.spec.stage == stage
                        && (executor.spec.languages.is_empty()
                            || executor
                                .spec
                                .languages
                                .iter()
                                .any(|language| detected.contains(language)))
                })
                .collect::<Vec<_>>();
            if executors.is_empty() {
                if !stage_already_passed {
                    missing.push(key);
                }
                continue;
            }
            for executor in executors {
                let producer = format!("executor:{}", executor.spec.id);
                if before
                    .stage_producer_results
                    .get(&key)
                    .and_then(|results| results.get(&producer))
                    .is_some_and(|result| *result == crate::evidence::EvidenceResult::Pass)
                {
                    skipped_passing.push(executor.spec.id.clone());
                    continue;
                }
                let execution = match stage_executor::execute(workspace, &executor.spec).await {
                    Ok(execution) => execution,
                    Err(error) => {
                        execution_errors.push(json!({
                            "executor_id": executor.spec.id,
                            "stage": key,
                            "error": error.to_string(),
                        }));
                        continue;
                    }
                };
                self.intelligence.verification_stage_submit(
                    workspace_id,
                    workspace,
                    plan_id,
                    StageSubmission {
                        stage: execution.stage,
                        producer,
                        verdict: execution.verdict,
                        summary: execution.summary.clone(),
                        artifact_digest: execution.artifact_digest.clone(),
                        model: None,
                    },
                )?;
                results.push(execution);
            }
        }
        let after = self.verification_status(workspace_id, workspace, plan_id)?;
        Ok(json!({
            "workspace": workspace_id,
            "plan_id": plan_id,
            "results": results,
            "skipped_passing_executors": skipped_passing,
            "execution_errors": execution_errors,
            "missing_executors": missing,
            "status": after,
        }))
    }

    pub fn verification_stage_submit(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        submission: StageSubmission,
    ) -> Result<crate::evidence::Evidence> {
        self.intelligence
            .verification_stage_submit(workspace_id, workspace, plan_id, submission)
    }

    pub fn verification_approve(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
        approver: &str,
        statement: &str,
    ) -> Result<crate::evidence::Evidence> {
        self.intelligence.verification_approve(
            workspace_id,
            workspace,
            plan_id,
            approver,
            statement,
        )
    }

    pub fn verification_status(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        plan_id: &str,
    ) -> Result<VerificationStatus> {
        let status = self
            .intelligence
            .verification_status(workspace_id, workspace, plan_id)?;
        if status.plan.workspace != workspace_id {
            bail!("verification plan does not belong to the selected workspace");
        }
        Ok(status)
    }

    pub fn verification_history(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        limit: usize,
    ) -> Result<Vec<VerificationStatus>> {
        self.intelligence
            .verification_history(workspace_id, workspace, limit)
    }

    fn known_checks(&self, workspace: &Workspace) -> Result<HashSet<String>> {
        let (profile, _) = self.load_project_profile(workspace)?;
        Ok(profile
            .recommended_checks
            .iter()
            .map(|check| check.id.clone())
            .collect())
    }

    pub fn file_outline(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        path: &str,
        max_symbols: usize,
    ) -> Result<Value> {
        self.code_index
            .file_outline(workspace_id, workspace, path, max_symbols)
    }

    pub fn find_symbol(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        query: &str,
        path: &str,
        kind: Option<&str>,
        max_results: usize,
    ) -> Result<Value> {
        self.code_index
            .find_symbol(workspace_id, workspace, query, path, kind, max_results)
    }

    pub fn symbol_context(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        symbol_id: &str,
        max_body_lines: usize,
    ) -> Result<Value> {
        self.code_index
            .symbol_context(workspace_id, workspace, symbol_id, max_body_lines)
    }

    pub fn invalidate_code_file(&self, workspace: &Workspace, path: &str) {
        self.code_index.invalidate(workspace.root(), path);
    }

    pub fn invalidate_code_prefix(&self, workspace: &Workspace, path: &str) {
        self.code_index.invalidate_prefix(workspace.root(), path);
    }

    pub async fn acquire(&self) -> Result<OwnedSemaphorePermit, String> {
        self.slots
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "tool harness is shutting down".to_owned())
    }

    pub fn project_context(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
    ) -> Result<ProjectContext> {
        let (profile, cache_hit) = self.load_project_profile(workspace)?;
        let (conventions, language_quality) = rayon::join(
            || self.convention_status(workspace),
            || self.language_quality_status(workspace),
        );
        let conventions = conventions?;
        let language_quality = language_quality?;
        Ok(ProjectContext {
            workspace: workspace_id.into(),
            cache_hit,
            root: profile.root.clone(),
            project_types: profile.project_types.clone(),
            manifests: profile.manifests.clone(),
            guidance: profile.guidance.clone(),
            recommended_checks: profile.recommended_checks.clone(),
            workflow: profile.workflow.clone(),
            write_enabled: profile.write_enabled,
            exec_enabled: profile.exec_enabled,
            product_scopes: scopes::registry(),
            conventions,
            language_quality,
        })
    }

    pub async fn review_changes(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        timeout_seconds: u64,
        monitor: &TaskMonitor,
    ) -> Result<ChangeReviewReport> {
        if !workspace.exec_enabled() {
            bail!("change review requires command execution; restart without --no-exec");
        }
        if !workspace.root().join(".git").exists() {
            bail!(
                "change review requires the configured workspace root to be a Git repository root"
            );
        }

        let workspace_id = workspace_id.into();
        let (profile, _) = self.load_project_profile(workspace)?;
        let mut tasks = JoinSet::new();
        for spec in review_probe_specs() {
            let harness = self.clone();
            let monitor = monitor.clone();
            let workspace = workspace.clone();
            let workspace_id = workspace_id.clone();
            tasks.spawn(async move {
                run_review_probe(
                    harness,
                    monitor,
                    workspace_id,
                    workspace,
                    spec,
                    timeout_seconds,
                )
                .await
            });
        }

        let mut outputs = Vec::new();
        while let Some(joined) = tasks.join_next().await {
            outputs.push(match joined {
                Ok(output) => output,
                Err(error) => ReviewProbeOutput {
                    id: "internal-join-error".to_owned(),
                    result: None,
                    elapsed_ms: 0,
                    error: Some(error.to_string()),
                },
            });
        }
        outputs.sort_by(|left, right| left.id.cmp(&right.id));

        let status = outputs
            .iter()
            .find(|output| output.id == "status")
            .ok_or_else(|| anyhow::anyhow!("change review did not receive Git status output"))?;
        let status_result = status.result.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "git status failed: {}",
                status
                    .error
                    .as_deref()
                    .unwrap_or("the probe returned no result")
            )
        })?;
        if !status_result.success {
            bail!(
                "git status failed: {}",
                probe_failure_text(status_result).unwrap_or_else(|| "unknown error".to_owned())
            );
        }

        let (mut changed, mut truncated) = parse_git_status(&status_result.stdout);
        for probe_id in ["unstaged-numstat", "staged-numstat"] {
            if let Some(result) = outputs
                .iter()
                .find(|output| output.id == probe_id)
                .and_then(|output| output.result.as_ref())
            {
                if result.success {
                    truncated |= merge_numstat(&mut changed, &result.stdout);
                }
            }
        }

        let mut findings = Vec::new();
        for probe_id in ["unstaged-check", "staged-check"] {
            if let Some(output) = outputs.iter().find(|output| output.id == probe_id) {
                append_diff_check_findings(&mut findings, output);
            }
        }
        for probe_id in ["unstaged-numstat", "staged-numstat"] {
            if let Some(output) = outputs.iter().find(|output| output.id == probe_id) {
                if output.result.as_ref().is_some_and(|result| !result.success)
                    || output.error.is_some()
                {
                    findings.push(ReviewFinding {
                        severity: "warning".to_owned(),
                        code: "incomplete-diff-metrics".to_owned(),
                        message: format!(
                            "The {probe_id} probe failed; line counts may be incomplete."
                        ),
                        paths: Vec::new(),
                    });
                }
            }
        }

        let mut files = Vec::with_capacity(changed.len());
        let mut security_paths = Vec::new();
        let mut manifest_paths = Vec::new();
        let mut deleted_test_paths = Vec::new();
        let mut source_changed = false;
        let mut tests_changed = false;
        let mut docs_only = !changed.is_empty();
        let mut additions = 0u64;
        let mut deletions = 0u64;
        let mut binary_files = 0usize;

        for (path, change) in changed {
            let category = file_category(&path).to_owned();
            source_changed |= category == "source";
            tests_changed |= category == "test";
            docs_only &= category == "docs";
            additions = additions.saturating_add(change.additions);
            deletions = deletions.saturating_add(change.deletions);
            binary_files += usize::from(change.binary);

            let mut risk_reasons = Vec::new();
            if security_sensitive_path(&path) {
                risk_reasons.push("security-sensitive path".to_owned());
                security_paths.push(path.clone());
            }
            if category == "manifest" {
                risk_reasons.push("dependency or build metadata".to_owned());
                manifest_paths.push(path.clone());
            }
            if category == "migration" {
                risk_reasons.push("data migration".to_owned());
            }
            if category == "workflow" {
                risk_reasons.push("automation or release workflow".to_owned());
            }
            if change.status == "deleted" {
                risk_reasons.push("deleted file".to_owned());
                if category == "test" {
                    deleted_test_paths.push(path.clone());
                }
            }

            files.push(ChangedFileReview {
                path,
                status: change.status,
                staged: change.staged,
                unstaged: change.unstaged,
                untracked: change.untracked,
                category,
                additions: change.has_numstat.then_some(change.additions),
                deletions: change.has_numstat.then_some(change.deletions),
                binary: change.binary,
                risk_reasons,
            });
        }

        let staged_files = files.iter().filter(|file| file.staged).count();
        let unstaged_files = files.iter().filter(|file| file.unstaged).count();
        let untracked_files = files.iter().filter(|file| file.untracked).count();
        let files_changed = files.len();
        let total_lines = additions.saturating_add(deletions);
        append_maintainability_findings(workspace, &files, &mut findings);

        if source_changed && !tests_changed {
            findings.push(ReviewFinding {
                severity: "warning".to_owned(),
                code: "source-without-test-change".to_owned(),
                message: "Source files changed without a corresponding test-file change; confirm existing coverage or add a focused regression test."
                    .to_owned(),
                paths: files
                    .iter()
                    .filter(|file| file.category == "source")
                    .take(8)
                    .map(|file| file.path.clone())
                    .collect(),
            });
        }
        if !security_paths.is_empty() {
            findings.push(ReviewFinding {
                severity: "high".to_owned(),
                code: "security-sensitive-change".to_owned(),
                message: "Authentication, authorization, token, crypto, or security-related files changed; review trust boundaries and failure paths explicitly."
                    .to_owned(),
                paths: security_paths.clone(),
            });
        }
        if !manifest_paths.is_empty() {
            findings.push(ReviewFinding {
                severity: "warning".to_owned(),
                code: "manifest-change".to_owned(),
                message: "Dependency or build metadata changed; verify lockfiles and perform a full project check."
                    .to_owned(),
                paths: manifest_paths.clone(),
            });
        }
        if !deleted_test_paths.is_empty() {
            findings.push(ReviewFinding {
                severity: "high".to_owned(),
                code: "deleted-tests".to_owned(),
                message: "Test files were deleted; confirm coverage was intentionally relocated or removed."
                    .to_owned(),
                paths: deleted_test_paths,
            });
        }
        if files_changed > 25 || total_lines > 1_000 {
            findings.push(ReviewFinding {
                severity: "warning".to_owned(),
                code: "large-change-set".to_owned(),
                message: format!(
                    "The change set spans {files_changed} files and approximately {total_lines} changed lines; consider splitting independent concerns."
                ),
                paths: Vec::new(),
            });
        }
        if untracked_files > 0 {
            findings.push(ReviewFinding {
                severity: "info".to_owned(),
                code: "untracked-files".to_owned(),
                message: format!(
                    "{untracked_files} untracked file(s) are part of the working tree review."
                ),
                paths: files
                    .iter()
                    .filter(|file| file.untracked)
                    .take(12)
                    .map(|file| file.path.clone())
                    .collect(),
            });
        }
        if docs_only {
            findings.push(ReviewFinding {
                severity: "info".to_owned(),
                code: "docs-only".to_owned(),
                message: "Only documentation files changed; a quick verification gate is normally sufficient."
                    .to_owned(),
                paths: Vec::new(),
            });
        }
        if truncated {
            findings.push(ReviewFinding {
                severity: "warning".to_owned(),
                code: "review-truncated".to_owned(),
                message: format!(
                    "The review reached its {MAX_REVIEW_FILES}-file bound; inspect the remaining change set separately."
                ),
                paths: Vec::new(),
            });
        }
        findings.truncate(MAX_REVIEW_FINDINGS);

        let high_risk = !security_paths.is_empty()
            || files.iter().any(|file| file.category == "migration")
            || findings.iter().any(|finding| finding.severity == "high")
            || files_changed > 50
            || total_lines > 2_000;
        let moderate_risk = source_changed
            || tests_changed
            || !manifest_paths.is_empty()
            || files.iter().any(|file| file.category == "workflow")
            || files_changed > 10
            || findings
                .iter()
                .any(|finding| matches!(finding.severity.as_str(), "warning" | "error"));
        let risk_level = if high_risk {
            "high"
        } else if moderate_risk {
            "moderate"
        } else {
            "low"
        };
        let recommended_verification = if high_risk
            || tests_changed
            || !manifest_paths.is_empty()
            || total_lines > 500
            || files_changed > 10
        {
            "full"
        } else {
            "quick"
        };
        let recommended_checks = profile
            .recommended_checks
            .iter()
            .filter(|check| recommended_verification == "full" || check.level == "quick")
            .map(|check| check.id.clone())
            .collect::<Vec<_>>();
        let clean = files_changed == 0;
        let summary = if clean {
            "No staged, unstaged, or untracked files were detected.".to_owned()
        } else {
            format!(
                "Reviewed {files_changed} changed file(s): {staged_files} staged, {unstaged_files} unstaged, {untracked_files} untracked; risk {risk_level}, recommend {recommended_verification} verification."
            )
        };
        let probes = outputs.iter().map(review_probe_summary).collect::<Vec<_>>();

        Ok(ChangeReviewReport {
            workspace: workspace_id,
            execution: "parallel-git-probes".to_owned(),
            clean,
            files_changed,
            staged_files,
            unstaged_files,
            untracked_files,
            additions,
            deletions,
            binary_files,
            source_changed,
            tests_changed,
            docs_only,
            risk_level: risk_level.to_owned(),
            recommended_verification: recommended_verification.to_owned(),
            recommended_checks,
            summary,
            files,
            findings,
            probes,
            truncated,
        })
    }

    fn load_project_profile(&self, workspace: &Workspace) -> Result<(Arc<ProjectProfile>, bool)> {
        let root = workspace.root().to_path_buf();
        let fingerprint = project_fingerprint(&root);
        if let Some(profile) = self
            .project_cache
            .lock()
            .map_err(|_| anyhow::anyhow!("project context cache poisoned"))?
            .get(&root)
            .filter(|cached| cached.fingerprint == fingerprint)
            .map(|cached| cached.profile.clone())
        {
            return Ok((profile, true));
        }

        // Build outside the cache lock so context discovery for one workspace does not
        // block independent requests for other workspaces.
        let built = Arc::new(build_project_profile(workspace)?);
        let mut cache = self
            .project_cache
            .lock()
            .map_err(|_| anyhow::anyhow!("project context cache poisoned"))?;
        if let Some(profile) = cache
            .get(&root)
            .filter(|cached| cached.fingerprint == fingerprint)
            .map(|cached| cached.profile.clone())
        {
            return Ok((profile, true));
        }
        cache.insert(
            root,
            CachedProjectProfile {
                fingerprint,
                profile: built.clone(),
            },
        );
        Ok((built, false))
    }

    pub async fn verify_project(
        &self,
        workspace_id: impl Into<String>,
        workspace: &Workspace,
        level: &str,
        timeout_seconds: u64,
        monitor: &TaskMonitor,
    ) -> Result<VerificationReport> {
        if !workspace.exec_enabled() {
            bail!("project verification requires command execution; restart without --no-exec");
        }
        if !matches!(level, "quick" | "full") {
            bail!("verification level must be quick or full");
        }

        let workspace_id = workspace_id.into();
        let (profile, _) = self.load_project_profile(workspace)?;
        let mut plan = profile
            .recommended_checks
            .iter()
            .filter(|check| level == "full" || check.level == "quick")
            .take(MAX_VERIFICATION_CHECKS)
            .cloned()
            .collect::<Vec<_>>();
        sort_checks(&mut plan);

        let phases_run = plan
            .iter()
            .map(|check| check.phase)
            .collect::<HashSet<_>>()
            .len();
        let started = Instant::now();
        let mut checks = Vec::with_capacity(plan.len());
        let mut start = 0usize;

        while start < plan.len() {
            let phase = plan[start].phase;
            let end = plan[start..]
                .iter()
                .position(|check| check.phase != phase)
                .map(|offset| start + offset)
                .unwrap_or(plan.len());
            let mut tasks = JoinSet::new();
            for check in plan[start..end].iter().cloned() {
                let harness = self.clone();
                let monitor = monitor.clone();
                let workspace = workspace.clone();
                let workspace_id = workspace_id.clone();
                tasks.spawn(async move {
                    run_verification_check(
                        harness,
                        monitor,
                        workspace_id,
                        workspace,
                        check,
                        timeout_seconds,
                    )
                    .await
                });
            }
            while let Some(joined) = tasks.join_next().await {
                checks.push(match joined {
                    Ok(check) => check,
                    Err(error) => VerificationCheck {
                        id: "internal-join-error".to_owned(),
                        phase,
                        command: "verification task".to_owned(),
                        reason: "A verification worker failed before returning its result."
                            .to_owned(),
                        success: false,
                        exit_code: None,
                        elapsed_ms: 0,
                        stdout_tail: String::new(),
                        stderr_tail: error.to_string(),
                        output_truncated: false,
                    },
                });
            }
            start = end;
        }

        checks.sort_by(|left, right| {
            left.phase
                .cmp(&right.phase)
                .then_with(|| left.id.cmp(&right.id))
        });
        let checks_failed = checks.iter().filter(|check| !check.success).count();
        let checks_run = checks.len();
        let passed = checks_run > 0 && checks_failed == 0;
        let summary = if checks_run == 0 {
            "No verification commands could be inferred for this project; inspect its guidance and manifests manually."
                .to_owned()
        } else if passed {
            format!(
                "All {checks_run} inferred {level} checks passed across {phases_run} execution phase(s)."
            )
        } else {
            format!(
                "{checks_failed} of {checks_run} inferred {level} checks failed across {phases_run} execution phase(s)."
            )
        };

        let report = VerificationReport {
            workspace: workspace_id.clone(),
            level: level.to_owned(),
            execution: "phased-parallel".to_owned(),
            phases_run,
            passed,
            checks_run,
            checks_failed,
            elapsed_ms: started.elapsed().as_millis(),
            summary,
            checks,
        };
        self.intelligence
            .record_verification_report(&workspace_id, workspace, &report)?;
        Ok(report)
    }
}

#[path = "harness_graph.rs"]
mod harness_graph;
use harness_graph::{design_product_id, overlay_design_graph};

#[path = "harness_scope.rs"]
mod harness_scope;

#[path = "harness_project.rs"]
mod harness_project;

#[path = "harness_review.rs"]
mod harness_review;
use harness_review::*;

#[path = "harness_verification.rs"]
mod harness_verification;
use harness_verification::run_verification_check;
fn build_project_profile(workspace: &Workspace) -> Result<ProjectProfile> {
    let root = workspace.root();
    let manifests = MANIFEST_FILES
        .iter()
        .filter(|path| root.join(path).is_file())
        .map(|path| (*path).to_owned())
        .collect::<Vec<_>>();
    let mut project_types = BTreeSet::new();
    let mut checks = Vec::new();

    if root.join(".git").exists() {
        push_check(
            &mut checks,
            "git-diff-check",
            "quick",
            "git",
            &["diff", "--check"],
            "Detect conflict markers and whitespace errors in the current change.",
        );
    }

    if root.join("Cargo.toml").is_file() {
        project_types.insert("rust".to_owned());
        let locked = root.join("Cargo.lock").is_file();
        push_check(
            &mut checks,
            "rust-format",
            "quick",
            "cargo",
            &["fmt", "--check"],
            "Verify Rust formatting without modifying files.",
        );
        push_cargo_check(
            &mut checks,
            "rust-check",
            "quick",
            "check",
            locked,
            "Type-check the complete Rust workspace.",
        );
        push_cargo_check(
            &mut checks,
            "rust-test",
            "full",
            "test",
            locked,
            "Run the Rust test suite.",
        );
        let mut clippy_args = vec!["clippy".to_owned()];
        if locked {
            clippy_args.push("--locked".to_owned());
        }
        clippy_args.extend(["--".to_owned(), "-D".to_owned(), "warnings".to_owned()]);
        push_check_owned(
            &mut checks,
            "rust-clippy",
            "full",
            "cargo",
            clippy_args,
            "Run Clippy and treat warnings as quality-gate failures.",
        );
        let mut release_args = vec!["build".to_owned(), "--release".to_owned()];
        if locked {
            release_args.push("--locked".to_owned());
        }
        push_check_owned(
            &mut checks,
            "rust-release-build",
            "full",
            "cargo",
            release_args,
            "Build the optimized release binary with the locked dependency graph.",
        );
    }

    if root.join("package.json").is_file() {
        project_types.insert("node".to_owned());
        add_node_checks(root, &mut checks);
    }

    if root.join("pyproject.toml").is_file() || root.join("requirements.txt").is_file() {
        project_types.insert("python".to_owned());
        push_check(
            &mut checks,
            "python-tests",
            "full",
            "pytest",
            &["-q"],
            "Run the Python test suite with concise output.",
        );
    }

    if root.join("go.mod").is_file() {
        project_types.insert("go".to_owned());
        push_check(
            &mut checks,
            "go-tests",
            "full",
            "go",
            &["test", "./..."],
            "Compile and test all Go packages.",
        );
    }

    if root.join("Makefile").is_file() {
        project_types.insert("make".to_owned());
        add_make_checks(root, &mut checks);
    }

    if project_types.is_empty() {
        project_types.insert("generic".to_owned());
    }

    deduplicate_checks(&mut checks);
    let guidance = collect_guidance(workspace)?;
    Ok(ProjectProfile {
        root: root.display().to_string(),
        project_types: project_types.into_iter().collect(),
        manifests,
        guidance,
        recommended_checks: checks,
        workflow: vec![
            "Treat always-on repository guidance as a short map; retrieve detailed Design State, Product Scope, symbol, and language-quality context only when the task needs it.".to_owned(),
            "Read the returned repository guidance before substantial edits.".to_owned(),
            "Call scope_status before broad source inspection; treat relevant unmapped supported source as architecture debt before adding production modules.".to_owned(),
            "Use search_many and read_files to collect relevant implementation and tests in few round trips."
                .to_owned(),
            "Batch writes when targets are already known: use one apply_edits for multiple changes in a file, apply_file_edits for independent existing files, and create_files for independent new files instead of serial single-file tool calls."
                .to_owned(),
            "Use isolated workers or parallel_tools for independent research/review when the host supports it, but never treat worker consensus as deterministic proof.".to_owned(),
            "Keep mandatory policy in deterministic Harness gates and Evidence rather than relying on an agent instruction to remember it.".to_owned(),
            "Prefer the smallest coherent change that preserves existing architecture and public behavior."
                .to_owned(),
            "Read every edited file first and keep SHA-256 preconditions on writes.".to_owned(),
            "Run verify_project with level=quick after edits; run level=full before release-sized changes."
                .to_owned(),
            "Report checks actually run, failures that remain, and any assumptions that were not verified."
                .to_owned(),
        ],
        write_enabled: workspace.write_enabled(),
        exec_enabled: workspace.exec_enabled(),
    })
}

fn collect_guidance(workspace: &Workspace) -> Result<Vec<GuidanceDocument>> {
    let mut remaining = MAX_GUIDANCE_CHARS_TOTAL;
    let mut documents = Vec::new();
    for path in GUIDANCE_FILES {
        if remaining == 0 || !workspace.root().join(path).is_file() {
            continue;
        }
        let view = workspace.read_file(path, 1, Some(MAX_GUIDANCE_LINES_PER_FILE))?;
        let limit = remaining.min(MAX_GUIDANCE_CHARS_PER_FILE);
        let (excerpt, excerpt_truncated) = truncate_chars(&view.content, limit);
        remaining = remaining.saturating_sub(excerpt.chars().count());
        let included_lines = if excerpt.is_empty() {
            0
        } else {
            excerpt.lines().count()
        };
        documents.push(GuidanceDocument {
            path: (*path).to_owned(),
            excerpt,
            included_lines,
            total_lines: view.total_lines,
            truncated: excerpt_truncated || view.end_line < view.total_lines,
            redacted: view.redacted,
        });
    }
    Ok(documents)
}

fn add_node_checks(root: &Path, checks: &mut Vec<CheckSpec>) {
    let Some(content) = read_small_text(&root.join("package.json")) else {
        return;
    };
    let Ok(package) = serde_json::from_str::<Value>(&content) else {
        return;
    };
    let Some(scripts) = package.get("scripts").and_then(Value::as_object) else {
        return;
    };
    let runner = node_runner(root);
    for (name, level, reason) in [
        (
            "lint",
            "quick",
            "Run the repository's JavaScript/TypeScript lint script.",
        ),
        (
            "typecheck",
            "quick",
            "Run the repository's static type-check script.",
        ),
        (
            "check",
            "quick",
            "Run the repository's general validation script.",
        ),
        (
            "format:check",
            "quick",
            "Verify repository formatting without writing files.",
        ),
        (
            "test",
            "full",
            "Run the repository's JavaScript/TypeScript tests.",
        ),
        (
            "build",
            "full",
            "Build the project to catch integration and bundling errors.",
        ),
    ] {
        let Some(command) = scripts.get(name).and_then(Value::as_str) else {
            continue;
        };
        if name == "test" && command.contains("no test specified") {
            continue;
        }
        push_check_owned(
            checks,
            &format!("node-{name}"),
            level,
            &runner,
            vec!["run".to_owned(), name.to_owned()],
            reason,
        );
    }
}

fn add_make_checks(root: &Path, checks: &mut Vec<CheckSpec>) {
    let Some(content) = read_small_text(&root.join("Makefile")) else {
        return;
    };
    let targets = content
        .lines()
        .filter_map(|line| {
            let line = line.trim_end();
            if line.starts_with(['\t', '#', ' ']) {
                return None;
            }
            line.split_once(':')
                .map(|(target, _)| target.trim())
                .filter(|target| !target.is_empty() && !target.contains(char::is_whitespace))
                .map(str::to_owned)
        })
        .collect::<HashSet<_>>();
    for (target, level, reason) in [
        (
            "check",
            "quick",
            "Run the Makefile's repository validation target.",
        ),
        ("lint", "quick", "Run the Makefile's lint target."),
        ("test", "full", "Run the Makefile's test target."),
    ] {
        if targets.contains(target) {
            push_check(
                checks,
                &format!("make-{target}"),
                level,
                "make",
                &[target],
                reason,
            );
        }
    }
}

fn read_small_text(path: &Path) -> Option<String> {
    let metadata = fs::metadata(path).ok()?;
    if !metadata.is_file() || metadata.len() > MAX_PROFILE_SOURCE_BYTES {
        return None;
    }
    fs::read_to_string(path).ok()
}

fn node_runner(root: &Path) -> String {
    if root.join("pnpm-lock.yaml").is_file() {
        "pnpm"
    } else if root.join("yarn.lock").is_file() {
        "yarn"
    } else if root.join("bun.lock").is_file() || root.join("bun.lockb").is_file() {
        "bun"
    } else {
        "npm"
    }
    .to_owned()
}

fn push_cargo_check(
    checks: &mut Vec<CheckSpec>,
    id: &str,
    level: &str,
    subcommand: &str,
    locked: bool,
    reason: &str,
) {
    let mut args = vec![subcommand.to_owned()];
    if locked {
        args.push("--locked".to_owned());
    }
    push_check_owned(checks, id, level, "cargo", args, reason);
}

fn push_check(
    checks: &mut Vec<CheckSpec>,
    id: &str,
    level: &str,
    program: &str,
    args: &[&str],
    reason: &str,
) {
    push_check_owned(
        checks,
        id,
        level,
        program,
        args.iter().map(|arg| (*arg).to_owned()).collect(),
        reason,
    );
}

fn push_check_owned(
    checks: &mut Vec<CheckSpec>,
    id: &str,
    level: &str,
    program: &str,
    args: Vec<String>,
    reason: &str,
) {
    checks.push(CheckSpec {
        id: id.to_owned(),
        level: level.to_owned(),
        phase: verification_phase(id),
        program: program.to_owned(),
        args,
        reason: reason.to_owned(),
    });
}

fn deduplicate_checks(checks: &mut Vec<CheckSpec>) {
    let mut seen = HashSet::new();
    checks.retain(|check| seen.insert((check.program.clone(), check.args.clone())));
    sort_checks(checks);
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

#[path = "harness_text.rs"]
mod harness_text;
use harness_text::{command_text, tail_chars, truncate_chars};

fn project_fingerprint(root: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    root.hash(&mut hasher);
    for relative in PROFILE_FILES {
        relative.hash(&mut hasher);
        let path = root.join(relative);
        if let Ok(metadata) = fs::metadata(path) {
            metadata.len().hash(&mut hasher);
            metadata.is_file().hash(&mut hasher);
            if let Ok(modified) = metadata.modified() {
                if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                    duration.as_nanos().hash(&mut hasher);
                }
            }
        }
    }
    hasher.finish()
}

#[cfg(test)]
#[path = "harness_tests.rs"]
mod tests;
