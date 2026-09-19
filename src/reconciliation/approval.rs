use crate::evidence::Revision;
use crate::reconcile::{ReconciliationExecution, ReconciliationPlan, ReconciliationRunStatus};
use crate::risk::RiskLevel;
use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const APPROVAL_SCHEMA_VERSION: u32 = 1;
const MAX_APPROVAL_BYTES: u64 = 768 * 1024;
const MAX_APPROVAL_SNAPSHOTS: usize = 512;
const MAX_ACCEPTANCE_REFS: usize = 256;
const MAX_VERIFICATION_REFS: usize = 2_048;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApprovedPlanSnapshot {
    pub schema_version: u32,
    pub plan_id: String,
    pub workspace: String,
    pub plan_digest: String,
    pub approved_revision: Revision,
    pub acceptance_refs: Vec<String>,
    pub verification_refs: Vec<String>,
    pub risk_level: RiskLevel,
    pub verification_policy: String,
    pub require_human_approval: bool,
    pub approved_by: String,
    pub statement_digest: String,
    pub approved_at_ms: u64,
    pub plan: ReconciliationPlan,
}

impl ApprovedPlanSnapshot {
    pub(crate) fn from_plan(
        plan: &ReconciliationPlan,
        approved_revision: Revision,
        approver: &str,
        statement: &str,
    ) -> Result<Self> {
        plan.validate()?;
        let approver = approver.trim();
        let statement = statement.trim();
        if approver.is_empty()
            || approver.len() > 256
            || statement.is_empty()
            || statement.chars().count() > 2_000
        {
            bail!("plan approval identity or statement is invalid");
        }
        if plan.verification_plan.revision.as_ref() != Some(&approved_revision) {
            bail!("replan_required: reconciliation plan is not bound to the current revision");
        }

        let mut acceptance_refs = plan.impacted_acceptance.clone();
        acceptance_refs.sort();
        acceptance_refs.dedup();
        acceptance_refs.truncate(MAX_ACCEPTANCE_REFS);
        let mut verification_refs = vec![plan.verification_plan.id.clone()];
        verification_refs.extend(plan.impacted_tests.iter().cloned());
        verification_refs.sort();
        verification_refs.dedup();
        verification_refs.truncate(MAX_VERIFICATION_REFS);

        let snapshot = Self {
            schema_version: APPROVAL_SCHEMA_VERSION,
            plan_id: plan.id.clone(),
            workspace: plan.workspace.clone(),
            plan_digest: digest_plan(plan)?,
            approved_revision,
            acceptance_refs,
            verification_refs,
            risk_level: plan.risk_level,
            verification_policy: plan.verification_plan.policy.clone(),
            require_human_approval: plan.verification_plan.require_human_approval,
            approved_by: approver.to_owned(),
            statement_digest: digest_text(statement),
            approved_at_ms: now_ms(),
            plan: plan.clone(),
        };
        snapshot.validate()?;
        Ok(snapshot)
    }

    fn validate(&self) -> Result<()> {
        if self.schema_version != APPROVAL_SCHEMA_VERSION
            || self.plan_id.trim().is_empty()
            || self.plan_id.len() > 160
            || self.workspace.trim().is_empty()
            || self.workspace.len() > 300
            || self.plan_digest.len() != 64
            || !is_hex_digest(&self.plan_digest)
            || self.approved_revision.code.trim().is_empty()
            || self.approved_revision.code.len() > 256
            || self
                .approved_revision
                .design
                .as_ref()
                .is_some_and(|revision| revision.trim().is_empty() || revision.len() > 256)
            || self.acceptance_refs.len() > MAX_ACCEPTANCE_REFS
            || self.verification_refs.len() > MAX_VERIFICATION_REFS
            || self
                .acceptance_refs
                .iter()
                .chain(&self.verification_refs)
                .any(|reference| reference.trim().is_empty() || reference.len() > 512)
            || self.verification_policy.trim().is_empty()
            || self.verification_policy.len() > 256
            || self.approved_by.trim().is_empty()
            || self.approved_by.len() > 256
            || self.statement_digest.len() != 64
            || !is_hex_digest(&self.statement_digest)
        {
            bail!("approved reconciliation plan snapshot is invalid");
        }
        self.plan.validate()?;
        if self.plan.id != self.plan_id
            || self.plan.workspace != self.workspace
            || self.plan.risk_level != self.risk_level
            || self.plan.verification_plan.policy != self.verification_policy
            || self.plan.verification_plan.require_human_approval != self.require_human_approval
            || self.plan.verification_plan.revision.as_ref() != Some(&self.approved_revision)
            || digest_plan(&self.plan)? != self.plan_digest
        {
            bail!("approved reconciliation plan snapshot binding is invalid");
        }
        let mut acceptance_refs = self.plan.impacted_acceptance.clone();
        acceptance_refs.sort();
        acceptance_refs.dedup();
        acceptance_refs.truncate(MAX_ACCEPTANCE_REFS);
        let mut verification_refs = vec![self.plan.verification_plan.id.clone()];
        verification_refs.extend(self.plan.impacted_tests.iter().cloned());
        verification_refs.sort();
        verification_refs.dedup();
        verification_refs.truncate(MAX_VERIFICATION_REFS);
        if self.acceptance_refs != acceptance_refs || self.verification_refs != verification_refs {
            bail!("approved reconciliation plan references do not match frozen plan");
        }
        Ok(())
    }
}

