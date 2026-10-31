use crate::design::{self, DesignState};
use crate::graph::{EdgeKind, GraphNode, GraphPrecision, NodeKind, SoftwareGraphSnapshot};
use crate::intelligence_types::{
    FeatureDependencyAlignment, FeatureDependencyEvidence, FeatureDependencyOwner,
    ProjectArchitectureComponentView, ProjectArchitectureSubsystemView, ProjectArchitectureView,
    TraceReferenceKind, TraceabilityStatus,
};
use crate::scopes;
use std::collections::{BTreeMap, BTreeSet, HashSet};

const MAX_DEPENDENCY_EVIDENCE: usize = 8;
const MAX_DEPENDENCY_CANDIDATES: usize = 128;
const MAX_DEPENDENCY_OWNERS: usize = 16;

type ComponentOwners = BTreeMap<String, BTreeSet<String>>;

#[derive(Default)]
pub(super) struct ComponentDependencyObservation {
    pub precision: String,
    pub evidence: Vec<FeatureDependencyEvidence>,
    pub evidence_total: usize,
}

#[derive(Default)]
pub(super) struct ComponentDependencies {
    pub confirmed: BTreeMap<(String, String), ComponentDependencyObservation>,
    pub candidates: Vec<FeatureDependencyEvidence>,
    pub candidates_total: usize,
}

