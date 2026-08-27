use super::*;
use crate::graph::{EdgeKind, GraphPrecision, NodeKind, SoftwareGraphSnapshot};
use crate::graph_store::{GraphDiffResult, GraphHistoryEntry};
use crate::harness::{ChangeReviewReport, ChangedFileReview};
use crate::intelligence_types::{
    CodeStatBreakdown, FeatureAcceptanceView, FeatureComponentView, FeatureConstraintView,
    FeatureConvergenceState, FeatureDecisionView, FeatureDependencyAlignment,
    FeatureImplementationView, FeatureRequirementView, ProjectChangeView, ProjectCodeStats,
    ProjectConvergenceSummary, ProjectGraphDeltaView, ProjectGraphPrecisionSummary,
    ProjectObservatory, ProjectProofSummary, ProjectRevisionView,
};
use crate::reconcile::ImpactAnalysis;
use crate::scopes;
use crate::semantic_provider::language_for_path;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const MAX_PROJECT_CHANGES: usize = 500;
const MAX_REQUIREMENT_DRIFT: usize = 32;

pub(crate) struct ObservatoryInput<'a> {
    pub workspace: String,
    pub root: String,
    pub design: design::DesignLoad,
    pub traceability: TraceabilityStatus,
    pub graph: &'a SoftwareGraphSnapshot,
    pub review: Option<&'a ChangeReviewReport>,
    pub impact: Option<ImpactAnalysis>,
    pub risk: Option<RiskStatus>,
    pub history: &'a [GraphHistoryEntry],
    pub graph_diff: Option<&'a GraphDiffResult>,
    pub language_quality: crate::quality_provider::LanguageQualityRegistry,
    pub proof: ProjectProofSummary,
    pub reconciliation_plans: usize,
    pub latest_reconciliation_plan: Option<String>,
}

pub(crate) fn build_project_observatory(input: ObservatoryInput<'_>) -> ProjectObservatory {
    let design_valid = input.design.initialized && input.design.error_count() == 0;
    let state = input.design.state;
    let changed_paths = input
        .review
        .map(|review| {
            review
                .files
                .iter()
                .map(|file| file.path.clone())
                .collect::<HashSet<_>>()
        })
        .unwrap_or_default();
    let path_lines = graph_file_lines(input.graph);
    let code = code_stats(input.graph, input.review);
    let graph_precision = graph_precision_summary(input.graph);
    let component_paths = component_paths(&state);
    let actual_dependencies =
        actual_component_dependencies(&state, &input.traceability, input.graph, &component_paths);
    let desired_dependencies = desired_component_dependencies(&state);
    let changes = input
        .review
        .map(|review| build_changes(&state, review))
        .unwrap_or_default();
    let drift = input.risk.as_ref().map(|risk| &risk.drift);
    let requirement_context = RequirementBuildContext {
        state: &state,
        changed_paths: &changed_paths,
        review: input.review,
        drift,
        impact: input.impact.as_ref(),
        path_lines: &path_lines,
        desired_dependencies: &desired_dependencies,
        actual_dependencies: &actual_dependencies,
    };
    let requirements = input
        .traceability
        .requirements
        .iter()
        .filter_map(|trace| {
            state
                .requirements
                .get(&trace.id)
                .map(|requirement| build_requirement(requirement, trace, &requirement_context))
        })
        .collect::<Vec<_>>();
    let architecture_dependencies =
        project_dependency_alignment(&state, &desired_dependencies, &actual_dependencies);
    let architecture = super::observatory_architecture::build_project_architecture(
        &state,
        &path_lines,
        &changed_paths,
        architecture_dependencies,
    );
    let history = input
        .history
        .iter()
        .take(32)
        .map(|entry| ProjectRevisionView {
            id: entry.id.clone(),
            captured_at_ms: entry.captured_at_ms,
            nodes: entry.nodes,
            edges: entry.edges,
            files_indexed: entry.files_indexed,
            truncated: entry.truncated,
        })
        .collect();
    let mut convergence = ProjectConvergenceSummary {
        stable_requirements: 0,
        changing_requirements: 0,
        needs_convergence_requirements: 0,
        incomplete_requirements: 0,
        reconciliation_plans: input.reconciliation_plans,
        latest_reconciliation_plan: input.latest_reconciliation_plan,
    };
    for requirement in &requirements {
        match requirement.convergence {
            FeatureConvergenceState::Stable => convergence.stable_requirements += 1,
            FeatureConvergenceState::Changing => convergence.changing_requirements += 1,
            FeatureConvergenceState::NeedsConvergence => {
                convergence.needs_convergence_requirements += 1
            }
            FeatureConvergenceState::Incomplete => convergence.incomplete_requirements += 1,
        }
    }
    let latest_delta = input.graph_diff.map(|diff| ProjectGraphDeltaView {
        from_snapshot_id: diff.from_snapshot_id.clone(),
        to_snapshot_id: diff.to_snapshot_id.clone(),
        from_captured_at_ms: diff.from_captured_at_ms,
        to_captured_at_ms: diff.to_captured_at_ms,
        added_nodes: diff.added_node_count,
        removed_nodes: diff.removed_node_count,
        changed_nodes: diff.changed_node_count,
        added_edges: diff.added_edge_count,
        removed_edges: diff.removed_edge_count,
        changed_edges: diff.changed_edge_count,
        changed_paths: graph_delta_paths(diff, input.graph),
        truncated: diff.truncated,
    });

    ProjectObservatory {
        workspace: input.workspace,
        root: input.root,
        project: state.project.as_ref().map(|project| project.name.clone()),
        product: state.product.as_ref().map(|product| product.name.clone()),
        product_vision: state
            .product
            .as_ref()
            .map(|product| product.vision.trim())
            .filter(|vision| !vision.is_empty())
            .map(str::to_owned),
        product_principles: state
            .product
            .as_ref()
            .map(|product| product.principles.clone())
            .unwrap_or_default(),
        design_valid,
        coverage: input.traceability,
        code,
        graph_precision,
        language_quality: input.language_quality,
        proof: input.proof,
        convergence,
        architecture,
        requirements,
        changes,
        history,
        latest_delta,
        impact: input.impact,
        risk: input.risk,
    }
}

