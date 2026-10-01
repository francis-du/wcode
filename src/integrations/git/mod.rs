//! Provider-neutral merge-gate metadata. Only an opaque native acceptance
//! capture can create a success publication; serialized CAR JSON cannot.
use crate::verification::acceptance::{AcceptanceState, ChangeAcceptanceRecord};
use crate::verification::acceptance_native::NativeAcceptanceRecord;
use crate::verification::change::{full_oid, GitChangeTarget};
use anyhow::{bail, Result};
use serde::Serialize;
use std::future::Future;
use std::pin::Pin;

pub mod enrollment;
pub mod github;
pub use enrollment::{GitHubEnrollment, GITHUB_ENROLLMENT_FILE};

pub type ProviderFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T>> + Send + 'a>>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderRepository {
    namespace: String,
    name: String,
}

impl ProviderRepository {
    pub fn new(namespace: &str, name: &str) -> Result<Self> {
        if !safe_name(namespace) || !safe_name(name) {
            bail!("invalid provider repository identity");
        }
        Ok(Self {
            namespace: namespace.to_ascii_lowercase(),
            name: name.to_ascii_lowercase(),
        })
    }
    pub fn namespace(&self) -> &str {
        &self.namespace
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn key(&self) -> String {
        format!("{}/{}", self.namespace, self.name)
    }
}

fn safe_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 100
        && value != "."
        && value != ".."
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ProviderTarget {
    pub repository: ProviderRepository,
    pub change: u64,
    pub base_sha: String,
    pub head_sha: String,
}

impl ProviderTarget {
    pub fn validate(&self) -> Result<()> {
        if self.change == 0
            || !full_oid(&self.base_sha)
            || !full_oid(&self.head_sha)
            || self.base_sha.len() != self.head_sha.len()
        {
            bail!("provider target requires complete matching commit identities");
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GateVerdict {
    Ready,
    Blocked,
}

/// Public serialization is an observation, never an imported authority.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CheckObservation {
    pub id: u64,
    pub head_sha: String,
    pub name: String,
    pub source_id: String,
    pub external_id: String,
    pub status: String,
    pub conclusion: Option<String>,
}

impl CheckObservation {
    pub fn strictly_successful(&self) -> bool {
        self.status == "completed" && self.conclusion.as_deref() == Some("success")
    }
}

/// No Deserialize or public success constructor. The caller must obtain a new
/// NativeAcceptanceRecord from the trusted native runtime, not an artifact.
///
/// ```compile_fail
/// let imported: wcode::verification::acceptance_native::NativeAcceptanceRecord =
///     serde_json::from_str(r#"{\"ready\":true}"#).unwrap();
/// ```
#[derive(Debug, Eq, PartialEq)]
pub struct NativePublication {
    target: ProviderTarget,
    verdict: GateVerdict,
    record_digest: Option<String>,
}

impl NativePublication {
    pub fn target(&self) -> &ProviderTarget {
        &self.target
    }
    pub fn verdict(&self) -> GateVerdict {
        self.verdict
    }
    pub fn record_digest(&self) -> Option<&str> {
        self.record_digest.as_deref()
    }
    pub(crate) fn unavailable(target: ProviderTarget) -> Result<Self> {
        target.validate()?;
        Ok(Self {
            target,
            verdict: GateVerdict::Blocked,
            record_digest: None,
        })
    }
    pub(crate) fn from_native(
        native: &NativeAcceptanceRecord,
        target: ProviderTarget,
    ) -> Result<Self> {
        target.validate()?;
        let record = native.record();
        let ready = record.state == AcceptanceState::Ready
            && !record.partial
            && exact_candidate(record, &target);
        Ok(Self {
            target,
            verdict: if ready {
                GateVerdict::Ready
            } else {
                GateVerdict::Blocked
            },
            record_digest: Some(record.digest().to_owned()),
        })
    }
}

fn exact_candidate(record: &ChangeAcceptanceRecord, target: &ProviderTarget) -> bool {
    let git = &record.git;
    git.complete
        && matches!(git.target, GitChangeTarget::Commit { .. })
        && git.base_sha.as_deref() == Some(target.base_sha.as_str())
        && git.target_sha.as_deref() == Some(target.head_sha.as_str())
        && git.binding.as_ref().is_some_and(|binding| {
            binding.valid() && !binding.dirty && binding.head_sha == target.head_sha
        })
        && record.policy.is_some()
}

pub trait GitProvider {
    fn observe<'a>(&'a self, change: u64) -> ProviderFuture<'a, ProviderTarget>;
    fn write_check<'a>(
        &'a self,
        publication: &'a NativePublication,
    ) -> ProviderFuture<'a, CheckObservation>;
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/git/contract.rs"]
mod tests;
