use crate::graph::NodeId;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub type EvidenceId = String;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize)]
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<String>,
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
            targets: Vec::new(),
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
            || self.targets.len() > 32
            || self
                .claims
                .iter()
                .chain(&self.risks)
                .any(|value| value.trim().is_empty() || value.chars().count() > 1_000)
            || self
                .targets
                .iter()
                .any(|target| !valid_text_id(target, 128))
        {
            return Err(EvidenceError::InvalidEvidence);
        }
        Ok(())
    }
}

pub(crate) fn result_severity(result: EvidenceResult) -> u8 {
    match result {
        EvidenceResult::Pass => 0,
        EvidenceResult::Inconclusive => 1,
        EvidenceResult::Disagree => 2,
        EvidenceResult::Fail => 3,
    }
}

// A later retry replaces only the identical verification scope. Ordering a
// UUID must never turn a simultaneous failure into success. This is a view of
// retained evidence, not a release gate or a mutation of the evidence store.
pub(crate) fn latest_current<'a>(
    records: impl IntoIterator<Item = &'a Evidence>,
    revision: &Revision,
) -> Vec<&'a Evidence> {
    let mut latest = std::collections::BTreeMap::new();
    for record in records {
        if record.revision != *revision
            || matches!(
                record.kind,
                EvidenceKind::HumanApproval | EvidenceKind::Reconciliation
            )
        {
            continue;
        }
        let mut targets = record
            .targets
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        targets.sort_unstable();
        targets.dedup();
        let key = (
            record.subject.as_str(),
            record.kind,
            record.producer.as_str(),
            record.model.as_deref(),
            record.confidence,
            targets,
            record.policy.as_deref(),
        );
        let current = latest.entry(key).or_insert(record);
        if (
            record.timestamp_ms,
            result_severity(record.result),
            &record.id,
        ) > (
            current.timestamp_ms,
            result_severity(current.result),
            &current.id,
        ) {
            *current = record;
        }
    }
    latest
        .iter()
        .filter_map(|(key, record)| {
            let full = match record.policy.as_deref() {
                Some("deterministic/quick/v1") => Some("deterministic/full/v1"),
                Some("acceptance/quick/v1") => Some("acceptance/full/v1"),
                _ => None,
            };
            if let Some(full) = full {
                let mut full_key = key.clone();
                full_key.6 = Some(full);
                if latest
                    .get(&full_key)
                    .is_some_and(|full| full.timestamp_ms > record.timestamp_ms)
                {
                    return None;
                }
            }
            Some(*record)
        })
        .collect()
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
#[path = "../../tests/unit/evidence/mod.rs"]
mod tests;
