use crate::code_index::{CodeIndex, SymbolResolution};
use crate::design::{self, CodeRef, Priority, VerificationRef};
use crate::evidence::{Confidence, Evidence, EvidenceKind, EvidenceResult, Revision};
use crate::evidence_store;
use crate::graph::{EdgeKind, NodeKind, SoftwareGraphSnapshot};
use crate::graph_provider_store;
use crate::harness::{ChangeReviewReport, VerificationReport};
use crate::reconcile::{
    ChangeIntent, DesignChange, DesignChangeKind, ImpactAnalysis, ReconciliationExecution,
    ReconciliationExecutionStatus, ReconciliationPlan, ReconciliationRunStatus, ReconciliationTask,
    ReconciliationTaskKind, ReconciliationTaskRun, ReconciliationTaskSubmission,
};
use crate::reconciliation_execution_store;
use crate::reconciliation_store;
use crate::risk::{Risk, RiskCategory, RiskLevel, VerificationProfile};
use crate::scopes;
use crate::semantic::{self, SemanticCandidateInput, SemanticFact, SemanticMatch, SemanticStatus};
use crate::semantic_store;
use crate::verification::{
    ReviewSubmission, ReviewerRole, StageSubmission, VerificationJob, VerificationPlan,
    VerificationStage, VerificationState, VerificationStatus,
};
use crate::verification_store;
use crate::workspace::Workspace;
use anyhow::{anyhow, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::sync::{Arc, Mutex};
use uuid::Uuid;

const MAX_EVIDENCE_RECORDS: usize = 4_096;
const MAX_DRIFT_FINDINGS: usize = 256;
const MAX_CONTEXT_ITEMS: usize = 64;
const MAX_REVISION_FILES: usize = 10_000;
const MAX_IMPACT_GRAPH_FILES: usize = 1_500;
const MAX_IMPACT_GRAPH_SYMBOLS: usize = 5_000;
const MAX_TRANSITIVE_IMPACT_SYMBOLS: usize = 2_000;

#[derive(Default)]
struct IntelligenceState {
    evidence: Vec<StoredEvidence>,
    verification: VerificationState,
    verification_loaded: BTreeSet<String>,
    latest_risks: BTreeMap<String, Vec<Risk>>,
}

#[derive(Clone, Debug, Serialize)]
struct StoredEvidence {
    workspace: String,
    evidence: Evidence,
}

#[derive(Clone)]
pub struct SoftwareIntelligenceRuntime {
    state: Arc<Mutex<IntelligenceState>>,
}

impl Default for SoftwareIntelligenceRuntime {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(IntelligenceState::default())),
        }
    }
}

pub use crate::intelligence_types::{
    CodeStatBreakdown, CoverageDimension, DesignContextItem, DesignStatus, DriftFinding, DriftKind,
    DriftStatus, EvidenceStatus, FeatureAcceptanceView, FeatureComponentView,
    FeatureConstraintView, FeatureConvergenceState, FeatureDecisionView,
    FeatureDependencyAlignment, FeatureImplementationView, FeatureRequirementView, GraphContext,
    GraphContextEdge, GraphContextNode, ProjectChangeView, ProjectCodeStats,
    ProjectConvergenceSummary, ProjectFileView, ProjectGraphDeltaView, ProjectObservatory,
    ProjectProofSummary, ProjectRevisionView, ProjectStructureView, RequirementTrace,
    RequirementTraceStatus, RiskStatus, SemanticStatusView, SoftwareContext,
    SoftwareContextRequest, TraceReference, TraceReferenceKind, TraceabilityStatus,
};

#[path = "analysis.rs"]
mod analysis;
#[path = "context.rs"]
mod context;
#[path = "observatory.rs"]
mod observatory;
#[path = "observatory/architecture.rs"]
mod observatory_architecture;
#[path = "observatory/files.rs"]
mod observatory_files;
use analysis::*;
use context::*;
pub(crate) use observatory::{build_project_observatory, ObservatoryInput};

const MAX_TRACE_REQUIREMENTS: usize = 200;
const MAX_TRACE_DIAGNOSTICS: usize = 128;

#[path = "runtime/design.rs"]
mod design_runtime;
#[path = "runtime/reconcile.rs"]
mod reconcile_runtime;
#[path = "runtime/semantic.rs"]
mod semantic_runtime;

