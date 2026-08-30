use crate::workspace::Workspace;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Component, Path};

pub const DESIGN_ROOT: &str = ".wcode/design";
pub const PROJECT_FILE: &str = ".wcode/project.yaml";
const MAX_DESIGN_FILES: usize = 512;
const CURRENT_SCHEMA_VERSION: u32 = 1;

#[derive(Clone, Debug, Default, Serialize)]
pub struct DesignState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<ProjectDesign>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub product: Option<ProductDesign>,
    pub requirements: BTreeMap<String, Requirement>,
    pub components: BTreeMap<String, ComponentDesign>,
    pub constraints: BTreeMap<String, ConstraintDesign>,
    pub decisions: BTreeMap<String, DecisionRecord>,
    pub acceptance: BTreeMap<String, AcceptanceCriterion>,
}

impl DesignState {
    pub fn node_count(&self) -> usize {
        usize::from(self.product.is_some())
            + self.requirements.len()
            + self.components.len()
            + self.constraints.len()
            + self.decisions.len()
            + self.acceptance.len()
    }

    pub fn known_id(&self, id: &str) -> bool {
        self.product.as_ref().is_some_and(|item| item.id == id)
            || self.requirements.contains_key(id)
            || self.components.contains_key(id)
            || self.constraints.contains_key(id)
            || self.decisions.contains_key(id)
            || self.acceptance.contains_key(id)
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct DesignLoad {
    pub initialized: bool,
    pub design_root: String,
    pub files_loaded: usize,
    pub state: DesignState,
    pub diagnostics: Vec<DesignDiagnostic>,
}

impl DesignLoad {
    pub fn error_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error)
            .count()
    }