fn graph_delta_paths(diff: &GraphDiffResult, current: &SoftwareGraphSnapshot) -> Vec<String> {
    const MAX_DELTA_PATHS: usize = 48;
    let mut paths = BTreeSet::<String>::new();
    let mut fallback = HashMap::<String, String>::new();
    for node in diff
        .removed_nodes
        .iter()
        .chain(diff.changed_nodes.iter().map(|change| &change.before))
    {
        if let Some(path) = graph_node_path(node) {
            fallback.insert(node.id.clone(), path.to_owned());
            paths.insert(path.to_owned());
        }
    }
    for node in diff
        .added_nodes
        .iter()
        .chain(diff.changed_nodes.iter().map(|change| &change.after))
    {
        if let Some(path) = graph_node_path(node) {
            paths.insert(path.to_owned());
        }
    }
    for edge in diff
        .added_edges
        .iter()
        .chain(diff.removed_edges.iter())
        .chain(
            diff.changed_edges
                .iter()
                .flat_map(|change| [&change.before, &change.after]),
        )
    {
        for id in [&edge.from, &edge.to] {
            if let Some(path) = current
                .graph
                .nodes
                .get(id)
                .and_then(graph_node_path)
                .or_else(|| fallback.get(id).map(String::as_str))
            {
                paths.insert(path.to_owned());
            }
        }
    }
    paths.into_iter().take(MAX_DELTA_PATHS).collect()
}

fn graph_node_path(node: &crate::graph::GraphNode) -> Option<&str> {
    node.attributes
        .get("path")
        .and_then(serde_json::Value::as_str)
}

fn graph_file_lines(graph: &SoftwareGraphSnapshot) -> BTreeMap<String, usize> {
    graph
        .graph
        .nodes
        .values()
        .filter(|node| node.kind == NodeKind::File)
        .filter_map(|node| {
            let path = node
                .attributes
                .get("path")
                .and_then(serde_json::Value::as_str)?;
            let lines = node
                .attributes
                .get("line_count")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or_default();
            Some((path.to_owned(), lines))
        })
        .collect()
}

