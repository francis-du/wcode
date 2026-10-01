//! A native capture receipt, never constructible by imported JSON or an Agent label.
use super::acceptance::ChangeAcceptanceRecord;

/// Only the native runtime may mint this after repository, Policy and store guards.
/// The public Record is inspectable; it is not a signed remote execution attestation.
#[derive(Clone, Debug)]
pub struct NativeAcceptanceRecord {
    record: ChangeAcceptanceRecord,
}

impl NativeAcceptanceRecord {
    pub(crate) fn captured(record: ChangeAcceptanceRecord) -> Self {
        Self { record }
    }

    pub fn record(&self) -> &ChangeAcceptanceRecord {
        &self.record
    }
}

/// Capture current locally trusted state for an independent publisher.
/// This only reads repository inputs and runs bounded Git metadata probes. It
/// does not execute repository checks, import worker JSON, approve Policy or
/// authenticate a Team actor. Deploy outside the untrusted worker's OS identity.
pub async fn capture_local(
    root: impl AsRef<std::path::Path>,
    workspace_id: &str,
    base_sha: &str,
    head_sha: &str,
) -> anyhow::Result<NativeAcceptanceRecord> {
    let workspace = crate::workspace::Workspace::new_with_security(
        root,
        false,
        true,
        crate::workspace::WorkspaceSecurity::default(),
    )?;
    let harness = crate::harness::ToolHarness::new(4)?;
    harness
        .capture_acceptance(
            workspace_id,
            &workspace,
            base_sha,
            crate::verification::change::GitChangeTarget::Commit {
                revision: head_sha.into(),
            },
        )
        .await
}
