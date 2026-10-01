use crate::core_types::ExecutionGitBinding;
pub use crate::core_types::{RequiredVerificationCheck, VerificationCheckExecution};
use crate::graph::NodeId;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

pub type EvidenceId = String;

pub const MAX_RECEIPT_CHECKS: usize = 64;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationCheckReceipt {
    pub check: RequiredVerificationCheck,
    pub result: EvidenceResult,
    #[serde(default)]
    pub execution: VerificationCheckExecution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reused_from: Option<EvidenceId>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationExecutionReceipt {
    pub schema_version: u32,
    pub level: String,
    pub required_checks: Vec<RequiredVerificationCheck>,
    pub checks: Vec<VerificationCheckReceipt>,
    pub skipped_checks: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_git_binding: Option<ExecutionGitBinding>,
}

impl VerificationExecutionReceipt {
    pub(crate) fn valid(&self) -> bool {
        let unique = |ids: Vec<&str>| {
            let count = ids.len();
            ids.into_iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                == count
        };
        self.schema_version == 1
            && self
                .execution_git_binding
                .as_ref()
                .is_none_or(|binding| binding.valid())
            && matches!(self.level.as_str(), "quick" | "full")
            && !self.required_checks.is_empty()
            && self.required_checks.len() <= 32
            && self.checks.len() <= MAX_RECEIPT_CHECKS
            && self.skipped_checks.len() <= 32
            && self
                .required_checks
                .iter()
                .all(RequiredVerificationCheck::valid)
            && self.checks.iter().all(|item| {
                item.check.valid()
                    && item
                        .reused_from
                        .as_deref()
                        .is_none_or(|id| valid_text_id(id, 160))
            })
            && self.skipped_checks.iter().all(|id| valid_text_id(id, 160))
            && unique(
                self.required_checks
                    .iter()
                    .map(|item| item.id.as_str())
                    .collect(),
            )
            && unique(
                self.checks
                    .iter()
                    .map(|item| item.check.id.as_str())
                    .collect(),
            )
            && unique(self.skipped_checks.iter().map(String::as_str).collect())
            && self
                .checks
                .iter()
                .all(|item| !self.skipped_checks.contains(&item.check.id))
    }

    pub(crate) fn satisfies_level(&self, required: &str) -> bool {
        matches!(
            (self.level.as_str(), required),
            ("full", "quick" | "full") | ("quick", "quick")
        )
    }

    pub(crate) fn covers(&self, required: &[RequiredVerificationCheck]) -> bool {
        !required.is_empty()
            && required.iter().all(|binding| {
                self.required_checks.contains(binding)
                    && self.checks.iter().any(|item| {
                        &item.check == binding
                            && item.execution == VerificationCheckExecution::Executed
                    })
                    && !self.skipped_checks.contains(&binding.id)
            })
    }

    pub(crate) fn result(&self) -> EvidenceResult {
        if self.checks.iter().any(|item| {
            item.execution == VerificationCheckExecution::Executed
                && item.result == EvidenceResult::Fail
        }) {
            EvidenceResult::Fail
        } else if self.checks.iter().any(|item| {
            item.result != EvidenceResult::Pass
                || item.execution != VerificationCheckExecution::Executed
        }) || !self.skipped_checks.is_empty()
            || !self.covers(&self.required_checks)
        {
            EvidenceResult::Inconclusive
        } else {
            EvidenceResult::Pass
        }
    }
}

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

// Source authority is assigned by trusted native entry points, never by a
// producer label, confidence score or an Agent's submission fields.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceAuthority {
    #[default]
    LegacyUnknown,
    NativeVerification,
    NativeStage,
    LocalOperator,
    SelfReported,
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
    #[serde(default)]
    pub authority: EvidenceAuthority,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_receipt: Option<VerificationExecutionReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_git_binding: Option<ExecutionGitBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_policy_binding: Option<String>,
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
            authority: EvidenceAuthority::LegacyUnknown,
            policy: None,
            artifact_digest: None,
            summary: None,
            claims: Vec::new(),
            risks: Vec::new(),
            targets: Vec::new(),
            execution_receipt: None,
            execution_git_binding: None,
            execution_policy_binding: None,
            timestamp_ms: now_ms(),
        };
        evidence.validate()?;
        Ok(evidence)
    }

    // Typed v1 receipts existed before this field. Preserve only their strong
    // native verification contract; legacy Stage/approval labels never migrate.
    pub fn effective_authority(&self) -> EvidenceAuthority {
        if self.authority != EvidenceAuthority::LegacyUnknown {
            return self.authority;
        }
        let typed_native = self.confidence == Confidence::Deterministic
            && receipt_policy_family(self).is_some()
            && match self.kind {
                EvidenceKind::Verification => {
                    self.producer == "verify_project"
                        && self.subject == format!("change:{}", self.revision.code)
                }
                EvidenceKind::IntegrationTest
                    if self.producer == "deterministic-verification-mesh" =>
                {
                    self.policy
                        .as_deref()
                        .is_some_and(|policy| policy.starts_with("acceptance/"))
                }
                EvidenceKind::Compiler
                | EvidenceKind::StaticAnalysis
                | EvidenceKind::UnitTest
                | EvidenceKind::IntegrationTest
                | EvidenceKind::EndToEndTest
                | EvidenceKind::ContractTest
                | EvidenceKind::Benchmark => {
                    self.policy
                        .as_deref()
                        .is_some_and(|policy| policy.starts_with("deterministic/"))
                        && self.execution_receipt.as_ref().is_some_and(|receipt| {
                            receipt.checks.len() == 1
                                && self.subject
                                    == format!("verification:{}", receipt.checks[0].check.id)
                        })
                }
                _ => false,
            };
        if typed_native {
            EvidenceAuthority::NativeVerification
        } else {
            EvidenceAuthority::LegacyUnknown
        }
    }

    pub fn validate(&self) -> Result<(), EvidenceError> {
        if self
            .execution_policy_binding
            .as_ref()
            .is_some_and(|binding| {
                !valid_text_id(binding, 256) || binding.chars().any(char::is_control)
            })
            || self
                .execution_git_binding
                .as_ref()
                .is_some_and(|binding| !binding.valid())
            || self.execution_git_binding.as_ref().is_some_and(|binding| {
                self.execution_receipt
                    .as_ref()
                    .and_then(|receipt| receipt.execution_git_binding.as_ref())
                    .is_some_and(|receipt_binding| receipt_binding != binding)
            })
            || self
                .execution_receipt
                .as_ref()
                .is_some_and(|receipt| !receipt.valid())
            || !valid_text_id(&self.id, 160)
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
        let receipt_scope = record.execution_receipt.as_ref().map(|receipt| {
            let mut required = receipt.required_checks.iter().collect::<Vec<_>>();
            required.sort_unstable();
            (
                receipt_policy_family(record).is_some(),
                receipt.schema_version,
                receipt.level.as_str(),
                required,
            )
        });
        let key = (
            record.subject.as_str(),
            record.kind,
            record.producer.as_str(),
            record.model.as_deref(),
            record.confidence,
            targets,
            record.policy.as_deref(),
            receipt_scope,
            record.effective_authority(),
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
            if let Some(family) = receipt_policy_family(record) {
                let receipt = record.execution_receipt.as_ref().expect("typed receipt");
                let superseded = latest.iter().any(|(new_key, newer)| {
                    new_key.0 == key.0
                        && new_key.1 == key.1
                        && new_key.2 == key.2
                        && new_key.3 == key.3
                        && new_key.4 == key.4
                        && new_key.5 == key.5
                        && new_key.8 == key.8
                        && newer.timestamp_ms > record.timestamp_ms
                        && receipt_policy_family(newer) == Some(family)
                        && newer.execution_receipt.as_ref().is_some_and(|stronger| {
                            stronger.satisfies_level(&receipt.level)
                                && stronger.covers(&receipt.required_checks)
                        })
                });
                if superseded {
                    return None;
                }
            } else if record.execution_receipt.is_none() {
                // Legacy records remain readable. Their policy-only relation
                // cannot substitute for a typed v2 execution receipt.
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
            }
            Some(*record)
        })
        .collect()
}

// Acceptance outcomes can be inconclusive despite a passing command receipt:
// a declared Test still needs its own event. Deterministic aggregate outcomes,
// unlike acceptance outcomes, must agree with the receipt.
fn receipt_policy_family(record: &Evidence) -> Option<&'static str> {
    let receipt = record
        .execution_receipt
        .as_ref()
        .filter(|receipt| receipt.valid())?;
    let policy = record.policy.as_deref()?;
    if policy == format!("deterministic/{}/v2", receipt.level) && record.result == receipt.result()
    {
        Some("deterministic")
    } else if policy == format!("acceptance/{}/v2", receipt.level) {
        Some("acceptance")
    } else {
        None
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
#[path = "../../tests/unit/evidence/mod.rs"]
mod tests;
