use crate::evidence::Revision;
use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};
use std::time::Instant;

#[path = "migration_accumulator.rs"]
mod migration_accumulator;
pub(crate) use migration_accumulator::{audit, capabilities, context_summary};

pub(crate) const CONFIG_PATH: &str = ".wcode/migration.yaml";
const MAX_TRANSITIONS: usize = 16;
const MAX_PATHS: usize = 16;
const MAX_AUDIT_FILES: usize = 2_500;
const MAX_AUDIT_BYTES: usize = 16 * 1024 * 1024;
const MAX_PATTERN_CHARS: usize = 256;
const MAX_FINDINGS: usize = 48;
const MAX_SAMPLE_LOCATIONS: usize = 4;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MigrationAuditSpec {
    #[serde(default = "schema_version")]
    schema_version: u32,
    id: String,
    #[serde(default)]
    scopes: Vec<String>,
    #[serde(default)]
    exclude_paths: Vec<String>,
    #[serde(default)]
    required_paths: Vec<String>,
    #[serde(default)]
    forbidden_paths: Vec<String>,
    #[serde(default)]
    transitions: Vec<MigrationTransition>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct MigrationTransition {
    id: String,
    #[serde(default)]
    legacy: Option<String>,
    #[serde(default)]
    replacement: Option<String>,
    #[serde(default)]
    paths: Vec<String>,
    #[serde(default)]
    min_replacement_matches: usize,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MigrationAuditLocation {
    pub path: String,
    pub line: usize,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MigrationAuditFinding {
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rule_id: Option<String>,
    pub message: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub locations: Vec<MigrationAuditLocation>,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MigrationTransitionResult {
    pub id: String,
    pub legacy_matches: usize,
    pub replacement_matches: usize,
    pub min_replacement_matches: usize,
    pub mixed_state: bool,
}

#[derive(Clone, Debug, Serialize)]
pub(crate) struct MigrationAuditReport {
    pub id: String,
    pub config_path: &'static str,
    pub provider: &'static str,
    pub precision: &'static str,
    pub revision: Revision,
    pub passed: bool,
    pub scanned_files: usize,
    pub scanned_bytes: usize,
    pub truncated: bool,
    pub findings_truncated: bool,
    pub required_paths: usize,
    pub forbidden_paths: usize,
    pub transitions: Vec<MigrationTransitionResult>,
    pub findings: Vec<MigrationAuditFinding>,
    pub summary: String,
    pub elapsed_ms: u128,
}

fn load_spec(workspace: &Workspace) -> Result<Option<MigrationAuditSpec>> {
    if !config_present(workspace) {
        return Ok(None);
    }
    let file = workspace.load_source(CONFIG_PATH)?;
    let spec: MigrationAuditSpec =
        serde_yaml::from_str(&file.content).context("invalid .wcode/migration.yaml")?;
    validate_spec(&spec)?;
    Ok(Some(spec))
}

fn validate_spec(spec: &MigrationAuditSpec) -> Result<()> {
    if spec.schema_version != 1
        || !valid_id(&spec.id)
        || spec.scopes.len() > MAX_PATHS
        || spec.exclude_paths.len() > MAX_PATHS
        || spec.required_paths.len() > MAX_PATHS
        || spec.forbidden_paths.len() > MAX_PATHS
        || spec.transitions.len() > MAX_TRANSITIONS
        || (spec.transitions.is_empty()
            && spec.required_paths.is_empty()
            && spec.forbidden_paths.is_empty())
        || spec
            .scopes
            .iter()
            .chain(&spec.exclude_paths)
            .chain(&spec.required_paths)
            .chain(&spec.forbidden_paths)
            .any(|path| !valid_path_text(path))
    {
        bail!("migration audit config is invalid or exceeds its bounds");
    }
    let mut ids = BTreeSet::new();
    for transition in &spec.transitions {
        if !valid_id(&transition.id)
            || !ids.insert(transition.id.as_str())
            || transition.paths.len() > MAX_PATHS
            || transition.paths.iter().any(|path| !valid_path_text(path))
            || transition
                .legacy
                .as_ref()
                .is_some_and(|pattern| !valid_pattern(pattern))
            || transition
                .replacement
                .as_ref()
                .is_some_and(|pattern| !valid_pattern(pattern))
            || (transition.legacy.is_none() && transition.replacement.is_none())
            || (transition.paths.is_empty() && spec.scopes.is_empty())
            || (transition.replacement.is_none() && transition.min_replacement_matches != 0)
            || transition.min_replacement_matches > 100_000
        {
            bail!(
                "migration transition `{}` is invalid or exceeds its bounds",
                transition.id
            );
        }
    }
    Ok(())
}

fn invalid_report(revision: &Revision, message: String, elapsed_ms: u128) -> MigrationAuditReport {
    MigrationAuditReport {
        id: "invalid-config".into(),
        config_path: CONFIG_PATH,
        provider: "wcode-migration-audit",
        precision: "deterministic",
        revision: revision.clone(),
        passed: false,
        scanned_files: 0,
        scanned_bytes: 0,
        truncated: false,
        findings_truncated: false,
        required_paths: 0,
        forbidden_paths: 0,
        transitions: Vec::new(),
        findings: vec![MigrationAuditFinding {
            code: "invalid_config".into(),
            rule_id: None,
            message,
            locations: Vec::new(),
        }],
        summary: "Migration audit configuration is invalid; behavioral verification is blocked until the audit policy is valid.".into(),
        elapsed_ms,
    }
}

fn transition_applies(path: &str, transition: &MigrationTransition, scopes: &[String]) -> bool {
    let paths = if transition.paths.is_empty() {
        scopes
    } else {
        transition.paths.as_slice()
    };
    paths.iter().any(|scope| path_in_scope(path, scope))
}

fn excluded_path(path: &str, exclusions: &[String]) -> bool {
    exclusions.iter().any(|scope| path_in_scope(path, scope))
}

fn path_in_scope(path: &str, scope: &str) -> bool {
    let scope = scope.trim().trim_end_matches('/');
    scope.is_empty()
        || scope == "."
        || path == scope
        || path
            .strip_prefix(scope)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn line_number(content: &str, byte_index: usize) -> usize {
    content.as_bytes()[..byte_index]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        .saturating_add(1)
}

fn push_finding(
    findings: &mut Vec<MigrationAuditFinding>,
    truncated: &mut bool,
    finding: MigrationAuditFinding,
) {
    if findings.len() < MAX_FINDINGS {
        findings.push(finding);
    } else {
        *truncated = true;
    }
}

fn valid_id(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 160 && !value.contains(['\n', '\r', '\0'])
}

fn valid_path_text(value: &str) -> bool {
    let value = value.trim();
    let path = Path::new(value);
    !value.is_empty()
        && value.len() <= 512
        && !value.contains(['\n', '\r', '\0'])
        && value != CONFIG_PATH
        && value != ".wcode"
        && !value.starts_with(".wcode/")
        && !path.is_absolute()
        && !path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
}

fn valid_pattern(value: &str) -> bool {
    !value.is_empty() && value.chars().count() <= MAX_PATTERN_CHARS && !value.contains('\0')
}

fn config_present(workspace: &Workspace) -> bool {
    std::fs::symlink_metadata(workspace.root().join(CONFIG_PATH)).is_ok()
}

const fn schema_version() -> u32 {
    1
}

#[cfg(test)]
#[path = "../../tests/unit/verification/migration.rs"]
mod tests;