pub(super) fn actual_component_dependencies(
    state: &DesignState,
    traceability: &TraceabilityStatus,
    graph: &SoftwareGraphSnapshot,
) -> ComponentDependencies {
    let unresolved_symbols = traceability
        .requirements
        .iter()
        .flat_map(|requirement| &requirement.implementation)
        .filter(|reference| {
            reference.kind == TraceReferenceKind::Symbol
                && (!reference.resolved || reference.node_id.is_none())
        })
        .map(|reference| (reference.owner.as_str(), reference.target.as_str()))
        .collect::<HashSet<_>>();
    let mut nodes_by_path = BTreeMap::<&str, Vec<&GraphNode>>::new();
    for node in graph.graph.nodes.values() {
        if let Some(path) = graph_node_path(node) {
            nodes_by_path.entry(path).or_default().push(node);
        }
    }
    let mut node_owners = BTreeMap::<String, ComponentOwners>::new();
    for component in state.components.values() {
        for reference in &component.implementation {
            let Some(nodes) = nodes_by_path.get(reference.path()) else {
                continue;
            };
            let (matches, source) = match reference {
                design::CodeRef::File { .. } => (nodes.clone(), "file_reference"),
                design::CodeRef::Symbol { symbol, .. } => {
                    let matching = |key| {
                        nodes
                            .iter()
                            .copied()
                            .filter(|node| {
                                node.kind != NodeKind::File
                                    && node.attributes.get(key).and_then(serde_json::Value::as_str)
                                        == Some(symbol.as_str())
                            })
                            .collect::<Vec<_>>()
                    };
                    let mut matches = matching("qualified_name");
                    if matches.is_empty() {
                        matches = matching("name");
                    }
                    let target = format!("{}::{symbol}", reference.path());
                    let source = if matches.len() == 1
                        && !graph.truncated
                        && !graph.scan_truncated
                        && !unresolved_symbols.contains(&(component.id.as_str(), target.as_str()))
                    {
                        "symbol_reference"
                    } else {
                        "ambiguous_symbol_reference"
                    };
                    (matches, source)
                }
            };
            for node in matches {
                node_owners
                    .entry(node.id.clone())
                    .or_default()
                    .entry(component.id.clone())
                    .or_default()
                    .insert(source.to_owned());
            }
        }
    }
    for requirement in &traceability.requirements {
        for reference in &requirement.implementation {
            if !reference.resolved || !state.components.contains_key(&reference.owner) {
                continue;
            }
            let Some(node) = reference
                .node_id
                .as_ref()
                .and_then(|id| graph.graph.nodes.get(id))
            else {
                continue;
            };
            let source = match reference.kind {
                TraceReferenceKind::Symbol if node.kind != NodeKind::File => "trace_symbol",
                TraceReferenceKind::File if node.kind == NodeKind::File => "trace_file",
                _ => continue,
            };
            node_owners
                .entry(node.id.clone())
                .or_default()
                .entry(reference.owner.clone())
                .or_default()
                .insert(source.to_owned());
        }
    }

    let no_owners = ComponentOwners::new();
    let mut actual = ComponentDependencies::default();
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
        let from = node_owners.get(&edge.from).unwrap_or(&no_owners);
        let to = node_owners.get(&edge.to).unwrap_or(&no_owners);
        if from.is_empty() && to.is_empty() {
            continue;
        }
        let pair = unique_component_owner(from).zip(unique_component_owner(to));
        let unique = pair.is_some();
        if unique && pair.is_some_and(|(from, to)| from == to) {
            continue;
        }
        let ownership =
            if from.len() > 1 || to.len() > 1 || (!from.is_empty() && !to.is_empty() && !unique) {
                "ambiguous"
            } else if unique {
                "unique"
            } else {
                "unowned"
            };
        let evidence = FeatureDependencyEvidence {
            from_node: edge.from.clone(),
            to_node: edge.to.clone(),
            from_path: graph
                .graph
                .nodes
                .get(&edge.from)
                .and_then(graph_node_path)
                .map(str::to_owned),
            to_path: graph
                .graph
                .nodes
                .get(&edge.to)
                .and_then(graph_node_path)
                .map(str::to_owned),
            from_symbol: graph
                .graph
                .nodes
                .get(&edge.from)
                .and_then(graph_node_symbol)
                .map(str::to_owned),
            to_symbol: graph
                .graph
                .nodes
                .get(&edge.to)
                .and_then(graph_node_symbol)
                .map(str::to_owned),
            kind: edge.kind,
            provider: edge.provenance.provider.clone(),
            precision: precision_name(edge.provenance.precision),
            revision: edge.provenance.revision.clone(),
            ownership: ownership.to_owned(),
            from_owners: owner_samples(from),
            to_owners: owner_samples(to),
            from_owners_total: from.len(),
            to_owners_total: to.len(),
        };
        let observed = matches!(
            edge.provenance.precision,
            GraphPrecision::Syntax
                | GraphPrecision::Semantic
                | GraphPrecision::Deterministic
                | GraphPrecision::Runtime
        );
        if let Some((from, to)) = pair.filter(|_| observed) {
            let observation = actual
                .confirmed
                .entry((from.clone(), to.clone()))
                .or_default();
            observation.evidence_total += 1;
            bounded_dependency_evidence(
                &mut observation.evidence,
                evidence,
                MAX_DEPENDENCY_EVIDENCE,
            );
            observation.precision = observation.evidence[0].precision.clone();
        } else {
            actual.candidates_total += 1;
            bounded_dependency_evidence(
                &mut actual.candidates,
                evidence,
                MAX_DEPENDENCY_CANDIDATES,
            );
        }
    }
    actual
}

fn unique_component_owner(owners: &ComponentOwners) -> Option<&String> {
    if owners.len() != 1 {
        return None;
    }
    let (owner, sources) = owners.first_key_value()?;
    sources
        .iter()
        .any(|source| {
            matches!(
                source.as_str(),
                "symbol_reference" | "file_reference" | "trace_symbol" | "trace_file"
            )
        })
        .then_some(owner)
}

fn owner_samples(owners: &ComponentOwners) -> Vec<FeatureDependencyOwner> {
    owners
        .iter()
        .take(MAX_DEPENDENCY_OWNERS)
        .map(|(component, sources)| FeatureDependencyOwner {
            component: component.clone(),
            sources: sources.iter().cloned().collect(),
        })
        .collect()
}

