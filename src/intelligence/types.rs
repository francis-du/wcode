use crate::design::{DesignDiagnostic, Priority};
use crate::evidence::Evidence;
use crate::graph::{EdgeKind, GraphPrecision, NodeKind};
use crate::risk::{Risk, RiskLevel, VerificationProfile};
use crate::semantic::{SemanticFact, SemanticMatch};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize)]
pub struct DesignStatus {
    pub workspace: String,
    pub initialized: bool,
    pub valid: bool,
    pub schema_version: u32,
    pub design_root: String,
    pub files_loaded: usize,
    pub project: Option<String>,
    pub requirements: usize,
    pub components: usize,
    pub constraints: usize,
    pub decisions: usize,
    pub acceptance_criteria: usize,
    pub design_nodes: usize,
    pub errors: usize,
    pub warnings: usize,
    pub diagnostics: Vec<DesignDiagnostic>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CoverageDimension {
    pub covered: usize,
    pub total: usize,
    pub percent: u8,
}

impl CoverageDimension {
    pub(crate) fn new(covered: usize, total: usize) -> Self {
        let percent = covered
            .saturating_mul(100)
            .checked_div(total)
            .and_then(|value| u8::try_from(value).ok())
            .unwrap_or(0);
        Self {
            covered,
            total,
            percent,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TraceReferenceKind {
    File,
    Symbol,
    Test,
    Check,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceReference {
    pub owner: String,
    pub kind: TraceReferenceKind,
    pub target: String,
    pub resolved: bool,
    pub provider: String,
    pub precision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RequirementTraceStatus {
    Complete,
    Partial,
    Missing,
}

#[derive(Clone, Debug, Serialize)]
pub struct RequirementTrace {
    pub id: String,
    pub title: String,
    pub priority: Priority,
    pub components: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub implementation: Vec<TraceReference>,
    pub verification: Vec<TraceReference>,
    pub status: RequirementTraceStatus,
}

#[derive(Clone, Debug, Serialize)]
pub struct TraceabilityStatus {
    pub workspace: String,
    pub initialized: bool,
    pub valid_design: bool,
    pub requirements_total: usize,
    pub requirements_returned: usize,
    pub truncated: bool,
    pub requirement_to_component: CoverageDimension,
    pub design_to_implementation: CoverageDimension,
    pub acceptance_to_verification: CoverageDimension,
    pub complete_requirements: usize,
    pub partial_requirements: usize,
    pub missing_requirements: usize,
    pub requirements: Vec<RequirementTrace>,
    pub diagnostics: Vec<DesignDiagnostic>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DriftKind {
    ImplementationDrift,
    DesignDrift,
}

#[derive(Clone, Debug, Serialize)]
pub struct DriftFinding {
    pub id: String,
    pub kind: DriftKind,
    pub risk_level: RiskLevel,
    pub subject: String,
    pub message: String,
    pub affected_requirements: Vec<String>,
    pub paths: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct DriftStatus {
    pub workspace: String,
    pub design_changed: bool,
    pub implementation_changed: bool,
    pub implementation_drift: usize,
    pub design_drift: usize,
    pub findings: Vec<DriftFinding>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct RiskStatus {
    pub workspace: String,
    pub level: RiskLevel,
    pub profile: VerificationProfile,
    pub risks: Vec<Risk>,
    pub drift: DriftStatus,
    pub traceability: TraceabilityStatus,
}

#[derive(Clone, Debug, Serialize)]
pub struct EvidenceStatus {
    pub workspace: String,
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub inconclusive: usize,
    pub disagreed: usize,
    pub deterministic: usize,
    pub evidence: Vec<Evidence>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct SemanticStatusView {
    pub workspace: String,
    pub total: usize,
    pub candidates: usize,
    pub confirmed: usize,
    pub retired: usize,
    pub facts: Vec<SemanticFact>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct DesignContextItem {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub summary: String,
    pub relations: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, Serialize)]
pub struct GraphContextNode {
    pub id: String,
    pub kind: NodeKind,
    pub label: String,
    pub path: Option<String>,
    pub provider: String,
    pub precision: GraphPrecision,
    pub score: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct GraphContextEdge {
    pub from: String,
    pub to: String,
    pub kind: EdgeKind,
    pub provider: String,
    pub precision: GraphPrecision,
}

#[derive(Clone, Debug, Default, Serialize)]
pub struct GraphContext {
    pub nodes: Vec<GraphContextNode>,
    pub edges: Vec<GraphContextEdge>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct SoftwareContext {
    pub workspace: String,
    pub query: String,
    pub intent: String,
    pub budget: usize,
    pub scopes: Vec<String>,
    pub requirements: Vec<String>,
    pub components: Vec<String>,
    pub constraints: Vec<String>,
    pub acceptance_criteria: Vec<String>,
    pub decisions: Vec<String>,
    pub design_items: Vec<DesignContextItem>,
    pub semantic_matches: Vec<SemanticMatch>,
    pub symbols: Vec<serde_json::Value>,
    pub graph_context: GraphContext,
    pub known_risks: Vec<Risk>,
    pub coverage: TraceabilityStatus,
}

#[derive(Clone, Debug)]
pub struct SoftwareContextRequest {
    pub query: String,
    pub intent: String,
    pub budget: usize,
    pub scopes: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CodeStatBreakdown {
    pub name: String,
    pub files: usize,
    pub lines: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectCodeStats {
    pub source_files: usize,
    pub source_lines: usize,
    pub source_bytes: u64,
    pub symbols: usize,
    pub call_edges: usize,
    pub languages: Vec<CodeStatBreakdown>,
    pub product_scopes: Vec<CodeStatBreakdown>,
    pub changed_files: usize,
    pub changed_source_files: usize,
    pub additions: u64,
    pub deletions: u64,
    pub untracked_files: usize,
    pub graph_truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectFileView {
    pub path: String,
    pub language: String,
    pub lines: usize,
    pub bytes: u64,
    pub depth: usize,
    pub over_limit: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectStructureView {
    pub entries: Vec<ProjectFileView>,
    pub largest_files: Vec<ProjectFileView>,
    pub directory_count: usize,
    pub max_depth: usize,
    pub oversized_files: usize,
    pub line_limit: usize,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct FeatureImplementationView {
    pub target: String,
    pub path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    pub resolved: bool,
    pub provider: String,
    pub precision: String,
    pub changed: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct FeatureComponentView {
    pub id: String,
    pub name: String,
    pub responsibilities: Vec<String>,
    pub depends_on: Vec<String>,
    pub implementation: Vec<FeatureImplementationView>,
    pub implementation_lines: usize,
    pub changed: bool,
    pub changed_paths: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FeatureAcceptanceView {
    pub id: String,
    pub title: String,
    pub statement: String,
    pub verification: Vec<FeatureImplementationView>,
}

#[derive(Clone, Debug, Serialize)]
pub struct FeatureConstraintView {
    pub id: String,
    pub title: String,
    pub statement: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct FeatureDecisionView {
    pub id: String,
    pub title: String,
    pub status: String,
    pub decision: String,
    pub rationale: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct FeatureDependencyAlignment {
    pub from: String,
    pub from_name: String,
    pub to: String,
    pub to_name: String,
    pub desired: bool,
    pub actual: bool,
    pub status: String,
    pub precision: String,
    pub blocking: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FeatureConvergenceState {
    Stable,
    Changing,
    NeedsConvergence,
    Incomplete,
}

#[derive(Clone, Debug, Serialize)]
pub struct FeatureRequirementView {
    pub id: String,
    pub title: String,
    pub intent: String,
    pub priority: Priority,
    pub status: RequirementTraceStatus,
    pub aligned: bool,
    pub convergence: FeatureConvergenceState,
    pub convergence_blockers: Vec<String>,
    pub changed: bool,
    pub change_additions: u64,
    pub change_deletions: u64,
    pub implementation_files: usize,
    pub implementation_symbols: usize,
    pub implementation_lines: usize,
    pub components: Vec<FeatureComponentView>,
    pub acceptance: Vec<FeatureAcceptanceView>,
    pub constraints: Vec<FeatureConstraintView>,
    pub decisions: Vec<FeatureDecisionView>,
    pub dependency_alignment: Vec<FeatureDependencyAlignment>,
    pub drift: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectChangeView {
    pub path: String,
    pub status: String,
    pub category: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    pub additions: Option<u64>,
    pub deletions: Option<u64>,
    pub staged: bool,
    pub unstaged: bool,
    pub untracked: bool,
    pub affected_requirements: Vec<String>,
    pub affected_components: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectRevisionView {
    pub id: String,
    pub captured_at_ms: u64,
    pub nodes: usize,
    pub edges: usize,
    pub files_indexed: usize,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectGraphDeltaView {
    pub from_snapshot_id: String,
    pub to_snapshot_id: String,
    pub from_captured_at_ms: u64,
    pub to_captured_at_ms: u64,
    pub added_nodes: usize,
    pub removed_nodes: usize,
    pub changed_nodes: usize,
    pub added_edges: usize,
    pub removed_edges: usize,
    pub changed_edges: usize,
    pub changed_paths: Vec<String>,
    pub truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectProofSummary {
    pub revision_code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub revision_design: Option<String>,
    pub current_evidence: usize,
    pub current_passed: usize,
    pub current_failed: usize,
    pub current_inconclusive: usize,
    pub current_disagreed: usize,
    pub current_verification_plans: usize,
    pub current_verification_ready: usize,
    pub current_verification_blocked: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_current_evidence_at_ms: Option<u64>,
    pub evidence_scan_truncated: bool,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectGraphPrecisionSummary {
    pub primary: String,
    pub providers: Vec<String>,
    pub declared_edges: usize,
    pub syntax_edges: usize,
    pub semantic_edges: usize,
    pub deterministic_edges: usize,
    pub runtime_edges: usize,
    pub heuristic_edges: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectConvergenceSummary {
    pub stable_requirements: usize,
    pub changing_requirements: usize,
    pub needs_convergence_requirements: usize,
    pub incomplete_requirements: usize,
    pub reconciliation_plans: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_reconciliation_plan: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectArchitectureComponentView {
    pub id: String,
    pub name: String,
    pub responsibilities: Vec<String>,
    pub depends_on: Vec<String>,
    pub implementation_targets: Vec<String>,
    pub implementation_files: usize,
    pub implementation_lines: usize,
    pub changed: bool,
    pub changed_paths: Vec<String>,
    pub requirements: Vec<String>,
    pub product_scopes: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectArchitectureView {
    pub components: Vec<ProjectArchitectureComponentView>,
    pub dependencies: Vec<FeatureDependencyAlignment>,
    pub desired_edges: usize,
    pub observed_edges: usize,
    pub aligned_edges: usize,
    pub blocking_drift_edges: usize,
    pub advisory_edges: usize,
    pub unverified_edges: usize,
    pub components_with_implementation: usize,
    pub observed_drift_percent: f64,
    pub evidence_coverage_percent: f64,
    pub implementation_coverage_percent: f64,
}

#[derive(Clone, Debug, Serialize)]
pub struct ProjectObservatory {
    pub workspace: String,
    pub root: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product_vision: Option<String>,
    pub product_principles: Vec<String>,
    pub design_valid: bool,
    pub coverage: TraceabilityStatus,
    pub code: ProjectCodeStats,
    pub structure: ProjectStructureView,
    pub graph_precision: ProjectGraphPrecisionSummary,
    pub language_quality: crate::quality_provider::LanguageQualityRegistry,
    pub proof: ProjectProofSummary,
    pub convergence: ProjectConvergenceSummary,
    pub architecture: ProjectArchitectureView,
    pub requirements: Vec<FeatureRequirementView>,
    pub changes: Vec<ProjectChangeView>,
    pub history: Vec<ProjectRevisionView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub latest_delta: Option<ProjectGraphDeltaView>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub impact: Option<crate::reconcile::ImpactAnalysis>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub risk: Option<RiskStatus>,
}
