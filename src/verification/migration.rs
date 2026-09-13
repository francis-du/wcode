use crate::evidence::Revision;
use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};
use std::time::Instant;

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
    pub transitions: Vec<MigrationTransitionResult>,
    pub findings: Vec<MigrationAuditFinding>,
    pub summary: String,
    pub elapsed_ms: u128,
}

#[derive(Default)]
struct TransitionAccumulator {
    legacy_matches: usize,
    replacement_matches: usize,
    legacy_locations: Vec<MigrationAuditLocation>,
}

pub(crate) fn audit(
    workspace: &Workspace,
    revision: &Revision,
) -> Result<Option<MigrationAuditReport>> {
    let started = Instant::now();
    let spec = match load_spec(workspace) {
        Ok(Some(spec)) => spec,
        Ok(None) => return Ok(None),
        Err(error) => {
            return Ok(Some(invalid_report(
                revision,
                error.to_string(),
                started.elapsed().as_millis(),
            )));
        }
    };

    let mut findings = Vec::new();
    let mut findings_truncated = false;
    for path in &spec.required_paths {
        if workspace.path_info(path).is_err() {
            push_finding(
                &mut findings,
                &mut findings_truncated,
                MigrationAuditFinding {
                    code: "required_path_missing".into(),
                    rule_id: None,
                    message: format!("required migration path is missing or inaccessible: {path}"),
                    locations: Vec::new(),
                },
            );
        }
    }

    let mut roots = BTreeSet::new();
    for transition in &spec.transitions {
        let paths = if transition.paths.is_empty() {
            &spec.scopes
        } else {
            &transition.paths
        };
        roots.extend(paths.iter().cloned());
    }

    let mut files = BTreeSet::new();
    let mut truncated = false;
    for root in roots {
        match workspace.source_files(&root, MAX_AUDIT_FILES) {
            Ok((scoped, scoped_truncated)) => {
                truncated |= scoped_truncated;
                for path in scoped {
                    if path == CONFIG_PATH
                        || path.starts_with(".wcode/")
                        || excluded_path(&path, &spec.exclude_paths)
                    {
                        continue;
                    }
                    files.insert(path);
                    if files.len() > MAX_AUDIT_FILES {
                        truncated = true;
                        break;
                    }
                }
            }
            Err(error) => {
                push_finding(
                    &mut findings,
                    &mut findings_truncated,
                    MigrationAuditFinding {
                        code: "scope_unavailable".into(),
                        rule_id: None,
                        message: format!("migration audit scope `{root}` is unavailable: {error}"),
                        locations: Vec::new(),
                    },
                );
            }
        }
        if truncated {
            break;
        }
    }
    if truncated {
        push_finding(
            &mut findings,
            &mut findings_truncated,
            MigrationAuditFinding {
                code: "scan_truncated".into(),
                rule_id: None,
                message: format!(
                    "migration audit exceeded its {MAX_AUDIT_FILES}-file scan bound; completeness cannot be proven"
                ),
                locations: Vec::new(),
            },
        );
    }

    let mut accumulators = spec
        .transitions
        .iter()
        .map(|transition| (transition.id.clone(), TransitionAccumulator::default()))
        .collect::<BTreeMap<_, _>>();
    let mut scanned_files = 0usize;
    let mut scanned_bytes = 0usize;
    if !truncated {
        for path in files.iter().take(MAX_AUDIT_FILES) {
            let source = match workspace.load_source(path) {
                Ok(source) => source,
                Err(error) => {
                    push_finding(
                        &mut findings,
                        &mut findings_truncated,
                        MigrationAuditFinding {
                            code: "target_unreadable".into(),
                            rule_id: None,
                            message: format!(
                                "migration target `{path}` cannot be audited: {error}"
                            ),
                            locations: Vec::new(),
                        },
                    );
                    continue;
                }
            };
            if scanned_bytes.saturating_add(source.content.len()) > MAX_AUDIT_BYTES {
                truncated = true;
                push_finding(
                    &mut findings,
                    &mut findings_truncated,
                    MigrationAuditFinding {
                        code: "scan_truncated".into(),
                        rule_id: None,
                        message: format!(
                            "migration audit exceeded its {MAX_AUDIT_BYTES}-byte scan bound; completeness cannot be proven"
                        ),
                        locations: Vec::new(),
                    },
                );
                break;
            }
            scanned_files = scanned_files.saturating_add(1);
            scanned_bytes = scanned_bytes.saturating_add(source.content.len());
            for transition in &spec.transitions {
                if !transition_applies(&source.path, transition, &spec.scopes) {
                    continue;
                }
                let accumulator = accumulators
                    .get_mut(&transition.id)
                    .expect("validated transition accumulator");
                if let Some(pattern) = transition.legacy.as_deref() {
                    for (index, _) in source.content.match_indices(pattern) {
                        accumulator.legacy_matches = accumulator.legacy_matches.saturating_add(1);
                        if accumulator.legacy_locations.len() < MAX_SAMPLE_LOCATIONS {
                            accumulator.legacy_locations.push(MigrationAuditLocation {
                                path: source.path.clone(),
                                line: line_number(&source.content, index),
                            });
                        }
                    }
                }
                if let Some(pattern) = transition.replacement.as_deref() {
                    accumulator.replacement_matches = accumulator
                        .replacement_matches
                        .saturating_add(source.content.match_indices(pattern).count());
                }
            }
        }
    }

    let mut transitions = Vec::with_capacity(spec.transitions.len());
    for transition in &spec.transitions {
        let accumulator = accumulators
            .remove(&transition.id)
            .expect("validated transition accumulator");
        let min_replacement_matches = transition
            .replacement
            .as_ref()
            .map(|_| transition.min_replacement_matches.max(1))
            .unwrap_or(0);
        let mixed_state = accumulator.legacy_matches > 0 && accumulator.replacement_matches > 0;
        if accumulator.legacy_matches > 0 {
            push_finding(
                &mut findings,
                &mut findings_truncated,
                MigrationAuditFinding {
                    code: "legacy_remaining".into(),
                    rule_id: Some(transition.id.clone()),
                    message: format!(
                        "{} legacy occurrence(s) remain for migration rule `{}`",
                        accumulator.legacy_matches, transition.id
                    ),
                    locations: accumulator.legacy_locations,
                },
            );
        }
        if transition.replacement.is_some()
            && accumulator.replacement_matches < min_replacement_matches
        {
            push_finding(
                &mut findings,
                &mut findings_truncated,
                MigrationAuditFinding {
                    code: "replacement_missing".into(),
                    rule_id: Some(transition.id.clone()),
                    message: format!(
                        "migration rule `{}` found {} replacement occurrence(s), below required minimum {}",
                        transition.id,
                        accumulator.replacement_matches,
                        min_replacement_matches
                    ),
                    locations: Vec::new(),
                },
            );
        }
        if mixed_state {
            push_finding(
                &mut findings,
                &mut findings_truncated,
                MigrationAuditFinding {
                    code: "mixed_state".into(),
                    rule_id: Some(transition.id.clone()),
                    message: format!(
                        "migration rule `{}` contains both legacy and replacement patterns; the repository is only partially migrated",
                        transition.id
                    ),
                    locations: Vec::new(),
                },
            );
        }
        transitions.push(MigrationTransitionResult {
            id: transition.id.clone(),
            legacy_matches: accumulator.legacy_matches,
            replacement_matches: accumulator.replacement_matches,
            min_replacement_matches,
            mixed_state,
        });
    }

    let passed = findings.is_empty() && !truncated;
    let summary = if passed {
        format!(
            "Migration audit `{}` passed: {} transition(s), {} required path(s), {scanned_files} file(s) inspected.",
            spec.id,
            spec.transitions.len(),
            spec.required_paths.len()
        )
    } else {
        format!(
            "Migration audit `{}` failed with {} retained finding(s){}; behavioral verification must not be treated as proof of migration completeness.",
            spec.id,
            findings.len(),
            if findings_truncated { " (details truncated)" } else { "" }
        )
    };
    Ok(Some(MigrationAuditReport {
        id: spec.id,
        config_path: CONFIG_PATH,
        provider: "wcode-migration-audit",
        precision: "deterministic",
        revision: revision.clone(),
        passed,
        scanned_files,
        scanned_bytes,
        truncated,
        findings_truncated,
        required_paths: spec.required_paths.len(),
        transitions,
        findings,
        summary,
        elapsed_ms: started.elapsed().as_millis(),
    }))
}

