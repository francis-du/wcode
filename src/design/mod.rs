use crate::workspace::Workspace;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::hash::{DefaultHasher, Hash, Hasher};

mod local;
mod project;
mod support;
pub use local::load_local_design;
pub use project::{
    AcceptancePolicy, AcceptanceRule, DocsOnlyPolicy, PolicyChangeSet, PolicyLevel, PolicyPath,
    PolicyPathMappings, PolicyRequirements, PolicySelection, PolicySelector, ProjectDesign,
};
use support::{
    apply_core_constraints, is_yaml, parse_yaml, push_error, push_warning, schema_version,
    valid_design_id, validate_design_state,
};

pub const DESIGN_ROOT: &str = ".wcode/design";
pub const PROJECT_FILE: &str = ".wcode/project.yaml";
pub(crate) const MAX_DESIGN_FILES: usize = 512;

#[cfg(test)]
thread_local! {
    pub(crate) static LOAD_DESIGN_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}
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

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
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
            statement: "Maintained text files must stay at or below 1000 physical lines; generated source outputs, lockfiles, and binary assets are exempt. Split cohesive responsibilities into domain subdirectories before a file crosses that boundary. Repository filenames stay at or below 32 characters, with Rust stems at or below 24, because the directory already supplies context.".into(),
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
        ConstraintDesign {
            schema_version: CURRENT_SCHEMA_VERSION,
            id: "CONSTRAINT-PARALLEL-FIRST".into(),
            title: "Independent work executes in parallel".into(),
            statement: "When Agent Context or a Worklist exposes two or more independent runnable lanes, execution must fan them out concurrently up to the runtime cap. Serialize only true data dependencies, overlapping writes, shared mutable state, or output-dependent follow-ups; serial execution of known independent lanes is non-compliant. Use the lowest-overhead safe primitive: prefer one-traversal bulk operations such as read_files, search_many, apply_file_edits, create_files or move_paths before per-item fan-out, reuse shared snapshots and caches, and avoid duplicate repository traversals. Hosts that cannot issue concurrent top-level calls must use wcode parallel_tools for compact known operations. Resource-specific subqueues may be narrower than the outer tool cap, but their capacity must be reported separately, process admission wait must remain bounded to five seconds, and saturation must not masquerade as available outer-slot throughput; live queue waits beyond that bound are explicit Runtime Drift signals with expected, observed and deviation-percent telemetry.".into(),
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

pub(crate) fn fingerprint(workspace: &Workspace) -> Result<u64> {
    let mut hasher = DefaultHasher::new();
    workspace.root().hash(&mut hasher);
    let project_path = workspace.root().join(PROJECT_FILE);
    project_path.is_file().hash(&mut hasher);
    if project_path.is_file() {
        PROJECT_FILE.hash(&mut hasher);
        workspace
            .load_source(PROJECT_FILE)?
            .sha256
            .hash(&mut hasher);
    }

    let design_dir = workspace.root().join(DESIGN_ROOT);
    design_dir.is_dir().hash(&mut hasher);
    if design_dir.is_dir() {
        let (files, truncated) = workspace.source_files(DESIGN_ROOT, MAX_DESIGN_FILES)?;
        truncated.hash(&mut hasher);
        files.len().hash(&mut hasher);
        for path in files {
            path.hash(&mut hasher);
            workspace.load_source(&path)?.sha256.hash(&mut hasher);
        }
    }
    Ok(hasher.finish())
}

pub fn load_design(workspace: &Workspace) -> Result<DesignLoad> {
    #[cfg(test)]
    LOAD_DESIGN_CALLS.with(|count| count.set(count.get() + 1));
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
        apply_core_constraints(&mut load);
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
            apply_core_constraints(&mut load);
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

    apply_core_constraints(&mut load);
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

#[cfg(test)]
#[path = "../../tests/unit/design/mod.rs"]
mod tests;
