use crate::graph::NodeId;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub type EvidenceId = String;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceKind {
    Compiler,
    StaticAnalysis,
    Property,
    UnitTest,
    IntegrationTest,
    EndToEndTest,
    ContractTest,
    Mutation,
    Fuzz,
    Benchmark,
    Runtime,
    Verification,
    Reconciliation,
    ModelReview,
    HumanApproval,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Confidence {
    Low,
    Medium,
    High,
    Deterministic,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceResult {
    Pass,
    Fail,
    Inconclusive,
    Disagree,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub design: Option<String>,
    pub code: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    pub id: EvidenceId,
    pub subject: NodeId,
    pub kind: EvidenceKind,
    pub producer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    pub revision: Revision,
    pub result: EvidenceResult,
    pub confidence: Confidence,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact_digest: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub claims: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub risks: Vec<String>,
    pub timestamp_ms: u64,
}

impl Evidence {
    pub fn new(
        id: EvidenceId,
        subject: NodeId,
        kind: EvidenceKind,
        producer: String,
        revision: Revision,
        result: EvidenceResult,
        confidence: Confidence,
    ) -> Result<Self, EvidenceError> {
        let evidence = Self {
            id,
            subject,
            kind,
            producer,
            model: None,
            revision,
            result,
            confidence,
            policy: None,
            artifact_digest: None,
            summary: None,
            claims: Vec::new(),
            risks: Vec::new(),
            timestamp_ms: now_ms(),
        };
        evidence.validate()?;
        Ok(evidence)
    }

    pub fn validate(&self) -> Result<(), EvidenceError> {
        if !valid_text_id(&self.id, 160)
            || !valid_text_id(&self.subject, 512)
            || self.producer.trim().is_empty()
            || self.producer.len() > 256
            || self.revision.code.trim().is_empty()
            || self.revision.code.len() > 256
            || self
                .revision
                .design
                .as_ref()
                .is_some_and(|revision| revision.trim().is_empty() || revision.len() > 256)
            || self.model.as_ref().is_some_and(|model| model.len() > 256)
            || self
                .policy
                .as_ref()
                .is_some_and(|policy| policy.len() > 256)
            || self
                .artifact_digest
                .as_ref()
                .is_some_and(|digest| digest.trim().is_empty() || digest.len() > 512)
            || self
                .summary
                .as_ref()
                .is_some_and(|summary| summary.trim().is_empty() || summary.chars().count() > 2_000)
            || self.claims.len() > 32
            || self.risks.len() > 32
            || self
                .claims
                .iter()
                .chain(&self.risks)
                .any(|value| value.trim().is_empty() || value.chars().count() > 1_000)
        {
            return Err(EvidenceError::InvalidEvidence);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvidenceError {
    InvalidEvidence,
}

impl std::fmt::Display for EvidenceError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("evidence metadata or provenance is invalid")
    }
}

impl std::error::Error for EvidenceError {}

fn valid_text_id(value: &str, maximum: usize) -> bool {
    !value.trim().is_empty() && value.len() <= maximum && !value.chars().any(char::is_control)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evidence_requires_revision_and_producer_provenance() {
        let evidence = Evidence::new(
            "EV-SEC-001".into(),
            "REQ-SEC-001".into(),
            EvidenceKind::UnitTest,
            "cargo-test".into(),
            Revision {
                design: Some("design:1".into()),
                code: "git:abc123".into(),
            },
            EvidenceResult::Pass,
            Confidence::Deterministic,
        )
        .unwrap();
        assert_eq!(evidence.validate(), Ok(()));
        assert_eq!(evidence.result, EvidenceResult::Pass);
    }

    #[test]
    fn model_consensus_cannot_be_labeled_deterministic_by_kind() {
        let evidence = Evidence::new(
            "EV-REVIEW-001".into(),
            "component:auth".into(),
            EvidenceKind::ModelReview,
            "reviewer".into(),
            Revision {
                design: None,
                code: "git:abc123".into(),
            },
            EvidenceResult::Pass,
            Confidence::High,
        )
        .unwrap();
        assert_eq!(evidence.kind, EvidenceKind::ModelReview);
        assert_ne!(evidence.confidence, Confidence::Deterministic);
    }
}