pub(crate) fn context_summary(workspace: &Workspace) -> Option<Value> {
    if !config_present(workspace) {
        return None;
    }
    Some(match load_spec(workspace) {
        Ok(Some(spec)) => json!({
            "configured": true,
            "valid": true,
            "id": spec.id,
            "config_path": CONFIG_PATH,
            "provider": "wcode-migration-audit",
            "precision": "deterministic",
            "gate_order": "before_behavioral_verification",
            "scopes": spec.scopes,
            "transitions": spec.transitions.len(),
            "required_paths": spec.required_paths.len(),
        }),
        Ok(None) => return None,
        Err(error) => json!({
            "configured": true,
            "valid": false,
            "config_path": CONFIG_PATH,
            "provider": "wcode-migration-audit",
            "precision": "deterministic",
            "error": error.to_string(),
        }),
    })
}

pub(crate) fn capabilities() -> Value {
    json!({
        "config": CONFIG_PATH,
        "schema_version": 1,
        "provider": "wcode-migration-audit",
        "precision": "deterministic",
        "matching": "literal-substring",
        "gate_order": "before_behavioral_verification",
        "max_transitions": MAX_TRANSITIONS,
        "max_paths": MAX_PATHS,
        "max_scan_files": MAX_AUDIT_FILES,
        "max_scan_bytes": MAX_AUDIT_BYTES,
        "no_shell": true,
    })
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
        || spec.transitions.len() > MAX_TRANSITIONS
        || (spec.transitions.is_empty() && spec.required_paths.is_empty())
        || spec
            .scopes
            .iter()
            .chain(&spec.exclude_paths)
            .chain(&spec.required_paths)
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
