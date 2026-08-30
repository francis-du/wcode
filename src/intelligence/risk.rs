use crate::graph::NodeId;
use serde::{Deserialize, Serialize};

pub type RiskId = String;

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