fn bounded_dependency_evidence(
    samples: &mut Vec<FeatureDependencyEvidence>,
    evidence: FeatureDependencyEvidence,
    limit: usize,
) {
    samples.push(evidence);
    samples.sort_by(|left, right| {
        precision_rank(&right.precision)
            .cmp(&precision_rank(&left.precision))
            .then_with(|| left.from_node.cmp(&right.from_node))
            .then_with(|| left.to_node.cmp(&right.to_node))
            .then_with(|| left.provider.cmp(&right.provider))
            .then_with(|| format!("{:?}", left.kind).cmp(&format!("{:?}", right.kind)))
            .then_with(|| left.revision.cmp(&right.revision))
    });
    samples.truncate(limit);
}

pub(super) fn graph_node_path(node: &GraphNode) -> Option<&str> {
    node.attributes
        .get("path")
        .and_then(serde_json::Value::as_str)
}

fn graph_node_symbol(node: &GraphNode) -> Option<&str> {
    if node.kind == NodeKind::File {
        return None;
    }
    ["qualified_name", "name"].iter().find_map(|key| {
        node.attributes
            .get(*key)
            .and_then(serde_json::Value::as_str)
    })
}

pub(super) fn precision_name(precision: GraphPrecision) -> String {
    format!("{precision:?}").to_ascii_lowercase()
}

