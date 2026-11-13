use super::{
    baseline_constraints, project, DesignDiagnostic, DesignLoad, DiagnosticSeverity,
    VerificationRef, CURRENT_SCHEMA_VERSION, DESIGN_ROOT,
};
use serde::Deserialize;
use std::collections::{BTreeSet, HashSet};
use std::path::{Component, Path};

pub(super) fn apply_core_constraints(load: &mut DesignLoad) {
    for core in baseline_constraints() {
        match load.state.constraints.get(&core.id).cloned() {
            Some(existing) if existing == core => {}
            Some(_) => {
                push_error(
                    load,
                    "core-constraint-override",
                    &core.id,
                    "project Design State cannot override or weaken a wcode core constraint; the canonical runtime constraint remains in force",
                );
                load.state.constraints.insert(core.id.clone(), core);
            }
            None => {
                load.state.constraints.insert(core.id.clone(), core);
            }
        }
    }
}

pub(super) fn validate_design_state(load: &mut DesignLoad) {
    project::validate_project_policy(load);
    let mut seen = HashSet::new();
    let ids = load
        .state
        .product
        .iter()
        .map(|item| item.id.as_str())
        .chain(load.state.requirements.keys().map(String::as_str))
        .chain(load.state.components.keys().map(String::as_str))
        .chain(load.state.constraints.keys().map(String::as_str))
        .chain(load.state.decisions.keys().map(String::as_str))
        .chain(load.state.acceptance.keys().map(String::as_str));
    for id in ids {
        if !seen.insert(id.to_owned()) {
            load.diagnostics.push(DesignDiagnostic {
                severity: DiagnosticSeverity::Error,
                code: "cross-kind-duplicate-id".to_owned(),
                path: DESIGN_ROOT.to_owned(),
                message: format!("design id is reused across document kinds: {id}"),
            });
        }
    }

    let requirements = load
        .state
        .requirements
        .values()
        .cloned()
        .collect::<Vec<_>>();
    for requirement in requirements {
        validate_required_text(load, &requirement.id, &requirement.title, "title");
        validate_required_text(load, &requirement.id, &requirement.intent, "intent");
        validate_unique_refs(
            load,
            &requirement.id,
            "implemented_by",
            &requirement.implemented_by,
        );
        validate_unique_refs(load, &requirement.id, "acceptance", &requirement.acceptance);
        validate_unique_refs(
            load,
            &requirement.id,
            "constraints",
            &requirement.constraints,
        );
        for component in &requirement.implemented_by {
            if !load.state.components.contains_key(component) {
                missing_ref(load, &requirement.id, "component", component);
            }
        }
        for acceptance in &requirement.acceptance {
            if !load.state.acceptance.contains_key(acceptance) {
                missing_ref(load, &requirement.id, "acceptance criterion", acceptance);
            }
        }
        for constraint in &requirement.constraints {
            if !load.state.constraints.contains_key(constraint) {
                missing_ref(load, &requirement.id, "constraint", constraint);
            }
        }
    }

    let components = load.state.components.values().cloned().collect::<Vec<_>>();
    for component in components {
        validate_required_text(load, &component.id, &component.name, "name");
        validate_unique_refs(load, &component.id, "depends_on", &component.depends_on);
        validate_unique_refs(load, &component.id, "constraints", &component.constraints);
        for dependency in &component.depends_on {
            if !load.state.components.contains_key(dependency) {
                missing_ref(load, &component.id, "component dependency", dependency);
            }
        }
        for constraint in &component.constraints {
            if !load.state.constraints.contains_key(constraint) {
                missing_ref(load, &component.id, "constraint", constraint);
            }
        }
        for code_ref in &component.implementation {
            if !valid_repo_path(code_ref.path()) {
                load.diagnostics.push(DesignDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    code: "invalid-code-reference".to_owned(),
                    path: component.id.clone(),
                    message: format!("invalid repository-relative path: {}", code_ref.path()),
                });
            }
            if code_ref
                .symbol()
                .is_some_and(|symbol| symbol.trim().is_empty())
            {
                load.diagnostics.push(DesignDiagnostic {
                    severity: DiagnosticSeverity::Error,
                    code: "invalid-symbol-reference".to_owned(),
                    path: component.id.clone(),
                    message: "symbol references must not be empty".to_owned(),
                });
            }
        }
    }

    let constraints = load.state.constraints.values().cloned().collect::<Vec<_>>();
    for constraint in constraints {
        validate_required_text(load, &constraint.id, &constraint.title, "title");
        validate_required_text(load, &constraint.id, &constraint.statement, "statement");
        for target in &constraint.applies_to {
            if !load.state.known_id(target) {
                missing_ref(load, &constraint.id, "design target", target);
            }
        }
    }

    let decisions = load.state.decisions.values().cloned().collect::<Vec<_>>();
    for decision in decisions {
        validate_required_text(load, &decision.id, &decision.title, "title");
        validate_required_text(load, &decision.id, &decision.decision, "decision");
        for target in &decision.affects {
            if !load.state.known_id(target) {
                missing_ref(load, &decision.id, "design target", target);
            }
        }
    }

    let acceptance = load.state.acceptance.values().cloned().collect::<Vec<_>>();
    for criterion in acceptance {
        validate_required_text(load, &criterion.id, &criterion.title, "title");
        validate_required_text(load, &criterion.id, &criterion.statement, "statement");
        for verification in &criterion.verification {
            match verification {
                VerificationRef::Test { path, symbol } => {
                    if !valid_repo_path(path) || symbol.trim().is_empty() {
                        load.diagnostics.push(DesignDiagnostic {
                            severity: DiagnosticSeverity::Error,
                            code: "invalid-test-reference".to_owned(),
                            path: criterion.id.clone(),
                            message: format!("invalid test reference: {path}::{symbol}"),
                        });
                    }
                }
                VerificationRef::Check { id } if id.trim().is_empty() => {
                    load.diagnostics.push(DesignDiagnostic {
                        severity: DiagnosticSeverity::Error,
                        code: "invalid-check-reference".to_owned(),
                        path: criterion.id.clone(),
                        message: "verification check id must not be empty".to_owned(),
                    });
                }
                VerificationRef::Check { .. } => {}
            }
        }
    }
}