impl SoftwareIntelligenceRuntime {
    fn create_plan_for_risk(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        risk_level: RiskLevel,
    ) -> Result<VerificationPlan> {
        self.ensure_verification_loaded(workspace_id, workspace)?;
        let revision = workspace_revision(workspace)?;
        let subject = format!("change:{}", revision.code);
        let plan_id = self.next_id("VP");
        let job_ids = std::iter::repeat_with(|| self.next_id("VJ"));
        let (plan, snapshot) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            let plan = state.verification.create_plan(
                plan_id,
                workspace_id.to_owned(),
                subject,
                risk_level,
                job_ids,
            )?;
            let snapshot = state.verification.workspace_snapshot(workspace_id);
            (plan, snapshot)
        };
        verification_store::persist(workspace, &snapshot)?;
        Ok(plan)
    }

    fn ensure_verification_loaded(&self, workspace_id: &str, workspace: &Workspace) -> Result<()> {
        {
            let state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            if state.verification_loaded.contains(workspace_id) {
                return Ok(());
            }
        }
        let persisted = verification_store::load(workspace)?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow!("software intelligence state poisoned"))?;
        if state.verification_loaded.contains(workspace_id) {
            return Ok(());
        }
        if let Some(snapshot) = persisted {
            state.verification.restore_workspace(snapshot)?;
        }
        state.verification_loaded.insert(workspace_id.to_owned());
        Ok(())
    }

    fn next_id(&self, prefix: &str) -> String {
        format!("{prefix}-{}", Uuid::new_v4().simple())
    }
}

fn apply_stage_status(
    status: &mut VerificationStatus,
    evidence: &[Evidence],
    stage: VerificationStage,
    kind: EvidenceKind,
    required: bool,
) {
    if !required {
        return;
    }
    let key = match stage {
        VerificationStage::Property => "property",
        VerificationStage::Mutation => "mutation",
        VerificationStage::Fuzz => "fuzz",
        VerificationStage::RuntimeCanary => "runtime_canary",
    };
    let mut latest_by_producer = BTreeMap::<String, &Evidence>::new();
    for record in evidence
        .iter()
        .filter(|record| record.subject == status.plan.subject && record.kind == kind)
    {
        let replace = latest_by_producer
            .get(&record.producer)
            .is_none_or(|current| {
                (current.timestamp_ms, current.id.as_str())
                    < (record.timestamp_ms, record.id.as_str())
            });
        if replace {
            latest_by_producer.insert(record.producer.clone(), record);
        }
    }
    let producer_results = latest_by_producer
        .into_iter()
        .map(|(producer, record)| (producer, record.result))
        .collect::<BTreeMap<_, _>>();
    let result = if producer_results
        .values()
        .any(|result| *result == EvidenceResult::Fail)
    {
        Some(EvidenceResult::Fail)
    } else if producer_results
        .values()
        .any(|result| *result == EvidenceResult::Disagree)
    {
        Some(EvidenceResult::Disagree)
    } else if producer_results
        .values()
        .any(|result| *result == EvidenceResult::Inconclusive)
    {
        Some(EvidenceResult::Inconclusive)
    } else if !producer_results.is_empty() {
        Some(EvidenceResult::Pass)
    } else {
        None
    };
    if let Some(result) = result {
        status.stage_results.insert(key.to_owned(), result);
    }
    if !producer_results.is_empty() {
        status
            .stage_producer_results
            .insert(key.to_owned(), producer_results);
    }
    match result {
        Some(EvidenceResult::Pass) => {}
        Some(EvidenceResult::Fail) => status.blockers.push(format!("{key}-evidence-failed")),
        Some(EvidenceResult::Inconclusive | EvidenceResult::Disagree) => {
            status.blockers.push(format!("{key}-evidence-inconclusive"))
        }
        None => status.blockers.push(format!("{key}-evidence-missing")),
    }
}

fn requirement_trace_status(
    components_resolved: bool,
    implementation: &[TraceReference],
    verification: &[TraceReference],
) -> RequirementTraceStatus {
    let implementation_complete =
        !implementation.is_empty() && implementation.iter().all(|reference| reference.resolved);
    let verification_complete =
        !verification.is_empty() && verification.iter().all(|reference| reference.resolved);
    if components_resolved && implementation_complete && verification_complete {
        RequirementTraceStatus::Complete
    } else if components_resolved
        || implementation.iter().any(|reference| reference.resolved)
        || verification.iter().any(|reference| reference.resolved)
    {
        RequirementTraceStatus::Partial
    } else {
        RequirementTraceStatus::Missing
    }
}

