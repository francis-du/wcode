use crate::code_index::{CodeIndex, SymbolResolution};
use crate::design::{self, Priority};
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
use crate::stage_executor::{self, StageExecutorRegistry};
use crate::verification::{
    ReviewSubmission, ReviewerRole, StageSubmission, VerificationJob, VerificationPlan,
    VerificationPlanBinding, VerificationStage, VerificationState, VerificationStatus,
};
use crate::verification_store;
use crate::workspace::Workspace;

#[path = "runtime/claim.rs"]
mod claim_runtime;
pub(crate) mod release_gate;
use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;
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
struct CachedDesignLoad {
    fingerprint: u64,
    last_used: Instant,
    load: Arc<design::DesignLoad>,
}

#[derive(Clone)]
struct CachedTraceabilityStatus {
    fingerprint: u64,
    last_used: Instant,
    status: Arc<TraceabilityStatus>,
}

const MAX_DESIGN_CACHE_WORKSPACES: usize = 4;

#[derive(Clone)]
pub struct SoftwareIntelligenceRuntime {
    state: Arc<Mutex<IntelligenceState>>,
    design_cache: Arc<Mutex<HashMap<PathBuf, CachedDesignLoad>>>,
    traceability_cache: Arc<Mutex<HashMap<PathBuf, CachedTraceabilityStatus>>>,
}

impl Default for SoftwareIntelligenceRuntime {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(IntelligenceState::default())),
            design_cache: Arc::new(Mutex::new(HashMap::new())),
            traceability_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

fn trim_intelligence_cache<K, V, F>(cache: &mut HashMap<K, V>, aggressive: bool, last_used: F)
where
    K: Clone + Eq + std::hash::Hash,
    F: Fn(&V) -> Instant,
{
    if aggressive {
        cache.clear();
        return;
    }
    while cache.len() > MAX_DESIGN_CACHE_WORKSPACES / 2 {
        let Some(oldest) = cache
            .iter()
            .min_by(|(_, left), (_, right)| last_used(left).cmp(&last_used(right)))
            .map(|(key, _)| key.clone())
        else {
            break;
        };
        cache.remove(&oldest);
    }
}