fn validate_required_text(load: &mut DesignLoad, id: &str, value: &str, field: &str) {
    if value.trim().is_empty() {
        load.diagnostics.push(DesignDiagnostic {
            severity: DiagnosticSeverity::Error,
            code: "empty-design-field".to_owned(),
            path: id.to_owned(),
            message: format!("{field} must not be empty"),
        });
    }
}

fn validate_unique_refs(load: &mut DesignLoad, id: &str, field: &str, values: &[String]) {
    let mut seen = BTreeSet::new();
    for value in values {
        if !seen.insert(value) {
            load.diagnostics.push(DesignDiagnostic {
                severity: DiagnosticSeverity::Error,
                code: "duplicate-design-reference".to_owned(),
                path: id.to_owned(),
                message: format!("{field} repeats reference {value}"),
            });
        }
    }
}

fn missing_ref(load: &mut DesignLoad, source: &str, kind: &str, target: &str) {
    load.diagnostics.push(DesignDiagnostic {
        severity: DiagnosticSeverity::Error,
        code: "missing-design-reference".to_owned(),
        path: source.to_owned(),
        message: format!("references unknown {kind}: {target}"),
    });
}

pub(super) fn parse_yaml<T>(content: &str, path: &str) -> std::result::Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    serde_yaml::from_str(content).map_err(|error| format!("{path}: {error}"))
}

pub(super) fn valid_design_id(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.'))
}

pub(super) fn valid_repo_path(value: &str) -> bool {
    if value.is_empty() || value.chars().any(char::is_control) {
        return false;
    }
    let path = Path::new(value);
    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

pub(super) fn is_yaml(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("yaml") || extension.eq_ignore_ascii_case("yml")
        })
}

pub(super) fn push_error(
    load: &mut DesignLoad,
    code: &str,
    path: &str,
    message: impl Into<String>,
) {
    load.diagnostics.push(DesignDiagnostic {
        severity: DiagnosticSeverity::Error,
        code: code.to_owned(),
        path: path.to_owned(),
        message: message.into(),
    });
}

pub(super) fn push_warning(
    load: &mut DesignLoad,
    code: &str,
    path: &str,
    message: impl Into<String>,
) {
    load.diagnostics.push(DesignDiagnostic {
        severity: DiagnosticSeverity::Warning,
        code: code.to_owned(),
        path: path.to_owned(),
        message: message.into(),
    });
}

pub(super) const fn schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}