fn resolve_code_reference(
    code_index: &CodeIndex,
    workspace: &Workspace,
    owner: &str,
    reference: &CodeRef,
) -> TraceReference {
    match reference {
        CodeRef::File { path } => match workspace.source_stamp(path) {
            Ok(_) => TraceReference {
                owner: owner.to_owned(),
                kind: TraceReferenceKind::File,
                target: path.clone(),
                resolved: true,
                provider: "filesystem".into(),
                precision: "deterministic".into(),
                node_id: Some(format!("file:{path}")),
                revision: None,
                message: None,
            },
            Err(error) => unresolved_reference(
                owner,
                TraceReferenceKind::File,
                path,
                "filesystem",
                "deterministic",
                error.to_string(),
            ),
        },
        CodeRef::Symbol { path, symbol } => resolve_symbol_reference(
            code_index,
            workspace,
            owner,
            TraceReferenceKind::Symbol,
            path,
            symbol,
        ),
    }
}

fn resolve_verification_reference(
    code_index: &CodeIndex,
    workspace: &Workspace,
    known_checks: &HashSet<String>,
    owner: &str,
    reference: &VerificationRef,
) -> TraceReference {
    match reference {
        VerificationRef::Test { path, symbol } => resolve_symbol_reference(
            code_index,
            workspace,
            owner,
            TraceReferenceKind::Test,
            path,
            symbol,
        ),
        VerificationRef::Check { id } if known_checks.contains(id) => TraceReference {
            owner: owner.to_owned(),
            kind: TraceReferenceKind::Check,
            target: id.clone(),
            resolved: true,
            provider: "harness".into(),
            precision: "deterministic".into(),
            node_id: Some(format!("verification:{id}")),
            revision: None,
            message: None,
        },
        VerificationRef::Check { id } => unresolved_reference(
            owner,
            TraceReferenceKind::Check,
            id,
            "harness",
            "deterministic",
            "verification check is not present in the inferred project profile",
        ),
    }
}

fn resolve_symbol_reference(
    code_index: &CodeIndex,
    workspace: &Workspace,
    owner: &str,
    kind: TraceReferenceKind,
    path: &str,
    symbol: &str,
) -> TraceReference {
    let target = format!("{path}::{symbol}");
    match code_index.resolve_symbol(workspace, path, symbol) {
        Ok(Some(resolution)) => resolved_symbol_reference(owner, kind, target, resolution),
        Ok(None) => unresolved_reference(
            owner,
            kind,
            &target,
            "tree-sitter",
            "syntax",
            "no unique symbol definition matched the declared reference",
        ),
        Err(error) => unresolved_reference(
            owner,
            kind,
            &target,
            "tree-sitter",
            "syntax",
            error.to_string(),
        ),
    }
}

fn resolved_symbol_reference(
    owner: &str,
    kind: TraceReferenceKind,
    target: String,
    resolution: SymbolResolution,
) -> TraceReference {
    TraceReference {
        owner: owner.to_owned(),
        kind,
        target,
        resolved: true,
        provider: "tree-sitter".into(),
        precision: "syntax".into(),
        node_id: Some(format!("symbol:{}", resolution.id)),
        revision: Some(resolution.revision),
        message: Some(format!(
            "resolved `{}` as `{}` ({}) in {}",
            resolution.name, resolution.qualified_name, resolution.kind, resolution.path
        )),
    }
}

fn unresolved_reference(
    owner: &str,
    kind: TraceReferenceKind,
    target: &str,
    provider: &str,
    precision: &str,
    message: impl AsRef<str>,
) -> TraceReference {
    TraceReference {
        owner: owner.to_owned(),
        kind,
        target: target.to_owned(),
        resolved: false,
        provider: provider.to_owned(),
        precision: precision.to_owned(),
        node_id: None,
        revision: None,
        message: Some(bounded_message(message.as_ref())),
    }
}

fn bounded_message(message: &str) -> String {
    message.chars().take(300).collect()
}