pub(crate) fn approve(
    workspace: &Workspace,
    plan: &ReconciliationPlan,
    current_revision: &Revision,
    approver: &str,
    statement: &str,
) -> Result<ApprovedPlanSnapshot> {
    if let Some(existing) = load(workspace, &plan.id)? {
        if existing.plan_digest != digest_plan(plan)? {
            bail!("replan_required: this reconciliation plan id was already approved with different content");
        }
        check_pre_start_revision(&existing, current_revision, false)?;
        return Ok(existing);
    }
    let snapshot =
        ApprovedPlanSnapshot::from_plan(plan, current_revision.clone(), approver, statement)?;
    persist(workspace, &snapshot)?;
    Ok(snapshot)
}

pub(crate) fn require(workspace: &Workspace, plan_id: &str) -> Result<ApprovedPlanSnapshot> {
    load(workspace, plan_id)?.ok_or_else(|| {
        anyhow::anyhow!(
            "plan_approval_required: reconciliation plan {plan_id} has no approved snapshot"
        )
    })
}

pub(crate) fn check_pre_start_revision(
    snapshot: &ApprovedPlanSnapshot,
    current_revision: &Revision,
    execution_started: bool,
) -> Result<()> {
    if !execution_started && snapshot.approved_revision != *current_revision {
        bail!(
            "replan_required: repository revision changed before approved plan execution started"
        );
    }
    Ok(())
}

pub(crate) fn status(
    workspace: &Workspace,
    live_plan: &ReconciliationPlan,
    current_revision: &Revision,
    execution: Option<&ReconciliationExecution>,
) -> Result<Value> {
    let Some(snapshot) = load(workspace, &live_plan.id)? else {
        return Ok(json!({
            "state": "unapproved",
            "plan_id": live_plan.id,
            "approved": false,
            "replan_required": false,
        }));
    };
    let started = execution.is_some_and(execution_started);
    let mut reasons = Vec::new();
    if snapshot.plan_digest != digest_plan(live_plan)? {
        reasons.push("approved_plan_digest_changed");
    }
    if snapshot.workspace != live_plan.workspace {
        reasons.push("workspace_changed");
    }
    if snapshot.plan.verification_plan.id != live_plan.verification_plan.id
        || snapshot.verification_policy != live_plan.verification_plan.policy
    {
        reasons.push("verification_contract_changed");
    }
    if live_plan.verification_plan.revision.as_ref() != Some(&snapshot.approved_revision) {
        reasons.push("plan_revision_binding_changed");
    }
    if !started && snapshot.approved_revision != *current_revision {
        reasons.push("repository_revision_changed_before_execution");
    }
    let replan_required = !reasons.is_empty();
    Ok(json!({
        "state": if replan_required {
            "replan_required"
        } else if started {
            "approved_active"
        } else {
            "approved"
        },
        "plan_id": live_plan.id,
        "approved": true,
        "replan_required": replan_required,
        "execution_started": started,
        "reasons": reasons,
        "snapshot": snapshot,
    }))
}

