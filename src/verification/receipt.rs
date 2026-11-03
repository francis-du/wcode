//! Trusted native import of a captured authoritative CI receipt.
//!
//! The adapter is deliberately observation-only: an imported receipt is never
//! an execution receipt, never satisfies a verification stage and never
//! reaches Acceptance. Every binding, digest or bound mismatch fails closed
//! and persists nothing.

use crate::core_types::ExecutionGitBinding;
use crate::evidence::{
    Confidence, Evidence, EvidenceAuthority, EvidenceKind, EvidenceResult, ExternalReceipt,
    Revision,
};
use crate::git_provider::{CheckReceipt, CHECK_RECEIPT_SCHEMA_VERSION};
use anyhow::{bail, Result};
use uuid::Uuid;

pub const CI_RECEIPT_POLICY: &str = "ci-receipt/observation/v1";
const GITHUB_PROVIDER: &str = "github";

/// The operator-declared repository, App and check a receipt must match.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceiptBinding {
    pub repository: String,
    pub repository_id: u64,
    pub app_id: u64,
    pub check_name: String,
}

impl ReceiptBinding {
    pub fn validate(&self) -> Result<()> {
        if self.repository.trim().is_empty()
            || self.repository.len() > 256
            || self.repository_id == 0
            || self.app_id == 0
            || self.check_name.trim().is_empty()
            || self.check_name.len() > 256
        {
            bail!("CI receipt binding is invalid");
        }
        Ok(())
    }
}

/// Import one captured check-run receipt as bounded observation-only Evidence.
/// Nothing is persisted here; the caller persists only on success.
pub fn import_check_receipt(
    receipt: &CheckReceipt,
    binding: &ReceiptBinding,
    git: &ExecutionGitBinding,
    revision: &Revision,
) -> Result<Evidence> {
    binding.validate()?;
    if !git.valid() {
        bail!("CI receipt import requires a valid Git execution binding");
    }
    if receipt.schema_version != CHECK_RECEIPT_SCHEMA_VERSION {
        bail!("unsupported CI receipt schema");
    }
    let (provider, repository, repository_id, app_id) = receipt
        .repository_binding()
        .ok_or_else(|| anyhow::anyhow!("CI receipt has no complete repository binding"))?;
    if provider != GITHUB_PROVIDER
        || repository != binding.repository
        || repository_id != binding.repository_id
        || app_id != binding.app_id
        || receipt.check.name != binding.check_name
    {
        bail!("CI receipt does not match the declared repository binding");
    }
    if receipt.check.head_sha != git.head_sha {
        bail!("CI receipt head SHA does not match the current Git execution binding");
    }
    if !receipt.verify_digest() {
        bail!("CI receipt digest does not match its captured contents");
    }
    let result = match (
        receipt.check.status.as_str(),
        receipt.check.conclusion.as_deref(),
    ) {
        ("completed", Some("success")) => EvidenceResult::Pass,
        ("completed", _) => EvidenceResult::Fail,
        _ => EvidenceResult::Inconclusive,
    };
    let producer = format!("ci-receipt:{provider}:{repository}");
    if producer.len() > 256 {
        bail!("CI receipt producer identity exceeds its bound");
    }
    let mut evidence = Evidence::new(
        format!("EV-{}", Uuid::new_v4().simple()),
        format!("change:{}", revision.code),
        EvidenceKind::CiReceipt,
        producer,
        revision.clone(),
        result,
        Confidence::Medium,
    )?;
    let external = ExternalReceipt {
        schema_version: receipt.schema_version,
        provider: receipt.provider.clone(),
        repository: receipt.repository.clone(),
        repository_id: receipt.repository_id,
        app_id: receipt.app_id,
        check_id: receipt.check.id,
        check_name: receipt.check.name.clone(),
        head_sha: receipt.check.head_sha.clone(),
        status: receipt.check.status.clone(),
        conclusion: receipt.check.conclusion.clone(),
        annotations: receipt.annotations.clone(),
        digest: receipt.digest.clone(),
        captured_at_ms: evidence.timestamp_ms,
    };
    external
        .validate()
        .map_err(|_| anyhow::anyhow!("CI receipt payload exceeds its bounds"))?;
    evidence.authority = EvidenceAuthority::ExternalReceipt;
    evidence.execution_git_binding = Some(git.clone());
    evidence.execution_policy_binding = Some(CI_RECEIPT_POLICY.to_owned());
    evidence.policy = Some(CI_RECEIPT_POLICY.to_owned());
    evidence.summary = Some(format!(
        "CI receipt {}:{} {} {}",
        external.provider, external.repository, external.check_name, external.status
    ));
    evidence.external_receipt = Some(external);
    evidence.validate()?;
    Ok(evidence)
}

#[cfg(test)]
#[path = "../../tests/unit/verification/receipt.rs"]
mod tests;