fn provider_graph_context(
    workspace: &Workspace,
    query: &str,
    tokens: &[String],
    symbols: &[serde_json::Value],
    limit: usize,
) -> Result<GraphContext> {
    let providers = graph_provider_store::load_latest(workspace)?;
    if providers.is_empty() {
        return Ok(GraphContext::default());
    }
    let limit = limit.clamp(1, MAX_CONTEXT_ITEMS);
    let needle = query.to_ascii_lowercase();
    let symbol_paths = symbols
        .iter()
        .filter_map(|symbol| symbol.get("path").and_then(serde_json::Value::as_str))
        .map(str::to_owned)
        .collect::<HashSet<_>>();
    let fresh_providers = providers
        .iter()
        .filter(|stored| {
            graph_provider_store::freshness(workspace, &stored.import)
                != graph_provider_store::GraphProviderFreshness::Stale
        })
        .collect::<Vec<_>>();
    let mut nodes_by_id = BTreeMap::<String, GraphContextNode>::new();
    let mut ranked = Vec::<(usize, String)>::new();

    for stored in &fresh_providers {
        let provenance = stored.import.provenance();
        for node in &stored.import.nodes {
            let path = node
                .attributes
                .get("path")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            let mut haystack = node.label.to_ascii_lowercase();
            for key in ["name", "qualified_name", "path"] {
                if let Some(value) = node.attributes.get(key).and_then(serde_json::Value::as_str) {
                    haystack.push(' ');
                    haystack.push_str(&value.to_ascii_lowercase());
                }
            }
            let exact = usize::from(!needle.is_empty() && haystack.contains(&needle));
            let token_hits = tokens
                .iter()
                .filter(|token| haystack.contains(token.as_str()))
                .count();
            let path_hit = usize::from(
                path.as_ref()
                    .is_some_and(|path| symbol_paths.contains(path)),
            );
            let score = exact
                .saturating_mul(100)
                .saturating_add(token_hits.saturating_mul(10))
                .saturating_add(path_hit.saturating_mul(40));
            let id = node.id.clone();
            nodes_by_id.entry(id.clone()).or_insert(GraphContextNode {
                id: id.clone(),
                kind: node.kind,
                label: node.label.clone(),
                path,
                provider: provenance.provider.clone(),
                precision: provenance.precision,
                score,
            });
            if score > 0 {
                ranked.push((score, id));
            }
        }
    }

    ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    ranked.dedup_by(|left, right| left.1 == right.1);
    let candidate_count = ranked.len();
    let seeds = ranked
        .into_iter()
        .take(limit)
        .map(|(_, id)| id)
        .collect::<BTreeSet<_>>();
    if seeds.is_empty() {
        return Ok(GraphContext::default());
    }
    let mut selected = seeds.clone();
    let edge_limit = limit.saturating_mul(4).min(256);
    let mut edges = Vec::new();
    let mut edge_matches = 0usize;
    for stored in fresh_providers {
        let provenance = stored.import.provenance();
        for edge in &stored.import.edges {
            if !seeds.contains(&edge.from) && !seeds.contains(&edge.to) {
                continue;
            }
            if !nodes_by_id.contains_key(&edge.from) || !nodes_by_id.contains_key(&edge.to) {
                continue;
            }
            edge_matches = edge_matches.saturating_add(1);
            if selected.len() < limit {
                selected.insert(edge.from.clone());
                if selected.len() < limit {
                    selected.insert(edge.to.clone());
                }
            }
            if selected.contains(&edge.from)
                && selected.contains(&edge.to)
                && edges.len() < edge_limit
            {
                edges.push(GraphContextEdge {
                    from: edge.from.clone(),
                    to: edge.to.clone(),
                    kind: edge.kind,
                    provider: provenance.provider.clone(),
                    precision: provenance.precision,
                });
            }
        }
    }
    let mut nodes = selected
        .into_iter()
        .filter_map(|id| nodes_by_id.remove(&id))
        .collect::<Vec<_>>();
    nodes.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.label.cmp(&right.label))
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(GraphContext {
        truncated: candidate_count > nodes.len() || edge_matches > edges.len(),
        nodes,
        edges,
    })
}

fn context_tokens(query: &str) -> Vec<String> {
    let mut tokens = query
        .split(|character: char| !character.is_alphanumeric())
        .map(str::trim)
        .filter(|token| token.chars().count() >= 2)
        .map(str::to_ascii_lowercase)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    tokens.sort_by_key(|token| std::cmp::Reverse(token.len()));
    tokens
}

fn ranked_context_ids(
    items: impl IntoIterator<Item = (String, String)>,
    query: &str,
    tokens: &[String],
    limit: usize,
) -> Vec<String> {
    let needle = query.to_ascii_lowercase();
    let mut ranked = items
        .into_iter()
        .filter_map(|(id, text)| {
            let haystack = text.to_ascii_lowercase();
            let exact = usize::from(haystack.contains(&needle));
            let token_hits = tokens
                .iter()
                .filter(|token| haystack.contains(token.as_str()))
                .count();
            let id_match = usize::from(
                tokens
                    .iter()
                    .any(|token| id.to_ascii_lowercase().contains(token)),
            );
            let score = exact
                .saturating_mul(100)
                .saturating_add(token_hits.saturating_mul(10))
                .saturating_add(id_match.saturating_mul(5));
            (score > 0).then_some((score, id))
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(&right.1)));
    ranked.into_iter().take(limit).map(|(_, id)| id).collect()
}

#[cfg(test)]
#[path = "../../tests/unit/intelligence/mod.rs"]
mod tests;