pub use crate::intelligence_types::{
    CodeStatBreakdown, CoverageDimension, DesignContextItem, DesignStatus, DriftFinding, DriftKind,
    DriftStatus, EvidenceStatus, FeatureAcceptanceView, FeatureComponentView,
    FeatureConstraintView, FeatureConvergenceState, FeatureDecisionView,
    FeatureDependencyAlignment, FeatureImplementationView, FeatureRequirementView, GraphContext,
    GraphContextEdge, GraphContextNode, ProjectAcceptanceProofSummary,
    ProjectAdaptiveVerificationView, ProjectChangeView, ProjectCodeStats,
    ProjectConvergenceSummary, ProjectCostEvaluationView, ProjectCostFrontierEntryView,
    ProjectCostSentinelView, ProjectEngineeringJournalView, ProjectEngineeringMilestoneView,
    ProjectFileView, ProjectFocusedVerificationView, ProjectGraphDeltaView, ProjectObservatory,
    ProjectProofSummary, ProjectRevisionView, ProjectStructureView,
    ProjectVerificationImpactReasonView, ProjectVerificationImpactView,
    ProjectVerifiedLearningView, RequirementTrace, RequirementTraceStatus, RiskStatus,
    SemanticStatusView, SoftwareContext, SoftwareContextRequest, TraceReference,
    TraceReferenceKind, TraceabilityStatus,
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
#[path = "runtime_drift.rs"]
mod runtime_drift;
use analysis::*;
use context::*;
pub(crate) use observatory::{build_project_observatory, ObservatoryInput};

const MAX_TRACE_REQUIREMENTS: usize = 200;
const MAX_TRACE_DIAGNOSTICS: usize = 128;

#[path = "runtime/approval.rs"]
mod approval_runtime;
#[path = "runtime/design.rs"]
mod design_runtime;
#[path = "runtime/reconcile.rs"]
mod reconcile_runtime;
#[path = "runtime/semantic.rs"]
mod semantic_runtime;
#[path = "runtime/trace_cache.rs"]
mod trace_cache;
#[path = "runtime/verification_snapshot.rs"]
mod verification_snapshot;
use trace_cache::TraceResolutionSnapshot;

impl SoftwareIntelligenceRuntime {
    pub(crate) fn design_load(&self, workspace: &Workspace) -> Result<Arc<design::DesignLoad>> {
        Ok(self.design_load_with_fingerprint(workspace)?.0)
    }

    pub(crate) fn design_load_with_fingerprint(
        &self,
        workspace: &Workspace,
    ) -> Result<(Arc<design::DesignLoad>, u64)> {
        let fingerprint = design::fingerprint(workspace)?;
        let root = workspace.root().to_path_buf();
        {
            let mut cache = self
                .design_cache
                .lock()
                .map_err(|_| anyhow!("design cache poisoned"))?;
            if let Some(cached) = cache
                .get_mut(&root)
                .filter(|cached| cached.fingerprint == fingerprint)
            {
                cached.last_used = Instant::now();
                return Ok((cached.load.clone(), fingerprint));
            }
        }

        let load = Arc::new(design::load_design(workspace)?);
        let confirmed_fingerprint = design::fingerprint(workspace)?;
        if confirmed_fingerprint != fingerprint {
            bail!("design state changed while loading; retry the request");
        }
        let mut cache = self
            .design_cache
            .lock()
            .map_err(|_| anyhow!("design cache poisoned"))?;
        if let Some(cached) = cache
            .get_mut(&root)
            .filter(|cached| cached.fingerprint == fingerprint)
        {
            cached.last_used = Instant::now();
            return Ok((cached.load.clone(), fingerprint));
        }
        if cache.len() >= MAX_DESIGN_CACHE_WORKSPACES && !cache.contains_key(&root) {
            if let Some(oldest) = cache
                .iter()
                .min_by(|(_, left), (_, right)| left.last_used.cmp(&right.last_used))
                .map(|(root, _)| root.clone())
            {
                cache.remove(&oldest);
            }
        }
        cache.insert(
            root,
            CachedDesignLoad {
                fingerprint,
                last_used: Instant::now(),
                load: load.clone(),
            },
        );
        Ok((load, fingerprint))
    }

    pub(crate) fn invalidate_design_cache(&self, root: &Path) {
        if let Ok(mut cache) = self.design_cache.lock() {
            cache.remove(root);
        }
        if let Ok(mut cache) = self.traceability_cache.lock() {
            cache.remove(root);
        }
    }

    pub(crate) fn trim_design_cache(&self, aggressive: bool) {
        if let Ok(mut cache) = self.design_cache.lock() {
            trim_intelligence_cache(&mut cache, aggressive, |entry| entry.last_used);
        }
        if let Ok(mut cache) = self.traceability_cache.lock() {
            trim_intelligence_cache(&mut cache, aggressive, |entry| entry.last_used);
        }
    }

    #[cfg(test)]
    fn create_plan_for_risk(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        risk_level: RiskLevel,
    ) -> Result<VerificationPlan> {
        let registry = stage_executor::registry(workspace)?;
        let stage_targets = registry
            .detected_languages
            .iter()
            .copied()
            .map(stage_executor::language_target)
            .collect::<Vec<_>>();
        self.create_plan_for_risk_with_targets(
            workspace_id,
            workspace,
            risk_level,
            stage_targets,
            &registry,
        )
    }

    fn create_plan_for_risk_with_targets(
        &self,
        workspace_id: &str,
        workspace: &Workspace,
        risk_level: RiskLevel,
        stage_targets: Vec<String>,
        registry: &StageExecutorRegistry,
    ) -> Result<VerificationPlan> {
        self.ensure_verification_loaded(workspace_id, workspace)?;
        let revision = self.current_revision(workspace)?;
        let subject = format!("change:{}", revision.code);
        let plan_id = self.next_id("VP");
        let job_ids = std::iter::repeat_with(|| self.next_id("VJ"));
        let profile = VerificationProfile::for_risk(risk_level);
        let automation_gaps = verification_automation_gaps(&profile, registry, &stage_targets);
        let (plan, snapshot) = {
            let mut state = self
                .state
                .lock()
                .map_err(|_| anyhow!("software intelligence state poisoned"))?;
            let plan = state.verification.create_plan(
                plan_id,
                workspace_id.to_owned(),
                subject,
                VerificationPlanBinding {
                    revision,
                    stage_targets,
                    automation_gaps,
                },
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

fn evidence_matches_plan_revision(record: &Evidence, plan: &VerificationPlan) -> bool {
    record.subject == plan.subject
        && plan
            .revision
            .as_ref()
            .is_none_or(|revision| record.revision == *revision)
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
    let records = evidence
        .iter()
        .filter(|record| {
            evidence_matches_plan_revision(record, &status.plan) && record.kind == kind
        })
        .collect::<Vec<_>>();
    let producer_results = latest_results_by_producer(records.iter().copied());
    if !producer_results.is_empty() {
        status
            .stage_producer_results
            .insert(key.to_owned(), producer_results.clone());
    }

    if status.plan.stage_targets.is_empty() {
        let result = aggregate_results(producer_results.values().copied());
        if let Some(result) = result {
            status.stage_results.insert(key.to_owned(), result);
        }
        apply_stage_blocker(status, key, result);
        return;
    }

    let mut target_results = BTreeMap::new();
    let mut missing_targets = Vec::new();
    for target in &status.plan.stage_targets {
        let results = latest_results_by_producer(
            records
                .iter()
                .copied()
                .filter(|record| record.targets.iter().any(|covered| covered == target)),
        );
        match aggregate_results(results.values().copied()) {
            Some(result) => {
                target_results.insert(target.clone(), result);
                match result {
                    EvidenceResult::Pass => {}
                    EvidenceResult::Fail => status
                        .blockers
                        .push(format!("{key}-target-failed:{target}")),
                    EvidenceResult::Inconclusive | EvidenceResult::Disagree => status
                        .blockers
                        .push(format!("{key}-target-inconclusive:{target}")),
                }
            }
            None => {
                missing_targets.push(target.clone());
                status
                    .blockers
                    .push(format!("{key}-target-missing:{target}"));
            }
        }
    }
    if !target_results.is_empty() {
        status
            .stage_target_results
            .insert(key.to_owned(), target_results.clone());
    }
    let target_result = aggregate_results(target_results.values().copied());
    let result = if missing_targets.is_empty() || target_result == Some(EvidenceResult::Fail) {
        target_result
    } else {
        None
    };
    if let Some(result) = result {
        status.stage_results.insert(key.to_owned(), result);
    }
    if !missing_targets.is_empty() {
        status.blockers.push(format!("{key}-evidence-missing"));
    }
    if matches!(target_result, Some(EvidenceResult::Fail)) {
        status.blockers.push(format!("{key}-evidence-failed"));
    } else if matches!(
        target_result,
        Some(EvidenceResult::Inconclusive | EvidenceResult::Disagree)
    ) {
        status.blockers.push(format!("{key}-evidence-inconclusive"));
    }
}

fn latest_results_by_producer<'a>(
    records: impl Iterator<Item = &'a Evidence>,
) -> BTreeMap<String, EvidenceResult> {
    let severity = |result| match result {
        EvidenceResult::Pass => 0,
        EvidenceResult::Inconclusive => 1,
        EvidenceResult::Disagree => 2,
        EvidenceResult::Fail => 3,
    };
    let mut latest = BTreeMap::<String, &Evidence>::new();
    for record in records {
        let replace = latest.get(&record.producer).is_none_or(|current| {
            record.timestamp_ms > current.timestamp_ms
                || (record.timestamp_ms == current.timestamp_ms
                    && (severity(record.result) > severity(current.result)
                        || (record.result == current.result && record.id > current.id)))
        });
        if replace {
            latest.insert(record.producer.clone(), record);
        }
    }
    latest
        .into_iter()
        .map(|(producer, record)| (producer, record.result))
        .collect()
}

fn aggregate_results(results: impl Iterator<Item = EvidenceResult>) -> Option<EvidenceResult> {
    let results = results.collect::<Vec<_>>();
    if results.contains(&EvidenceResult::Fail) {
        Some(EvidenceResult::Fail)
    } else if results.contains(&EvidenceResult::Disagree) {
        Some(EvidenceResult::Disagree)
    } else if results.contains(&EvidenceResult::Inconclusive) {
        Some(EvidenceResult::Inconclusive)
    } else if !results.is_empty() {
        Some(EvidenceResult::Pass)
    } else {
        None
    }
}

fn apply_stage_blocker(status: &mut VerificationStatus, key: &str, result: Option<EvidenceResult>) {
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
        .collect::<HashSet<_>>();
    let fresh_providers = providers
        .iter()
        .filter(|stored| {
            graph_provider_store::freshness(workspace, &stored.import)
                != graph_provider_store::GraphProviderFreshness::Stale
        })
        .collect::<Vec<_>>();
    let mut nodes_by_id = HashMap::<
        &str,
        (
            &crate::graph::GraphImportNode,
            &crate::graph::GraphProviderImport,
            usize,
        ),
    >::new();
    let mut ranked = Vec::<(usize, &str)>::new();

    for stored in &fresh_providers {
        for node in &stored.import.nodes {
            let path = node
                .attributes
                .get("path")
                .and_then(serde_json::Value::as_str);
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
            let path_hit = usize::from(path.is_some_and(|path| symbol_paths.contains(path)));
            let score = exact
                .saturating_mul(100)
                .saturating_add(token_hits.saturating_mul(10))
                .saturating_add(path_hit.saturating_mul(40));
            nodes_by_id
                .entry(node.id.as_str())
                .and_modify(|entry| {
                    if score > entry.2 {
                        *entry = (node, &stored.import, score);
                    }
                })
                .or_insert((node, &stored.import, score));
            if score > 0 {
                ranked.push((score, node.id.as_str()));
            }
        }
    }

    ranked.sort_by(|left, right| right.0.cmp(&left.0).then_with(|| left.1.cmp(right.1)));
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
            if !seeds.contains(edge.from.as_str()) && !seeds.contains(edge.to.as_str()) {
                continue;
            }
            if !nodes_by_id.contains_key(edge.from.as_str())
                || !nodes_by_id.contains_key(edge.to.as_str())
            {
                continue;
            }
            edge_matches = edge_matches.saturating_add(1);
            if selected.len() < limit {
                selected.insert(edge.from.as_str());
                if selected.len() < limit {
                    selected.insert(edge.to.as_str());
                }
            }
            if selected.contains(edge.from.as_str())
                && selected.contains(edge.to.as_str())
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
        .filter_map(|id| {
            let (node, import, score) = nodes_by_id.get(id)?;
            let provenance = import.provenance();
            Some(GraphContextNode {
                id: id.to_owned(),
                kind: node.kind,
                label: node.label.clone(),
                path: node
                    .attributes
                    .get("path")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned),
                provider: provenance.provider,
                precision: provenance.precision,
                score: *score,
            })
        })
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

pub(crate) fn code_query_literals(query: &str) -> Vec<String> {
    let whole_query = query.trim();
    let mut literals = Vec::new();
    let mut seen = HashSet::new();
    let mut current = String::new();
    let flush = |current: &mut String, literals: &mut Vec<String>, seen: &mut HashSet<String>| {
        let raw = current.trim_matches(['_', ':', '.']);
        if raw.chars().count() >= 2 {
            let has_separator = raw.contains('_') || raw.contains("::") || raw.contains('.');
            let has_camel_transition = raw
                .as_bytes()
                .windows(2)
                .any(|pair| pair[0].is_ascii_lowercase() && pair[1].is_ascii_uppercase());
            let exact_query = raw == whole_query;
            if has_separator || has_camel_transition || exact_query {
                let normalized = raw.to_ascii_lowercase();
                if seen.insert(normalized.clone()) {
                    literals.push(normalized);
                }
            }
        }
        current.clear();
    };
    for character in query.chars() {
        if character.is_ascii_alphanumeric() || matches!(character, '_' | ':' | '.') {
            current.push(character);
        } else {
            flush(&mut current, &mut literals, &mut seen);
        }
    }
    flush(&mut current, &mut literals, &mut seen);
    literals.truncate(8);
    literals
}

pub(crate) fn code_query_tokens(query: &str) -> Vec<String> {
    let literals = code_query_literals(query);
    let mut tokens = Vec::new();
    let mut seen = HashSet::new();
    for literal in literals {
        if seen.insert(literal.clone()) {
            tokens.push(literal.clone());
        }
        for part in literal
            .split(['_', ':', '.'])
            .filter(|part| part.chars().count() >= 2)
        {
            let part = part.to_owned();
            if seen.insert(part.clone()) {
                tokens.push(part);
            }
        }
    }
    tokens.truncate(16);
    tokens
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
            let id_lower = id.to_ascii_lowercase();
            let id_match = usize::from(tokens.iter().any(|token| id_lower.contains(token)));
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
