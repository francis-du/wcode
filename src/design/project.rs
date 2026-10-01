use super::DesignState;
use crate::risk::RiskLevel;
use crate::verification::{ReviewerRole, VerificationStage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectDesign {
    #[serde(default = "super::schema_version")]
    pub schema_version: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceptance_policy: Option<AcceptancePolicy>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyLevel {
    Quick,
    #[default]
    Full,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyRequirements {
    #[serde(default)]
    pub minimum_level: PolicyLevel,
    #[serde(default)]
    pub checks: Vec<String>,
    #[serde(default)]
    pub stages: Vec<VerificationStage>,
    #[serde(default)]
    pub reviewers: Vec<ReviewerRole>,
    #[serde(default)]
    pub human_approval: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub human_approval_min_risk: Option<RiskLevel>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PolicyPath {
    File { path: String },
    Directory { path: String },
}

impl PolicyPath {
    pub fn path(&self) -> &str {
        match self {
            Self::File { path } | Self::Directory { path } => path,
        }
    }

    fn matches(&self, candidate: &str) -> bool {
        match self {
            Self::File { path } => candidate == path,
            Self::Directory { path } => candidate
                .strip_prefix(path)
                .is_some_and(|tail| tail.starts_with('/')),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PolicySelector {
    #[serde(default)]
    pub paths: Vec<PolicyPath>,
    #[serde(default)]
    pub components: Vec<String>,
    #[serde(default)]
    pub requirements: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptanceRule {
    pub id: String,
    pub when: PolicySelector,
    pub require: PolicyRequirements,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct DocsOnlyPolicy {
    pub paths: Vec<PolicyPath>,
    pub require: PolicyRequirements,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptancePolicy {
    pub schema_version: u32,
    pub id: String,
    pub version: u64,
    #[serde(rename = "default")]
    pub requirements: PolicyRequirements,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs_only: Option<DocsOnlyPolicy>,
    #[serde(default)]
    pub rules: Vec<AcceptanceRule>,
}

/// Native change capture must include both rename paths and exact mode knowledge.
/// This is a pure matching input, never an authorization or an executed result.
pub struct PolicyChangeSet {
    pub paths: Vec<String>,
    pub complete: bool,
    pub executable_changes: Option<bool>,
    pub regular_file_changes: Option<bool>,
}

/// File scopes projected from an approved Design snapshot. Candidate remapping
/// must never replace these inputs when a trusted policy is evaluated.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyPathMappings {
    pub complete: bool,
    pub components: BTreeMap<String, Vec<String>>,
    pub requirements: BTreeMap<String, Vec<String>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PolicySelection {
    pub policy_id: String,
    pub policy_version: u64,
    pub policy_digest: String,
    pub docs_only: bool,
    pub matched_rules: Vec<String>,
    pub requirements: PolicyRequirements,
}

impl AcceptancePolicy {
    pub fn digest(&self) -> String {
        let bytes = serde_json::to_vec(self).expect("typed policy is JSON serializable");
        format!("sha256:{:x}", Sha256::digest(bytes))
    }

    pub fn validate(&self, design: &DesignState) -> Vec<String> {
        let mut errors = self.structure_errors();
        if !errors.is_empty() {
            return errors;
        }
        for rule in &self.rules {
            if rule
                .when
                .components
                .iter()
                .any(|id| !design.components.contains_key(id))
            {
                errors.push("unknown-policy-component".into());
            }
            if rule
                .when
                .requirements
                .iter()
                .any(|id| !design.requirements.contains_key(id))
            {
                errors.push("unknown-policy-requirement".into());
            }
        }
        bounded_errors(errors)
    }

    /// Produces requirements only. It does not activate the draft, accept a
    /// change, execute a check or authenticate the supplied mapping source.
    pub fn select(
        &self,
        change: &PolicyChangeSet,
        mappings: &PolicyPathMappings,
        risk_floor: &PolicyRequirements,
        risk: RiskLevel,
    ) -> Result<PolicySelection, Vec<String>> {
        let mut errors = self.structure_errors();
        validate_requirements(risk_floor, false, &mut errors);
        if !errors.is_empty() {
            return Err(bounded_errors(errors));
        }
        if !change.complete || change.paths.len() > 4096 {
            errors.push("policy-change-set-incomplete".into());
        }
        if change.paths.len() <= 4096 && change.paths.iter().any(|path| !valid_path(path)) {
            errors.push("invalid-policy-change-path".into());
        }
        if !errors.is_empty() {
            return Err(bounded_errors(errors));
        }
        let component_ids = self
            .rules
            .iter()
            .flat_map(|rule| rule.when.components.iter())
            .collect::<BTreeSet<_>>();
        let requirement_ids = self
            .rules
            .iter()
            .flat_map(|rule| rule.when.requirements.iter())
            .collect::<BTreeSet<_>>();
        let mapped_count = component_ids
            .iter()
            .filter_map(|id| mappings.components.get(*id))
            .map(Vec::len)
            .chain(
                requirement_ids
                    .iter()
                    .filter_map(|id| mappings.requirements.get(*id))
                    .map(Vec::len),
            )
            .fold(0usize, usize::saturating_add);
        if mapped_count > 4096 {
            return Err(vec!["policy-design-mapping-bound".into()]);
        }
        for rule in &self.rules {
            validate_mapping_inputs(
                &rule.when.components,
                &mappings.components,
                mappings.complete,
                &mut errors,
            );
            validate_mapping_inputs(
                &rule.when.requirements,
                &mappings.requirements,
                mappings.complete,
                &mut errors,
            );
        }
        if !errors.is_empty() {
            return Err(bounded_errors(errors));
        }
        let docs_only = self.docs_only.as_ref().is_some_and(|docs| {
            !change.paths.is_empty()
                && change.executable_changes == Some(false)
                && change.regular_file_changes == Some(true)
                && change.paths.iter().all(|path| {
                    pure_markdown_path(path)
                        && !instruction_markdown_path(path)
                        && docs.paths.iter().any(|scope| scope.matches(path))
                })
        });
        let mut requirements = if docs_only {
            self.docs_only
                .as_ref()
                .expect("matched docs rule")
                .require
                .clone()
        } else {
            self.requirements.clone()
        };
        let mut matched_rules = Vec::new();
        let changed = change
            .paths
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        for rule in &self.rules {
            let matches = change
                .paths
                .iter()
                .any(|path| rule.when.paths.iter().any(|scope| scope.matches(path)))
                || mapped_change_matches(&rule.when.components, &mappings.components, &changed)
                || mapped_change_matches(&rule.when.requirements, &mappings.requirements, &changed);
            if matches {
                requirements.merge(&rule.require, risk);
                matched_rules.push(rule.id.clone());
            }
        }
        requirements.merge(risk_floor, risk);
        // Apply the baseline threshold even if no custom rule matched.
        requirements.human_approval |= requirements
            .human_approval_min_risk
            .is_some_and(|threshold| risk >= threshold);
        requirements.human_approval_min_risk = None;
        requirements.normalize();
        validate_requirements(&requirements, true, &mut errors);
        if !errors.is_empty() {
            return Err(bounded_errors(errors));
        }
        matched_rules.sort();
        Ok(PolicySelection {
            policy_id: self.id.clone(),
            policy_version: self.version,
            policy_digest: self.digest(),
            docs_only,
            matched_rules,
            requirements,
        })
    }

    fn structure_errors(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.schema_version != 1 || self.version == 0 || !valid_id(&self.id) {
            errors.push("invalid-policy-identity".into());
        }
        validate_requirements(&self.requirements, true, &mut errors);
        if self.rules.len() > 32 {
            errors.push("policy-rule-bound".into());
            return bounded_errors(errors);
        }
        let mut ids = BTreeSet::new();
        for rule in &self.rules {
            if !valid_id(&rule.id) || !ids.insert(rule.id.as_str()) {
                errors.push("invalid-or-duplicate-policy-rule".into());
            }
            let selector = &rule.when;
            if selector.paths.is_empty()
                && selector.components.is_empty()
                && selector.requirements.is_empty()
            {
                errors.push("empty-policy-selector".into());
            }
            validate_paths(&selector.paths, &mut errors);
            validate_ids(&selector.components, "policy-components", &mut errors);
            validate_ids(&selector.requirements, "policy-requirements", &mut errors);
            validate_requirements(&rule.require, false, &mut errors);
        }
        if let Some(docs) = &self.docs_only {
            validate_paths(&docs.paths, &mut errors);
            if docs.paths.is_empty()
                || docs.paths.iter().any(|scope| {
                    !safe_document_scope(scope.path())
                        || matches!(scope, PolicyPath::File { path } if !pure_markdown_path(path))
                })
            {
                errors.push("invalid-docs-only-scope".into());
            }
            validate_requirements(&docs.require, true, &mut errors);
        }
        bounded_errors(errors)
    }
}

impl PolicyRequirements {
    fn merge(&mut self, other: &Self, risk: RiskLevel) {
        self.minimum_level = self.minimum_level.max(other.minimum_level);
        self.checks.extend(other.checks.iter().cloned());
        self.stages.extend(other.stages.iter().copied());
        self.reviewers.extend(other.reviewers.iter().copied());
        self.human_approval |= other.human_approval
            || other
                .human_approval_min_risk
                .is_some_and(|threshold| risk >= threshold);
        self.normalize();
    }

    fn normalize(&mut self) {
        self.checks.sort();
        self.checks.dedup();
        self.stages.sort_by_key(|stage| *stage as u8);
        self.stages.dedup();
        self.reviewers.sort();
        self.reviewers.dedup();
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 160
        && value
            .chars()
            .all(|ch| !ch.is_whitespace() && !ch.is_control())
}

fn valid_path(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && !value.starts_with('/')
        && !value.contains('\\')
        && !value.contains(':')
        && !value
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, '*' | '?' | '[' | ']'))
        && value
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn safe_document_scope(value: &str) -> bool {
    valid_path(value)
        && value
            .split('/')
            .all(|part| !matches!(part, ".git" | ".github" | ".wcode"))
}

fn pure_markdown_path(value: &str) -> bool {
    safe_document_scope(value) && value.ends_with(".md")
}

// Instruction files stay on the baseline even inside an explicit docs scope.
// Other Markdown embedded in source still requires a trusted graph risk floor.
fn instruction_markdown_path(value: &str) -> bool {
    let name = value.rsplit('/').next().unwrap_or(value);
    ["AGENTS.md", "CLAUDE.md", "SKILL.md"]
        .iter()
        .any(|instruction| name.eq_ignore_ascii_case(instruction))
}

fn validate_ids(values: &[String], label: &str, errors: &mut Vec<String>) {
    if values.len() > 32
        || values.iter().any(|value| !valid_id(value))
        || values.iter().collect::<BTreeSet<_>>().len() != values.len()
    {
        errors.push(format!("invalid-{label}"));
    }
}

fn validate_paths(paths: &[PolicyPath], errors: &mut Vec<String>) {
    if paths.len() > 32 {
        errors.push("invalid-policy-paths".into());
        return;
    }
    let keys = paths
        .iter()
        .map(|path| format!("{:?}:{}", std::mem::discriminant(path), path.path()))
        .collect::<BTreeSet<_>>();
    if paths.len() > 32
        || keys.len() != paths.len()
        || paths.iter().any(|path| !valid_path(path.path()))
    {
        errors.push("invalid-policy-paths".into());
    }
}

fn validate_requirements(
    requirements: &PolicyRequirements,
    need_checks: bool,
    errors: &mut Vec<String>,
) {
    validate_ids(&requirements.checks, "policy-checks", errors);
    if need_checks && requirements.checks.is_empty() {
        errors.push("empty-required-policy-checks".into());
    }
    if requirements.stages.len() > 4
        || requirements
            .stages
            .iter()
            .enumerate()
            .any(|(i, stage)| requirements.stages[..i].contains(stage))
        || requirements.reviewers.len() > 9
        || requirements.reviewers.iter().collect::<BTreeSet<_>>().len()
            != requirements.reviewers.len()
    {
        errors.push("invalid-policy-stage-or-reviewer-requirements".into());
    }
}

fn validate_mapping_inputs(
    ids: &[String],
    mappings: &BTreeMap<String, Vec<String>>,
    complete: bool,
    errors: &mut Vec<String>,
) {
    if ids.is_empty() {
        return;
    }
    if !complete {
        errors.push("policy-design-mapping-incomplete".into());
        return;
    }
    for id in ids {
        match mappings.get(id) {
            Some(paths)
                if !paths.is_empty()
                    && paths.len() <= 4096
                    && paths.iter().all(|path| valid_path(path)) => {}
            _ => errors.push("policy-design-mapping-unavailable".into()),
        }
    }
}

fn mapped_change_matches(
    ids: &[String],
    mappings: &BTreeMap<String, Vec<String>>,
    changed: &BTreeSet<&str>,
) -> bool {
    ids.iter().any(|id| {
        mappings
            .get(id)
            .is_some_and(|paths| paths.iter().any(|scope| changed.contains(scope.as_str())))
    })
}

fn bounded_errors(errors: Vec<String>) -> Vec<String> {
    errors
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .take(64)
        .collect()
}

pub(super) fn validate_project_policy(load: &mut super::DesignLoad) {
    let Some(policy) = load
        .state
        .project
        .as_ref()
        .and_then(|project| project.acceptance_policy.as_ref())
    else {
        return;
    };
    let errors = policy.validate(&load.state);
    load.diagnostics
        .extend(errors.into_iter().map(|code| super::DesignDiagnostic {
            severity: super::DiagnosticSeverity::Error,
            path: super::PROJECT_FILE.into(),
            message: format!("Acceptance policy draft is invalid: {code}."),
            code,
        }));
}

#[cfg(test)]
#[path = "../../tests/unit/design/project.rs"]
mod tests;
