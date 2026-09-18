use crate::graph::NodeId;
use crate::workspace::{SearchMode, Workspace};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub type RiskId = String;

#[derive(Clone, Debug, Serialize)]
pub struct BugPatternFinding {
    pub pattern: String,
    pub summary: String,
    pub confidence: &'static str,
    pub path: String,
    pub line: u64,
    pub text: String,
}

#[derive(Clone, Debug, Serialize)]
pub struct BugPatternStatus {
    pub precision: &'static str,
    pub patterns_scanned: usize,
    pub matches: usize,
    pub files: usize,
    pub findings: Vec<BugPatternFinding>,
    pub truncated: bool,
}

const BUG_PATTERNS: &[(&str, &str, &str, &str)] = &[
    (
        "deref-call-result",
        r"\*\s*(?:[A-Za-z_][A-Za-z0-9_]*\.)*[A-Za-z_][A-Za-z0-9_]*\([^;\n]*\)\.[A-Za-z_][A-Za-z0-9_]*",
        "A call result is dereferenced and a field/method is accessed in the same expression; verify the call cannot return nil/null before dereference.",
        "medium",
    ),
    (
        "json-decode-immediate-deref",
        r"json_decode\s*\([^;\n]*\)\s*(?:->|\[)",
        "json_decode is immediately dereferenced/indexed; verify decode failure/null is checked before access.",
        "medium",
    ),
];

pub(crate) fn scan_bug_patterns(workspace: &Workspace) -> Result<BugPatternStatus> {
    const LIMIT: usize = 128;
    let patterns = BUG_PATTERNS
        .iter()
        .map(|(_, pattern, _, _)| (*pattern).to_owned())
        .collect::<Vec<_>>();
    let matches =
        workspace.search_many_with_options(&patterns, ".", LIMIT, SearchMode::Regex, 0)?;
    let mut files = BTreeSet::new();
    let mut findings = Vec::with_capacity(matches.len());
    for item in &matches {
        let Some(pattern) = item.get("query").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let Some((name, _, summary, confidence)) = BUG_PATTERNS
            .iter()
            .find(|(_, candidate, _, _)| *candidate == pattern)
        else {
            continue;
        };
        let path = item
            .get("path")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .to_owned();
        files.insert(path.clone());
        findings.push(BugPatternFinding {
            pattern: (*name).to_owned(),
            summary: (*summary).to_owned(),
            confidence,
            path,
            line: item
                .get("line")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default(),
            text: item
                .get("text")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned(),
        });
    }
    Ok(BugPatternStatus {
        precision: "heuristic-regex-candidate",
        patterns_scanned: BUG_PATTERNS.len(),
        matches: findings.len(),
        files: files.len(),
        findings,
        truncated: matches.len() >= LIMIT,
    })
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    #[default]
    Low,
    Medium,
    High,
    Critical,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskCategory {
    Security,
    Compatibility,
    Reliability,
    Performance,
    Data,
    Migration,
    Dependency,
    Build,
    Runtime,
    VerificationGap,
    Architecture,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Risk {
    pub id: RiskId,
    pub subject: NodeId,
    pub category: RiskCategory,
    pub level: RiskLevel,
    pub summary: String,
    #[serde(default)]
    pub signals: Vec<String>,
    #[serde(default)]
    pub guards: Vec<NodeId>,
}

impl Risk {
    pub fn validate(&self) -> Result<(), RiskError> {
        if self.id.trim().is_empty()
            || self.id.len() > 160
            || self.subject.trim().is_empty()
            || self.subject.len() > 512
            || self.summary.trim().is_empty()
            || self.summary.len() > 1000
            || self.signals.len() > 32
            || self.guards.len() > 32
            || self
                .signals
                .iter()
                .any(|signal| signal.trim().is_empty() || signal.len() > 500)
            || self
                .guards
                .iter()
                .any(|guard| guard.trim().is_empty() || guard.len() > 512)
        {
            return Err(RiskError::InvalidRisk);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationProfile {
    pub level: RiskLevel,
    pub deterministic_checks: Vec<String>,
    pub independent_reviewers: usize,
    pub require_property: bool,
    pub require_mutation: bool,
    pub require_fuzz: bool,
    pub require_human_approval: bool,
}

impl VerificationProfile {
    pub fn for_risk(level: RiskLevel) -> Self {
        match level {
            RiskLevel::Low => Self {
                level,
                deterministic_checks: vec!["compile".into(), "targeted-tests".into()],
                independent_reviewers: 1,
                require_property: false,
                require_mutation: false,
                require_fuzz: false,
                require_human_approval: false,
            },
            RiskLevel::Medium => Self {
                level,
                deterministic_checks: vec!["compile".into(), "unit".into(), "integration".into()],
                independent_reviewers: 2,
                require_property: true,
                require_mutation: true,
                require_fuzz: false,
                require_human_approval: false,
            },
            RiskLevel::High => Self {
                level,
                deterministic_checks: vec![
                    "compile".into(),
                    "static-analysis".into(),
                    "unit".into(),
                    "integration".into(),
                    "compatibility".into(),
                    "security".into(),
                ],
                independent_reviewers: 3,
                require_property: true,
                require_mutation: true,
                require_fuzz: true,
                require_human_approval: false,
            },
            RiskLevel::Critical => Self {
                level,
                deterministic_checks: vec![
                    "compile".into(),
                    "static-analysis".into(),
                    "unit".into(),
                    "integration".into(),
                    "compatibility".into(),
                    "security".into(),
                    "runtime-gate".into(),
                ],
                independent_reviewers: 3,
                require_property: true,
                require_mutation: true,
                require_fuzz: true,
                require_human_approval: true,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RiskError {
    InvalidRisk,
}

impl std::fmt::Display for RiskError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("risk metadata is invalid")
    }
}

impl std::error::Error for RiskError {}

#[cfg(test)]
#[path = "../../tests/unit/intelligence/risk.rs"]
mod tests;