fn code_stats(
    graph: &SoftwareGraphSnapshot,
    review: Option<&ChangeReviewReport>,
) -> ProjectCodeStats {
    let mut source_files = 0usize;
    let mut source_lines = 0usize;
    let mut source_bytes = 0u64;
    let mut symbols = 0usize;
    let mut languages = BTreeMap::<String, (usize, usize)>::new();
    let mut product_scopes = BTreeMap::<String, (usize, usize)>::new();

    for node in graph.graph.nodes.values() {
        let path = node
            .attributes
            .get("path")
            .and_then(serde_json::Value::as_str);
        if node.kind == NodeKind::File {
            let Some(path) = path else { continue };
            source_files = source_files.saturating_add(1);
            let lines = node
                .attributes
                .get("line_count")
                .and_then(serde_json::Value::as_u64)
                .and_then(|value| usize::try_from(value).ok())
                .unwrap_or_default();
            let bytes = node
                .attributes
                .get("source_bytes")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default();
            source_lines = source_lines.saturating_add(lines);
            source_bytes = source_bytes.saturating_add(bytes);
            let language = node
                .attributes
                .get("language")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown")
                .to_owned();
            let entry = languages.entry(language).or_default();
            entry.0 = entry.0.saturating_add(1);
            entry.1 = entry.1.saturating_add(lines);
            if let Some(scope) = scopes::source_scope(path) {
                let entry = product_scopes.entry(scope.as_str().to_owned()).or_default();
                entry.0 = entry.0.saturating_add(1);
                entry.1 = entry.1.saturating_add(lines);
            }
        } else if path.is_some() {
            symbols = symbols.saturating_add(1);
        }
    }

    let mut language_breakdown = breakdown(languages);
    language_breakdown.sort_by(|left, right| {
        right
            .lines
            .cmp(&left.lines)
            .then_with(|| right.files.cmp(&left.files))
            .then_with(|| left.name.cmp(&right.name))
    });
    let mut scope_breakdown = breakdown(product_scopes);
    scope_breakdown.sort_by(|left, right| {
        right
            .lines
            .cmp(&left.lines)
            .then_with(|| left.name.cmp(&right.name))
    });

    let changed_source_files = review
        .map(|review| {
            review
                .files
                .iter()
                .filter(|file| language_for_path(&file.path).is_some())
                .count()
        })
        .unwrap_or_default();
    ProjectCodeStats {
        source_files,
        source_lines,
        source_bytes,
        symbols,
        call_edges: graph
            .graph
            .edges
            .iter()
            .filter(|edge| matches!(edge.kind, EdgeKind::Calls | EdgeKind::RuntimeCalls))
            .count(),
        languages: language_breakdown,
        product_scopes: scope_breakdown,
        changed_files: review
            .map(|review| review.files_changed)
            .unwrap_or_default(),
        changed_source_files,
        additions: review.map(|review| review.additions).unwrap_or_default(),
        deletions: review.map(|review| review.deletions).unwrap_or_default(),
        untracked_files: review
            .map(|review| review.untracked_files)
            .unwrap_or_default(),
        graph_truncated: graph.truncated || graph.scan_truncated,
    }
}

fn breakdown(values: BTreeMap<String, (usize, usize)>) -> Vec<CodeStatBreakdown> {
    values
        .into_iter()
        .map(|(name, (files, lines))| CodeStatBreakdown { name, files, lines })
        .collect()
}

fn component_paths(state: &design::DesignState) -> BTreeMap<String, BTreeSet<String>> {
    state
        .components
        .values()
        .map(|component| {
            (
                component.id.clone(),
                component
                    .implementation
                    .iter()
                    .map(|reference| reference.path().to_owned())
                    .collect(),
            )
        })
        .collect()
}

fn desired_component_dependencies(state: &design::DesignState) -> BTreeSet<(String, String)> {
    let mut pairs = BTreeSet::new();
    for component in state.components.values() {
        for dependency in &component.depends_on {
            if state.components.contains_key(dependency) && dependency != &component.id {
                pairs.insert((component.id.clone(), dependency.clone()));
            }
        }
    }
    pairs
}