pub(crate) fn persist(workspace: &Workspace, snapshot: &ApprovedPlanSnapshot) -> Result<()> {
    snapshot.validate()?;
    let directory = approval_directory(workspace)?;
    fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create plan approval store {}", directory.display()))?;
    ensure_regular_directory(&directory)?;
    let bytes = serde_json::to_vec(snapshot).context("cannot encode plan approval snapshot")?;
    if bytes.len() as u64 > MAX_APPROVAL_BYTES {
        bail!("plan approval snapshot exceeds persistent store size bound");
    }
    let path = directory.join(format!(
        "{}-{}.json",
        snapshot.plan_id,
        &snapshot.plan_digest[..16]
    ));
    if path.exists() {
        let existing = load(workspace, &snapshot.plan_id)?
            .ok_or_else(|| anyhow::anyhow!("existing approved plan snapshot is invalid"))?;
        if existing.plan_digest == snapshot.plan_digest
            && existing.approved_revision == snapshot.approved_revision
            && existing.plan == snapshot.plan
        {
            return Ok(());
        }
        bail!("replan_required: immutable plan approval path contains different content");
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .with_context(|| format!("cannot create plan approval snapshot {}", path.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("cannot write plan approval snapshot {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("cannot sync plan approval snapshot {}", path.display()))?;
    prune(&directory)?;
    Ok(())
}

pub(crate) fn load(workspace: &Workspace, plan_id: &str) -> Result<Option<ApprovedPlanSnapshot>> {
    validate_plan_id(plan_id)?;
    let directory = approval_directory(workspace)?;
    if !directory.exists() {
        return Ok(None);
    }
    ensure_regular_directory(&directory)?;
    let mut found: Option<ApprovedPlanSnapshot> = None;
    for path in approval_paths(&directory)? {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with(&format!("{plan_id}-")) {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("cannot inspect plan approval snapshot {}", path.display()))?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_APPROVAL_BYTES
        {
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let snapshot: ApprovedPlanSnapshot = match serde_json::from_slice(&bytes) {
            Ok(snapshot) => snapshot,
            Err(_) => continue,
        };
        if snapshot.plan_id != plan_id || snapshot.validate().is_err() {
            continue;
        }
        if let Some(existing) = found.as_ref() {
            if existing.plan_digest != snapshot.plan_digest
                || existing.approved_revision != snapshot.approved_revision
            {
                bail!("replan_required: conflicting immutable approvals exist for plan {plan_id}");
            }
        } else {
            found = Some(snapshot);
        }
    }
    Ok(found)
}

pub(crate) fn digest_plan(plan: &ReconciliationPlan) -> Result<String> {
    plan.validate()?;
    let bytes = serde_json::to_vec(plan).context("cannot encode reconciliation plan for digest")?;
    Ok(digest_bytes(&bytes))
}

fn execution_started(execution: &ReconciliationExecution) -> bool {
    execution.tasks.iter().any(|run| {
        matches!(
            run.task.kind,
            crate::reconcile::ReconciliationTaskKind::Design
                | crate::reconcile::ReconciliationTaskKind::Implementation
                | crate::reconcile::ReconciliationTaskKind::Review
        ) && run.status != ReconciliationRunStatus::Pending
    })
}

fn approval_directory(workspace: &Workspace) -> Result<PathBuf> {
    Ok(
        crate::evidence_store::workspace_state_directory(workspace)?
            .join("reconciliation-approval"),
    )
}

fn approval_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(directory)
        .with_context(|| format!("cannot list plan approval store {}", directory.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            name.ends_with(".json").then(|| entry.path())
        })
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn prune(directory: &Path) -> Result<()> {
    let paths = approval_paths(directory)?;
    let excess = paths.len().saturating_sub(MAX_APPROVAL_SNAPSHOTS);
    for path in paths.into_iter().take(excess) {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

fn ensure_regular_directory(directory: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(directory)
        .with_context(|| format!("cannot inspect plan approval store {}", directory.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("plan approval store path is not a regular directory");
    }
    Ok(())
}

fn validate_plan_id(plan_id: &str) -> Result<()> {
    if plan_id.trim().is_empty()
        || plan_id.len() > 160
        || plan_id.contains('/')
        || plan_id.contains('\\')
    {
        bail!("reconciliation plan id is invalid");
    }
    Ok(())
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

fn digest_text(value: &str) -> String {
    digest_bytes(value.as_bytes())
}

fn is_hex_digest(value: &str) -> bool {
    value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "../../tests/unit/reconciliation/approval.rs"]
mod tests;