pub(super) fn precision_rank(value: &str) -> u8 {
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

pub(super) fn build_project_architecture(
    state: &DesignState,
    path_lines: &BTreeMap<String, usize>,
    changed_paths: &HashSet<String>,
    dependencies: Vec<FeatureDependencyAlignment>,
    observations: &ComponentDependencies,
) -> ProjectArchitectureView {
    let mut requirements_by_component = BTreeMap::<String, Vec<String>>::new();
    for requirement in state.requirements.values() {
        for component in &requirement.implemented_by {
            requirements_by_component
                .entry(component.clone())
                .or_default()
                .push(requirement.id.clone());
        }
    }
    for requirements in requirements_by_component.values_mut() {
        requirements.sort();
        requirements.dedup();
    }

    let components = state
        .components
        .values()
        .map(|component| {
            let implementation_paths = component
                .implementation
                .iter()
                .map(|reference| reference.path().to_owned())
                .collect::<BTreeSet<_>>();
            let implementation_targets = component
                .implementation
                .iter()
                .map(code_ref_target)
                .collect::<Vec<_>>();
            let implementation_lines = implementation_paths
                .iter()
                .map(|path| path_lines.get(path).copied().unwrap_or_default())
                .sum();
            let changed_component_paths = implementation_paths
                .iter()
                .filter(|path| changed_paths.contains(*path))
                .cloned()
                .collect::<Vec<_>>();
            let product_scopes = implementation_paths
                .iter()
                .filter_map(|path| {
                    scopes::source_scope(path).map(|scope| scope.as_str().to_owned())
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>();
            let subsystem = component_subsystem(&product_scopes, &implementation_paths);
            ProjectArchitectureComponentView {
                id: component.id.clone(),
                name: component.name.clone(),
                responsibilities: component.responsibilities.clone(),
                depends_on: component.depends_on.clone(),
                implementation_targets,
                implementation_files: implementation_paths.len(),
                implementation_lines,
                changed: !changed_component_paths.is_empty(),
                changed_paths: changed_component_paths,
                requirements: requirements_by_component
                    .get(&component.id)
                    .cloned()
                    .unwrap_or_default(),
                product_scopes,
                subsystem,
            }
        })
        .collect::<Vec<_>>();

    let desired_edges = dependencies.iter().filter(|edge| edge.desired).count();
    let observed_edges = dependencies.iter().filter(|edge| edge.actual).count();
    let aligned_edges = dependencies
        .iter()
        .filter(|edge| edge.status == "aligned")
        .count();
    let blocking_drift_edges = dependencies.iter().filter(|edge| edge.blocking).count();
    let advisory_edges = dependencies
        .iter()
        .filter(|edge| !edge.blocking && edge.status != "aligned")
        .count();
    let unverified_edges = dependencies
        .iter()
        .filter(|edge| edge.status == "unverified_actual")
        .count();
    let components_with_implementation = components
        .iter()
        .filter(|component| component.implementation_files > 0)
        .count();

    let subsystems = build_subsystems(&components, &dependencies);

    ProjectArchitectureView {
        subsystems,
        components,
        dependencies,
        dependency_candidates: observations.candidates.clone(),
        dependency_candidates_total: observations.candidates_total,
        dependency_candidates_truncated: observations.candidates_total
            > observations.candidates.len(),
        desired_edges,
        observed_edges,
        aligned_edges,
        blocking_drift_edges,
        advisory_edges,
        unverified_edges,
        components_with_implementation,
        observed_drift_percent: percentage(blocking_drift_edges, observed_edges, 0.0),
        evidence_coverage_percent: percentage(aligned_edges, desired_edges, 100.0),
        implementation_coverage_percent: percentage(
            components_with_implementation,
            state.components.len(),
            0.0,
        ),
    }
}

fn component_subsystem(product_scopes: &[String], paths: &BTreeSet<String>) -> String {
    if let Some(scope) = product_scopes.first() {
        return scope.clone();
    }
    let roots = paths
        .iter()
        .filter_map(|path| derived_source_domain(path))
        .collect::<BTreeSet<_>>();
    match roots.len() {
        0 => "unscoped".to_owned(),
        1 => roots
            .into_iter()
            .next()
            .unwrap_or_else(|| "unscoped".to_owned()),
        _ => "cross-cutting".to_owned(),
    }
}

fn derived_source_domain(path: &str) -> Option<String> {
    let segments = path
        .split('/')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    match segments.as_slice() {
        ["src", file] if file.contains('.') => Some("source:src".to_owned()),
        ["src", domain, ..] => Some(format!("source:{domain}")),
        [root @ ("apps" | "packages" | "services" | "crates" | "modules" | "libs"), domain, ..] => {
            Some(format!("{root}:{domain}"))
        }
        [root, ..] => Some(format!("source:{root}")),
        [] => None,
    }
}

fn subsystem_label(id: &str) -> (String, String) {
    if let Some(scope) = scopes::registry().into_iter().find(|scope| scope.id == id) {
        return (scope.title.to_owned(), scope.purpose.to_owned());
    }
    if id == "cross-cutting" {
        return (
            "Cross-cutting".to_owned(),
            "Components implemented across multiple source domains.".to_owned(),
        );
    }
    if id == "unscoped" {
        return (
            "Unscoped".to_owned(),
            "Components without an explicit Product Scope or stable source-domain owner."
                .to_owned(),
        );
    }
    let raw = id.split_once(':').map(|(_, value)| value).unwrap_or(id);
    let title = raw
        .split(['-', '_'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            chars
                .next()
                .map(|first| format!("{}{}", first.to_uppercase(), chars.as_str()))
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(" ");
    (
        if title.is_empty() {
            id.to_owned()
        } else {
            title
        },
        "Derived from implementation ownership because no explicit subsystem scope is declared."
            .to_owned(),
    )
}

fn build_subsystems(
    components: &[ProjectArchitectureComponentView],
    dependencies: &[FeatureDependencyAlignment],
) -> Vec<ProjectArchitectureSubsystemView> {
    #[derive(Default)]
    struct Acc {
        component_ids: BTreeSet<String>,
        files: BTreeSet<String>,
        lines: usize,
        requirements: BTreeSet<String>,
        changed: usize,
        depends_on: BTreeSet<String>,
        depended_on_by: BTreeSet<String>,
        designed_depends_on: BTreeSet<String>,
        observed_depends_on: BTreeSet<String>,
        unobserved_designed_depends_on: BTreeSet<String>,
        undeclared_observed_depends_on: BTreeSet<String>,
        blocking: usize,
        advisory: usize,
    }

    let component_subsystems = components
        .iter()
        .map(|component| (component.id.clone(), component.subsystem.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut grouped = BTreeMap::<String, Acc>::new();
    for component in components {
        let entry = grouped.entry(component.subsystem.clone()).or_default();
        entry.component_ids.insert(component.id.clone());
        entry
            .files
            .extend(component.implementation_targets.iter().map(|target| {
                target
                    .split_once("::")
                    .map_or(target.as_str(), |(path, _)| path)
                    .to_owned()
            }));
        entry.lines += component.implementation_lines;
        entry
            .requirements
            .extend(component.requirements.iter().cloned());
        entry.changed += usize::from(component.changed);
    }
    for edge in dependencies {
        let Some(from) = component_subsystems.get(&edge.from) else {
            continue;
        };
        let Some(to) = component_subsystems.get(&edge.to) else {
            continue;
        };
        if from != to {
            let from_entry = grouped.entry(from.clone()).or_default();
            from_entry.depends_on.insert(to.clone());
            if edge.desired {
                from_entry.designed_depends_on.insert(to.clone());
            }
            if edge.actual {
                from_entry.observed_depends_on.insert(to.clone());
            }
            if edge.desired && !edge.actual {
                from_entry.unobserved_designed_depends_on.insert(to.clone());
            }
            if edge.actual && !edge.desired {
                from_entry.undeclared_observed_depends_on.insert(to.clone());
            }
            grouped
                .entry(to.clone())
                .or_default()
                .depended_on_by
                .insert(from.clone());
        }
        if edge.blocking {
            grouped.entry(from.clone()).or_default().blocking += 1;
        } else if edge.status != "aligned" {
            grouped.entry(from.clone()).or_default().advisory += 1;
        }
    }

    let dependency_map = grouped
        .iter()
        .map(|(id, acc)| (id.clone(), acc.depends_on.clone()))
        .collect::<BTreeMap<_, _>>();
    let mut memo = BTreeMap::<String, usize>::new();
    let mut result = grouped
        .into_iter()
        .map(|(id, acc)| {
            let (title, purpose) = subsystem_label(&id);
            let layer = subsystem_depth(&id, &dependency_map, &mut memo, &mut BTreeSet::new());
            let component_ids = acc.component_ids.into_iter().collect::<Vec<_>>();
            let component_count = component_ids.len();
            ProjectArchitectureSubsystemView {
                id,
                title,
                purpose,
                layer,
                component_ids,
                components: component_count,
                implementation_files: acc.files.len(),
                implementation_lines: acc.lines,
                requirements: acc.requirements.len(),
                changed_components: acc.changed,
                depends_on: acc.depends_on.into_iter().collect(),
                depended_on_by: acc.depended_on_by.into_iter().collect(),
                designed_depends_on: acc.designed_depends_on.into_iter().collect(),
                observed_depends_on: acc.observed_depends_on.into_iter().collect(),
                unobserved_designed_depends_on: acc
                    .unobserved_designed_depends_on
                    .into_iter()
                    .collect(),
                undeclared_observed_depends_on: acc
                    .undeclared_observed_depends_on
                    .into_iter()
                    .collect(),
                blocking_drift_edges: acc.blocking,
                advisory_edges: acc.advisory,
            }
        })
        .collect::<Vec<_>>();
    result.sort_by(|left, right| {
        right
            .layer
            .cmp(&left.layer)
            .then_with(|| left.title.cmp(&right.title))
    });
    result
}

fn subsystem_depth(
    id: &str,
    dependencies: &BTreeMap<String, BTreeSet<String>>,
    memo: &mut BTreeMap<String, usize>,
    stack: &mut BTreeSet<String>,
) -> usize {
    if let Some(depth) = memo.get(id) {
        return *depth;
    }
    if !stack.insert(id.to_owned()) {
        return 0;
    }
    let depth = dependencies
        .get(id)
        .into_iter()
        .flatten()
        .map(|dependency| subsystem_depth(dependency, dependencies, memo, stack) + 1)
        .max()
        .unwrap_or_default()
        .min(7);
    stack.remove(id);
    memo.insert(id.to_owned(), depth);
    depth
}

fn code_ref_target(reference: &design::CodeRef) -> String {
    match reference {
        design::CodeRef::File { path } => path.clone(),
        design::CodeRef::Symbol { path, symbol } => format!("{path}::{symbol}"),
    }
}

fn percentage(part: usize, total: usize, empty: f64) -> f64 {
    if total == 0 {
        empty
    } else {
        (part as f64 / total as f64) * 100.0
    }
}