fn actual_component_dependencies(
    state: &design::DesignState,
    traceability: &TraceabilityStatus,
    graph: &SoftwareGraphSnapshot,
    component_paths: &BTreeMap<String, BTreeSet<String>>,
) -> BTreeMap<(String, String), String> {
    let mut node_owners = HashMap::<String, BTreeSet<String>>::new();
    for component in state.components.values() {
        for reference in &component.implementation {
            for node in graph.graph.nodes.values() {
                let Some(path) = graph_node_path(node) else {
                    continue;
                };
                if path != reference.path() {
                    continue;
                }
                let matches_reference = match reference {
                    design::CodeRef::File { .. } => node.kind == NodeKind::File,
                    design::CodeRef::Symbol { symbol, .. } => {
                        let qualified_name = node
                            .attributes
                            .get("qualified_name")
                            .and_then(serde_json::Value::as_str);
                        let name = node
                            .attributes
                            .get("name")
                            .and_then(serde_json::Value::as_str);
                        qualified_name == Some(symbol.as_str()) || name == Some(symbol.as_str())
                    }
                };
                if matches_reference {
                    node_owners
                        .entry(node.id.clone())
                        .or_default()
                        .insert(component.id.clone());
                }
            }
        }
    }
    for requirement in &traceability.requirements {
        for reference in &requirement.implementation {
            if state.components.contains_key(&reference.owner) {
                if let Some(node_id) = &reference.node_id {
                    node_owners
                        .entry(node_id.clone())
                        .or_default()
                        .insert(reference.owner.clone());
                }
            }
        }
    }

    let mut owners_by_path = HashMap::<String, Vec<String>>::new();
    for (component, paths) in component_paths {
        for path in paths {
            owners_by_path
                .entry(path.clone())
                .or_default()
                .push(component.clone());
        }
    }
    for node in graph.graph.nodes.values() {
        if node_owners.contains_key(&node.id) {
            continue;
        }
        let Some(path) = node
            .attributes
            .get("path")
            .and_then(serde_json::Value::as_str)
        else {
            continue;
        };
        let Some(owners) = owners_by_path.get(path) else {
            continue;
        };
        if owners.len() == 1 {
            node_owners
                .entry(node.id.clone())
                .or_default()
                .insert(owners[0].clone());
        }
    }

    let mut actual = BTreeMap::<(String, String), String>::new();
    for edge in &graph.graph.edges {
        if !matches!(
            edge.kind,
            EdgeKind::Calls
                | EdgeKind::Imports
                | EdgeKind::DependsOn
                | EdgeKind::References
                | EdgeKind::RuntimeCalls
        ) {
            continue;
        }
        let Some(from) = node_owners.get(&edge.from) else {
            continue;
        };
        let Some(to) = node_owners.get(&edge.to) else {
            continue;
        };
        let precision = precision_name(edge.provenance.precision);
        for from_component in from {
            for to_component in to {
                if from_component == to_component {
                    continue;
                }
                let key = (from_component.clone(), to_component.clone());
                match actual.get(&key) {
                    Some(existing) if precision_rank(existing) >= precision_rank(&precision) => {}
                    _ => {
                        actual.insert(key, precision.clone());
                    }
                }
            }
        }
    }
    actual
}

fn graph_precision_summary(graph: &SoftwareGraphSnapshot) -> ProjectGraphPrecisionSummary {
    let mut providers = BTreeSet::new();
    let mut declared_edges = 0usize;
    let mut syntax_edges = 0usize;
    let mut semantic_edges = 0usize;
    let mut deterministic_edges = 0usize;
    let mut runtime_edges = 0usize;
    let mut heuristic_edges = 0usize;
    let mut primary = "declared";
    let mut best_rank = precision_rank(primary);

    for edge in &graph.graph.edges {
        let precision = precision_name(edge.provenance.precision);
        providers.insert(edge.provenance.provider.clone());
        match edge.provenance.precision {
            GraphPrecision::Declared => declared_edges += 1,
            GraphPrecision::Syntax => syntax_edges += 1,
            GraphPrecision::Semantic => semantic_edges += 1,
            GraphPrecision::Deterministic => deterministic_edges += 1,
            GraphPrecision::Runtime => runtime_edges += 1,
            GraphPrecision::Heuristic => heuristic_edges += 1,
            GraphPrecision::Mixed => {}
        }
        let rank = precision_rank(&precision);
        if rank > best_rank {
            best_rank = rank;
            primary = match edge.provenance.precision {
                GraphPrecision::Runtime => "runtime",
                GraphPrecision::Semantic => "semantic",
                GraphPrecision::Deterministic => "deterministic",
                GraphPrecision::Syntax => "syntax",
                GraphPrecision::Declared => "declared",
                GraphPrecision::Heuristic => "heuristic",
                GraphPrecision::Mixed => "mixed",
            };
        }
    }

    ProjectGraphPrecisionSummary {
        primary: primary.to_owned(),
        providers: providers.into_iter().collect(),
        declared_edges,
        syntax_edges,
        semantic_edges,
        deterministic_edges,
        runtime_edges,
        heuristic_edges,
    }
}

