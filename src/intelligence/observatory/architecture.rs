use crate::design::{self, DesignState};
use crate::intelligence_types::{
    FeatureDependencyAlignment, ProjectArchitectureComponentView, ProjectArchitectureSubsystemView,
    ProjectArchitectureView,
};
use crate::scopes;
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub(super) fn build_project_architecture(
    state: &DesignState,
    path_lines: &BTreeMap<String, usize>,
    changed_paths: &HashSet<String>,
    dependencies: Vec<FeatureDependencyAlignment>,
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
