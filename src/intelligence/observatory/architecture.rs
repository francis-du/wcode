use crate::design::{self, DesignState};
use crate::intelligence_types::{
    FeatureDependencyAlignment, ProjectArchitectureComponentView, ProjectArchitectureView,
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

    ProjectArchitectureView {
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