fn precision_name(precision: GraphPrecision) -> String {
    format!("{precision:?}").to_ascii_lowercase()
}

fn precision_rank(value: &str) -> u8 {
    match value {
        "runtime" => 6,
        "semantic" => 5,
        "deterministic" => 4,
        "syntax" => 3,
        "declared" => 2,
        "heuristic" => 1,
        _ => 0,
    }
}

struct RequirementBuildContext<'a> {
    state: &'a design::DesignState,
    changed_paths: &'a HashSet<String>,
    review: Option<&'a ChangeReviewReport>,
    drift: Option<&'a DriftStatus>,
    impact: Option<&'a ImpactAnalysis>,
    path_lines: &'a BTreeMap<String, usize>,
    desired_dependencies: &'a BTreeSet<(String, String)>,
    actual_dependencies: &'a BTreeMap<(String, String), String>,
}

fn build_requirement(
    requirement: &design::Requirement,
    trace: &RequirementTrace,
    context: &RequirementBuildContext<'_>,
) -> FeatureRequirementView {
    let state = context.state;
    let changed_paths = context.changed_paths;
    let components = requirement
        .implemented_by
        .iter()
        .filter_map(|id| state.components.get(id))
        .map(|component| {
            let implementation = trace
                .implementation
                .iter()
                .filter(|reference| reference.owner == component.id)
                .map(|reference| implementation_view(reference, changed_paths))
                .collect::<Vec<_>>();
            let mut component_changed_paths = component
                .implementation
                .iter()
                .map(|reference| reference.path())
                .filter(|path| changed_paths.contains(*path))
                .map(str::to_owned)
                .collect::<Vec<_>>();
            component_changed_paths.sort();
            component_changed_paths.dedup();
            let implementation_lines = component
                .implementation
                .iter()
                .map(|reference| reference.path())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(|path| context.path_lines.get(path).copied().unwrap_or_default())
                .sum();
            FeatureComponentView {
                id: component.id.clone(),
                name: component.name.clone(),
                responsibilities: component.responsibilities.clone(),
                depends_on: component.depends_on.clone(),
                implementation,
                implementation_lines,
                changed: !component_changed_paths.is_empty(),
                changed_paths: component_changed_paths,
            }
        })
        .collect::<Vec<_>>();
    let acceptance = requirement
        .acceptance
        .iter()
        .filter_map(|id| state.acceptance.get(id))
        .map(|criterion| FeatureAcceptanceView {
            id: criterion.id.clone(),
            title: criterion.title.clone(),
            statement: criterion.statement.clone(),
            verification: trace
                .verification
                .iter()
                .filter(|reference| reference.owner == criterion.id)
                .map(|reference| implementation_view(reference, changed_paths))
                .collect(),
        })
        .collect::<Vec<_>>();
    let constraints = requirement
        .constraints
        .iter()
        .filter_map(|id| state.constraints.get(id))
        .map(|constraint| FeatureConstraintView {
            id: constraint.id.clone(),
            title: constraint.title.clone(),
            statement: constraint.statement.clone(),
        })
        .collect::<Vec<_>>();
    let decisions = state
        .decisions
        .values()
        .filter(|decision| {
            decision.affects.iter().any(|target| {
                target == &requirement.id || requirement.implemented_by.contains(target)
            })
        })
        .map(|decision| FeatureDecisionView {
            id: decision.id.clone(),
            title: decision.title.clone(),
            status: format!("{:?}", decision.status).to_ascii_lowercase(),
            decision: decision.decision.clone(),
            rationale: decision.rationale.clone(),
        })
        .collect::<Vec<_>>();
    let dependency_alignment = dependency_alignment(
        requirement,
        state,
        context.desired_dependencies,
        context.actual_dependencies,
    );
    let drift_messages = context
        .drift
        .map(|status| {
            status
                .findings
                .iter()
                .filter(|finding| finding.affected_requirements.contains(&requirement.id))
                .take(MAX_REQUIREMENT_DRIFT)
                .map(|finding| finding.message.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let requirement_paths = components
        .iter()
        .flat_map(|component| {
            component
                .implementation
                .iter()
                .map(|item| item.path.clone())
        })
        .collect::<BTreeSet<_>>();
    let (change_additions, change_deletions) = context
        .review
        .map(|review| {
            review
                .files
                .iter()
                .filter(|file| requirement_paths.contains(&file.path))
                .fold((0u64, 0u64), |(additions, deletions), file| {
                    (
                        additions.saturating_add(file.additions.unwrap_or_default()),
                        deletions.saturating_add(file.deletions.unwrap_or_default()),
                    )
                })
        })
        .unwrap_or_default();
    let implementation_files = components
        .iter()
        .flat_map(|component| {
            component
                .implementation
                .iter()
                .map(|item| item.path.as_str())
        })
        .filter(|path| !path.is_empty())
        .collect::<BTreeSet<_>>()
        .len();
    let implementation_symbols = components
        .iter()
        .flat_map(|component| component.implementation.iter())
        .filter(|item| item.symbol.is_some())
        .count();
    let implementation_lines = requirement_paths
        .iter()
        .map(|path| context.path_lines.get(path).copied().unwrap_or_default())
        .sum();
    let changed = components.iter().any(|component| component.changed)
        || context
            .impact
            .is_some_and(|impact| impact.impacted_requirements.contains(&requirement.id))
        || context
            .drift
            .is_some_and(|status| status.design_changed && status.implementation_changed)
            && requirement_paths
                .iter()
                .any(|path| changed_paths.contains(path));
    let aligned = trace.status == RequirementTraceStatus::Complete
        && drift_messages.is_empty()
        && dependency_alignment
            .iter()
            .all(|dependency| !dependency.blocking);
    let mut convergence_blockers = Vec::new();
    if trace.status != RequirementTraceStatus::Complete {
        convergence_blockers.push("requirement traceability is incomplete".to_owned());
    }
    convergence_blockers.extend(
        dependency_alignment
            .iter()
            .filter(|dependency| dependency.blocking)
            .take(16)
            .map(|dependency| {
                format!(
                    "component dependency {} -> {} is {}",
                    dependency.from, dependency.to, dependency.status
                )
            }),
    );
    convergence_blockers.extend(drift_messages.iter().take(16).cloned());
    convergence_blockers.truncate(32);
    let convergence = if trace.status != RequirementTraceStatus::Complete {
        FeatureConvergenceState::Incomplete
    } else if !aligned {
        FeatureConvergenceState::NeedsConvergence
    } else if changed {
        FeatureConvergenceState::Changing
    } else {
        FeatureConvergenceState::Stable
    };

    FeatureRequirementView {
        id: requirement.id.clone(),
        title: requirement.title.clone(),
        intent: requirement.intent.clone(),
        priority: requirement.priority,
        status: trace.status,
        aligned,
        convergence,
        convergence_blockers,
        changed,
        change_additions,
        change_deletions,
        implementation_files,
        implementation_symbols,
        implementation_lines,
        components,
        acceptance,
        constraints,
        decisions,
        dependency_alignment,
        drift: drift_messages,
    }
}

fn dependency_alignment(
    requirement: &design::Requirement,
    state: &design::DesignState,
    desired_dependencies: &BTreeSet<(String, String)>,
    actual_dependencies: &BTreeMap<(String, String), String>,
) -> Vec<FeatureDependencyAlignment> {
    let requirement_components = requirement
        .implemented_by
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    let mut pairs = BTreeSet::<(String, String)>::new();
    pairs.extend(
        desired_dependencies
            .iter()
            .filter(|(from, _)| requirement_components.contains(from))
            .cloned(),
    );
    pairs.extend(
        actual_dependencies
            .keys()
            .filter(|(from, _)| requirement_components.contains(from))
            .cloned(),
    );
    dependency_alignment_for_pairs(state, pairs, desired_dependencies, actual_dependencies)
}

fn project_dependency_alignment(
    state: &design::DesignState,
    desired_dependencies: &BTreeSet<(String, String)>,
    actual_dependencies: &BTreeMap<(String, String), String>,
) -> Vec<FeatureDependencyAlignment> {
    let mut pairs = desired_dependencies.clone();
    pairs.extend(actual_dependencies.keys().cloned());
    dependency_alignment_for_pairs(state, pairs, desired_dependencies, actual_dependencies)
}

fn dependency_alignment_for_pairs(
    state: &design::DesignState,
    pairs: BTreeSet<(String, String)>,
    desired_dependencies: &BTreeSet<(String, String)>,
    actual_dependencies: &BTreeMap<(String, String), String>,
) -> Vec<FeatureDependencyAlignment> {
    pairs
        .into_iter()
        .map(|(from, to)| {
            let desired = desired_dependencies.contains(&(from.clone(), to.clone()));
            let actual_precision = actual_dependencies.get(&(from.clone(), to.clone()));
            let actual = actual_precision.is_some();
            let precision = actual_precision
                .cloned()
                .unwrap_or_else(|| "not_observed".to_owned());
            let strong_actual_evidence =
                matches!(precision.as_str(), "runtime" | "semantic" | "deterministic");
            let (status, blocking) = match (desired, actual) {
                (true, true) => ("aligned", false),
                // Absence from a bounded syntax/semantic graph is not proof that a declared
                // dependency does not exist. Keep it observable, but advisory.
                (true, false) => ("unverified_actual", false),
                // A positive undeclared dependency is actionable only when the provider is
                // stronger than syntax/heuristic precision. Syntax observations remain advisory.
                (false, true) if strong_actual_evidence => ("undeclared_actual", true),
                (false, true) => ("observed_actual", false),
                (false, false) => ("unknown", false),
            };
            FeatureDependencyAlignment {
                from_name: component_name(state, &from),
                to_name: component_name(state, &to),
                from,
                to,
                desired,
                actual,
                status: status.to_owned(),
                precision,
                blocking,
            }
        })
        .collect()
}

fn component_name(state: &design::DesignState, id: &str) -> String {
    state
        .components
        .get(id)
        .map(|component| component.name.clone())
        .unwrap_or_else(|| id.to_owned())
}

fn implementation_view(
    reference: &TraceReference,
    changed_paths: &HashSet<String>,
) -> FeatureImplementationView {
    let (path, symbol) = split_target(&reference.target);
    FeatureImplementationView {
        target: reference.target.clone(),
        changed: changed_paths.contains(&path),
        path,
        symbol,
        resolved: reference.resolved,
        provider: reference.provider.clone(),
        precision: reference.precision.clone(),
    }
}

fn split_target(target: &str) -> (String, Option<String>) {
    if let Some((path, symbol)) = target.split_once("::") {
        if path.contains('/') || path.contains('\\') || path.contains('.') {
            return (path.to_owned(), Some(symbol.to_owned()));
        }
    }
    if target.contains('/') || target.contains('\\') || target.contains('.') {
        (target.to_owned(), None)
    } else {
        (String::new(), Some(target.to_owned()))
    }
}

fn build_changes(
    state: &design::DesignState,
    review: &ChangeReviewReport,
) -> Vec<ProjectChangeView> {
    review
        .files
        .iter()
        .take(MAX_PROJECT_CHANGES)
        .map(|file| change_view(state, file))
        .collect()
}

fn change_view(state: &design::DesignState, file: &ChangedFileReview) -> ProjectChangeView {
    let mut affected_components = state
        .components
        .values()
        .filter(|component| {
            component
                .implementation
                .iter()
                .any(|reference| reference.path() == file.path)
        })
        .map(|component| component.id.clone())
        .collect::<Vec<_>>();
    affected_components.sort();
    let component_set = affected_components.iter().collect::<HashSet<_>>();
    let mut affected_requirements = state
        .requirements
        .values()
        .filter(|requirement| {
            requirement
                .implemented_by
                .iter()
                .any(|component| component_set.contains(component))
        })
        .map(|requirement| requirement.id.clone())
        .collect::<Vec<_>>();
    if file.path.starts_with(".wcode/design/") {
        affected_requirements.extend(state.requirements.keys().cloned());
    }
    affected_requirements.sort();
    affected_requirements.dedup();
    ProjectChangeView {
        path: file.path.clone(),
        status: file.status.clone(),
        category: file.category.clone(),
        scope: scopes::source_scope(&file.path).map(|scope| scope.as_str().to_owned()),
        additions: file.additions,
        deletions: file.deletions,
        staged: file.staged,
        unstaged: file.unstaged,
        untracked: file.untracked,
        affected_requirements,
        affected_components,
    }
}