    pub fn warning_count(&self) -> usize {
        self.diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.severity == DiagnosticSeverity::Warning)
            .count()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

#[derive(Clone, Debug, Serialize)]
pub struct DesignDiagnostic {
    pub severity: DiagnosticSeverity,
    pub code: String,
    pub path: String,
    pub message: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectDesign {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProductDesign {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub vision: String,
    #[serde(default)]
    pub principles: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Requirement {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub intent: String,
    #[serde(default)]
    pub priority: Priority,
    #[serde(default)]
    pub implemented_by: Vec<String>,
    #[serde(default)]
    pub acceptance: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub risk: DesignRisk,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Low,
    #[default]
    Medium,
    High,
    Critical,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DesignRisk {
    #[serde(default)]
    pub security: Option<RiskLevel>,
    #[serde(default)]
    pub compatibility: Option<RiskLevel>,
    #[serde(default)]
    pub performance: Option<RiskLevel>,
    #[serde(default)]
    pub reliability: Option<RiskLevel>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComponentDesign {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub responsibilities: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default)]
    pub constraints: Vec<String>,
    #[serde(default)]
    pub implementation: Vec<CodeRef>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CodeRef {
    File { path: String },
    Symbol { path: String, symbol: String },
}

impl CodeRef {
    pub fn path(&self) -> &str {
        match self {
            Self::File { path } | Self::Symbol { path, .. } => path,
        }
    }

    pub fn symbol(&self) -> Option<&str> {
        match self {
            Self::File { .. } => None,
            Self::Symbol { symbol, .. } => Some(symbol),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ConstraintDesign {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub statement: String,
    #[serde(default)]
    pub applies_to: Vec<String>,
}

pub fn baseline_constraints() -> Vec<ConstraintDesign> {
    vec![
        ConstraintDesign {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: "CONSTRAINT-SOURCE-DECOMPOSITION".into(),
            title: "Source modules stay bounded".into(),
            statement: "Maintained text files must stay at or below 1000 physical lines; generated lockfiles and binary assets are exempt. Split cohesive responsibilities into domain subdirectories before a file crosses that boundary. Repository filenames stay at or below 32 characters, with Rust stems at or below 24, because the directory already supplies context.".into(),
            applies_to: Vec::new(),
        },
        ConstraintDesign {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: "CONSTRAINT-TEST-ROOT".into(),
            title: "Tests live in the repository test root".into(),
            statement: "Standalone automated tests belong under the repository tests directory, grouped by the source domain. Source modules may attach those files for private unit coverage instead of accumulating large inline test modules.".into(),
            applies_to: Vec::new(),
        },
        ConstraintDesign {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: "CONSTRAINT-DESIGN-SYNC".into(),
            title: "Design references move with the code".into(),
            statement: "Changes to responsibilities, source paths, test paths, trust boundaries or transport behavior must update the matching .wcode Design State in the same change.".into(),
            applies_to: Vec::new(),
        },
    ]
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DecisionRecord {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub status: DecisionStatus,
    pub decision: String,
    #[serde(default)]
    pub rationale: String,
    #[serde(default)]
    pub affects: Vec<String>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DecisionStatus {
    Proposed,
    Accepted,
    Superseded,
    Rejected,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceCriterion {
    #[serde(default = "schema_version")]
    pub schema_version: u32,
    pub id: String,
    pub title: String,
    pub statement: String,
    #[serde(default)]
    pub verification: Vec<VerificationRef>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum VerificationRef {
    Test { path: String, symbol: String },
    Check { id: String },
}

pub fn load_design(workspace: &Workspace) -> Result<DesignLoad> {
    let design_dir = workspace.root().join(DESIGN_ROOT);
    let project_path = workspace.root().join(PROJECT_FILE);
    let initialized = design_dir.is_dir() || project_path.is_file();
    let mut load = DesignLoad {
        initialized,
        design_root: DESIGN_ROOT.to_owned(),
        files_loaded: 0,
        state: DesignState::default(),
        diagnostics: Vec::new(),
    };

    if project_path.exists() {
        match workspace.load_source(PROJECT_FILE) {
            Ok(source) => match parse_yaml::<ProjectDesign>(&source.content, PROJECT_FILE) {
                Ok(project) => {
                    validate_schema_version(project.schema_version, PROJECT_FILE, &mut load);
                    load.state.project = Some(project);
                    load.files_loaded += 1;
                }
                Err(message) => push_error(&mut load, "invalid-project", PROJECT_FILE, message),
            },
            Err(error) => push_error(
                &mut load,
                "project-read-failed",
                PROJECT_FILE,
                error.to_string(),
            ),
        }
    }

    if !design_dir.exists() {
        if initialized {
            push_warning(
                &mut load,
                "missing-design-root",
                DESIGN_ROOT,
                "project metadata exists but .wcode/design is missing",
            );
        }
        validate_design_state(&mut load);
        return Ok(load);
    }

    let (files, truncated) = match workspace.source_files(DESIGN_ROOT, MAX_DESIGN_FILES) {
        Ok(result) => result,
        Err(error) => {
            push_error(
                &mut load,
                "design-root-read-failed",
                DESIGN_ROOT,
                error.to_string(),
            );
            validate_design_state(&mut load);
            return Ok(load);
        }
    };
    if truncated {
        push_error(
            &mut load,
            "design-file-limit",
            DESIGN_ROOT,
            format!("design state exceeds the {MAX_DESIGN_FILES}-file safety bound"),
        );
    }

    for path in files {
        if !is_yaml(&path) {
            continue;
        }
        let source = match workspace.load_source(&path) {
            Ok(source) => source,
            Err(error) => {
                push_error(
                    &mut load,
                    "design-file-read-failed",
                    &path,
                    error.to_string(),
                );
                continue;
            }
        };
        let relative = path
            .strip_prefix(DESIGN_ROOT)
            .and_then(|path| path.strip_prefix('/'))
            .unwrap_or(path.as_str());
        let loaded = if relative == "product.yaml" || relative == "product.yml" {
            parse_and_insert_product(&mut load, &path, &source.content)
        } else if relative == "requirements.yaml" || relative == "requirements.yml" {
            parse_and_insert_requirements(&mut load, &path, &source.content)
        } else if relative == "components.yaml" || relative == "components.yml" {
            parse_and_insert_components(&mut load, &path, &source.content)
        } else if relative == "constraints.yaml" || relative == "constraints.yml" {
            parse_and_insert_constraints(&mut load, &path, &source.content)
        } else if relative == "decisions.yaml" || relative == "decisions.yml" {
            parse_and_insert_decisions(&mut load, &path, &source.content)
        } else if relative == "acceptance.yaml" || relative == "acceptance.yml" {
            parse_and_insert_acceptance_collection(&mut load, &path, &source.content)
        } else if relative.starts_with("requirements/") {
            parse_and_insert_requirement(&mut load, &path, &source.content)
        } else if relative.starts_with("components/") {
            parse_and_insert_component(&mut load, &path, &source.content)
        } else if relative.starts_with("constraints/") {
            parse_and_insert_constraint(&mut load, &path, &source.content)
        } else if relative.starts_with("decisions/") {
            parse_and_insert_decision(&mut load, &path, &source.content)
        } else if relative.starts_with("acceptance/") {
            parse_and_insert_acceptance(&mut load, &path, &source.content)
        } else {
            push_warning(
                &mut load,
                "unknown-design-document",
                &path,
                "YAML file is outside a recognized Design State collection",
            );
            false
        };
        load.files_loaded += usize::from(loaded);
    }

    validate_design_state(&mut load);
    Ok(load)
}

fn parse_and_insert_product(load: &mut DesignLoad, path: &str, content: &str) -> bool {
    match parse_yaml::<ProductDesign>(content, path) {
        Ok(item) => {
            validate_schema_version(item.schema_version, path, load);
            if load.state.product.is_some() {
                push_error(
                    load,
                    "duplicate-product",
                    path,
                    "only one product design document is allowed",
                );
                false
            } else {
                load.state.product = Some(item);
                true
            }
        }
        Err(message) => {
            push_error(load, "invalid-product", path, message);
            false
        }
    }
}

fn parse_and_insert_requirements(load: &mut DesignLoad, path: &str, content: &str) -> bool {
    let items = match parse_yaml::<Vec<Requirement>>(content, path) {
        Ok(items) => items,
        Err(message) => {
            push_error(load, "invalid-requirement-collection", path, message);
            return false;
        }
    };
    for item in items {
        insert_named(
            &mut load.state.requirements,
            item.id.clone(),
            item.schema_version,
            item,
            path,
            "requirement",
            &mut load.diagnostics,
        );
    }
    true
}

fn parse_and_insert_components(load: &mut DesignLoad, path: &str, content: &str) -> bool {
    let items = match parse_yaml::<Vec<ComponentDesign>>(content, path) {
        Ok(items) => items,
        Err(message) => {
            push_error(load, "invalid-component-collection", path, message);
            return false;
        }
    };
    for item in items {
        insert_named(
            &mut load.state.components,
            item.id.clone(),
            item.schema_version,
            item,
            path,
            "component",
            &mut load.diagnostics,
        );
    }
    true
}

fn parse_and_insert_constraints(load: &mut DesignLoad, path: &str, content: &str) -> bool {
    let items = match parse_yaml::<Vec<ConstraintDesign>>(content, path) {
        Ok(items) => items,
        Err(message) => {
            push_error(load, "invalid-constraint-collection", path, message);
            return false;
        }
    };
    for item in items {
        insert_named(
            &mut load.state.constraints,
            item.id.clone(),
            item.schema_version,
            item,
            path,
            "constraint",
            &mut load.diagnostics,
        );
    }
    true
}

fn parse_and_insert_decisions(load: &mut DesignLoad, path: &str, content: &str) -> bool {
    let items = match parse_yaml::<Vec<DecisionRecord>>(content, path) {
        Ok(items) => items,
        Err(message) => {
            push_error(load, "invalid-decision-collection", path, message);
            return false;
        }
    };
    for item in items {
        insert_named(
            &mut load.state.decisions,
            item.id.clone(),
            item.schema_version,
            item,
            path,
            "decision",
            &mut load.diagnostics,
        );
    }
    true
}

fn parse_and_insert_acceptance_collection(
    load: &mut DesignLoad,
    path: &str,
    content: &str,
) -> bool {
    let items = match parse_yaml::<Vec<AcceptanceCriterion>>(content, path) {
        Ok(items) => items,
        Err(message) => {
            push_error(load, "invalid-acceptance-collection", path, message);
            return false;
        }
    };
    for item in items {
        insert_named(
            &mut load.state.acceptance,
            item.id.clone(),
            item.schema_version,
            item,
            path,
            "acceptance criterion",
            &mut load.diagnostics,
        );
    }
    true
}

fn parse_and_insert_requirement(load: &mut DesignLoad, path: &str, content: &str) -> bool {
    match parse_yaml::<Requirement>(content, path) {
        Ok(item) => insert_named(
            &mut load.state.requirements,
            item.id.clone(),
            item.schema_version,
            item,
            path,
            "requirement",
            &mut load.diagnostics,
        ),
        Err(message) => {
            push_error(load, "invalid-requirement", path, message);
            false
        }
    }
}

fn parse_and_insert_component(load: &mut DesignLoad, path: &str, content: &str) -> bool {
    match parse_yaml::<ComponentDesign>(content, path) {
        Ok(item) => insert_named(
            &mut load.state.components,
            item.id.clone(),
            item.schema_version,
            item,
            path,
            "component",
            &mut load.diagnostics,
        ),
        Err(message) => {
            push_error(load, "invalid-component", path, message);
            false
        }
    }
}

fn parse_and_insert_constraint(load: &mut DesignLoad, path: &str, content: &str) -> bool {
    match parse_yaml::<ConstraintDesign>(content, path) {
        Ok(item) => insert_named(
            &mut load.state.constraints,
            item.id.clone(),
            item.schema_version,
            item,
            path,
            "constraint",
            &mut load.diagnostics,
        ),
        Err(message) => {
            push_error(load, "invalid-constraint", path, message);
            false
        }
    }
}

fn parse_and_insert_decision(load: &mut DesignLoad, path: &str, content: &str) -> bool {
    match parse_yaml::<DecisionRecord>(content, path) {
        Ok(item) => insert_named(
            &mut load.state.decisions,
            item.id.clone(),
            item.schema_version,
            item,
            path,
            "decision",
            &mut load.diagnostics,
        ),
        Err(message) => {
            push_error(load, "invalid-decision", path, message);
            false
        }
    }
}

fn parse_and_insert_acceptance(load: &mut DesignLoad, path: &str, content: &str) -> bool {
    match parse_yaml::<AcceptanceCriterion>(content, path) {
        Ok(item) => insert_named(
            &mut load.state.acceptance,
            item.id.clone(),
            item.schema_version,
            item,
            path,
            "acceptance criterion",
            &mut load.diagnostics,
        ),
        Err(message) => {
            push_error(load, "invalid-acceptance", path, message);
            false
        }
    }
}

fn insert_named<T>(
    map: &mut BTreeMap<String, T>,
    id: String,
    version: u32,
    item: T,
    path: &str,
    kind: &str,
    diagnostics: &mut Vec<DesignDiagnostic>,
) -> bool {
    if version != CURRENT_SCHEMA_VERSION {
        diagnostics.push(DesignDiagnostic {
            severity: DiagnosticSeverity::Error,
            code: "unsupported-schema-version".to_owned(),
            path: path.to_owned(),
            message: format!(
                "{kind} uses schema_version {version}; supported version is {CURRENT_SCHEMA_VERSION}"
            ),
        });
    }
    if !valid_design_id(&id) {
        diagnostics.push(DesignDiagnostic {
            severity: DiagnosticSeverity::Error,
            code: "invalid-design-id".to_owned(),
            path: path.to_owned(),
            message: format!("{kind} id is invalid: {id}"),
        });
        return false;
    }
    if map.insert(id.clone(), item).is_some() {
        diagnostics.push(DesignDiagnostic {
            severity: DiagnosticSeverity::Error,
            code: "duplicate-design-id".to_owned(),
            path: path.to_owned(),
            message: format!("duplicate {kind} id: {id}"),
        });
        return false;
    }
    true
}

fn validate_schema_version(version: u32, path: &str, load: &mut DesignLoad) {
    if version != CURRENT_SCHEMA_VERSION {
        push_error(
            load,
            "unsupported-schema-version",
            path,
            format!(
                "document uses schema_version {version}; supported version is {CURRENT_SCHEMA_VERSION}"
            ),
        );
    }
}

fn validate_design_state(load: &mut DesignLoad) {
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

fn parse_yaml<T>(content: &str, path: &str) -> std::result::Result<T, String>
where
    T: for<'de> Deserialize<'de>,
{
    serde_yaml::from_str(content).map_err(|error| format!("{path}: {error}"))
}

fn valid_design_id(value: &str) -> bool {
    !value.trim().is_empty()
        && value.len() <= 160
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.'))
}

fn valid_repo_path(value: &str) -> bool {
    if value.is_empty() || value.chars().any(char::is_control) {
        return false;
    }
    let path = Path::new(value);
    !path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

fn is_yaml(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("yaml") || extension.eq_ignore_ascii_case("yml")
        })
}

fn push_error(load: &mut DesignLoad, code: &str, path: &str, message: impl Into<String>) {
    load.diagnostics.push(DesignDiagnostic {
        severity: DiagnosticSeverity::Error,
        code: code.to_owned(),
        path: path.to_owned(),
        message: message.into(),
    });
}

fn push_warning(load: &mut DesignLoad, code: &str, path: &str, message: impl Into<String>) {
    load.diagnostics.push(DesignDiagnostic {
        severity: DiagnosticSeverity::Warning,
        code: code.to_owned(),
        path: path.to_owned(),
        message: message.into(),
    });
}

const fn schema_version() -> u32 {
    CURRENT_SCHEMA_VERSION
}

#[cfg(test)]
#[path = "../../tests/unit/design/mod.rs"]
mod tests;
