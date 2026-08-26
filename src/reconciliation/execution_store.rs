use crate::evidence_store::workspace_state_directory;
use crate::reconcile::ReconciliationExecution;
use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_EXECUTION_SNAPSHOTS: usize = 1_024;
const MAX_EXECUTION_BYTES: u64 = 2 * 1024 * 1024;

pub(crate) fn persist(workspace: &Workspace, execution: &ReconciliationExecution) -> Result<()> {
    execution.validate()?;
    let bytes = serde_json::to_vec(execution).context("cannot encode reconciliation execution")?;
    if bytes.len() as u64 > MAX_EXECUTION_BYTES {
        bail!("reconciliation execution exceeds the persistent store size bound");
    }
    let directory = execution_directory(workspace)?;
    fs::create_dir_all(&directory).with_context(|| {
        format!(
            "cannot create reconciliation execution store {}",
            directory.display()
        )
    })?;
    let digest = digest_bytes(&bytes);
    let path = directory.join(format!(
        "{:020}-{}-{}.json",
        execution.updated_at_ms,
        execution.plan_id,
        &digest[..16]
    ));
    if path.exists() {
        return Ok(());
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path).with_context(|| {
        format!(
            "cannot create reconciliation execution snapshot {}",
            path.display()
        )
    })?;
    file.write_all(&bytes).with_context(|| {
        format!(
            "cannot write reconciliation execution snapshot {}",
            path.display()
        )
    })?;
    file.sync_all().with_context(|| {
        format!(
            "cannot sync reconciliation execution snapshot {}",
            path.display()
        )
    })?;
    prune_directory(&directory)?;
    Ok(())
}

pub(crate) fn load(
    workspace: &Workspace,
    plan_id: &str,
) -> Result<Option<ReconciliationExecution>> {
    if plan_id.trim().is_empty()
        || plan_id.len() > 160
        || plan_id.contains('/')
        || plan_id.contains('\\')
    {
        bail!("reconciliation plan id is invalid");
    }
    let directory = execution_directory(workspace)?;
    if !directory.exists() {
        return Ok(None);
    }
    let prefix = format!("-{plan_id}-");
    for path in execution_paths(&directory)?.into_iter().rev() {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.contains(&prefix) {
            continue;
        }
        if let Some(execution) = read_execution(&path)? {
            if execution.plan_id == plan_id {
                return Ok(Some(execution));
            }
        }
    }
    Ok(None)
}

pub(crate) fn capabilities() -> serde_json::Value {
    serde_json::json!({
        "persistent": true,
        "format": "immutable-execution-snapshots",
        "scope": "per-workspace",
        "max_snapshots": MAX_EXECUTION_SNAPSHOTS,
        "max_snapshot_bytes": MAX_EXECUTION_BYTES
    })
}

fn execution_directory(workspace: &Workspace) -> Result<PathBuf> {
    Ok(workspace_state_directory(workspace)?.join("reconciliation-execution"))
}

fn execution_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let metadata = fs::symlink_metadata(directory).with_context(|| {
        format!(
            "cannot inspect reconciliation execution store {}",
            directory.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("reconciliation execution store path is not a regular directory");
    }
    let mut paths = fs::read_dir(directory)
        .with_context(|| {
            format!(
                "cannot list reconciliation execution store {}",
                directory.display()
            )
        })?
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

fn read_execution(path: &Path) -> Result<Option<ReconciliationExecution>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(_) => return Ok(None),
    };
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_EXECUTION_BYTES
    {
        return Ok(None);
    }
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return Ok(None),
    };
    let execution: ReconciliationExecution = match serde_json::from_slice(&bytes) {
        Ok(execution) => execution,
        Err(_) => return Ok(None),
    };
    if execution.validate().is_err() {
        return Ok(None);
    }
    Ok(Some(execution))
}

fn prune_directory(directory: &Path) -> Result<()> {
    let paths = execution_paths(directory)?;
    let excess = paths.len().saturating_sub(MAX_EXECUTION_SNAPSHOTS);
    for path in paths.into_iter().take(excess) {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reconcile::{
        ReconciliationExecution, ReconciliationPlan, ReconciliationTask, ReconciliationTaskKind,
    };
    use crate::risk::RiskLevel;
    use crate::verification::VerificationPlan;

    #[test]
    fn execution_state_survives_a_fresh_load() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let plan = ReconciliationPlan {
            id: "RP-exec".into(),
            workspace: "demo".into(),
            risk_level: RiskLevel::Low,
            design_changes: vec![],
            drift_ids: vec![],
            impacted_components: vec![],
            impacted_symbols: vec![],
            impacted_tests: vec![],
            implementation_tasks: vec![ReconciliationTask {
                id: "RT-1".into(),
                kind: ReconciliationTaskKind::Implementation,
                subject: "component:a".into(),
                description: "Implement change.".into(),
                depends_on: vec![],
            }],
            change_intents: vec![],
            verification_plan: VerificationPlan {
                id: "VP-exec".into(),
                workspace: "demo".into(),
                subject: "change:fixture".into(),
                risk_level: RiskLevel::Low,
                policy: "risk-adaptive/v1/low".into(),
                deterministic_level: "quick".into(),
                deterministic_checks: vec![],
                reviewer_roles: vec![],
                require_property: false,
                require_mutation: false,
                require_fuzz: false,
                require_human_approval: false,
                automation_gaps: vec![],
                job_ids: vec![],
            },
        };
        let execution = ReconciliationExecution::from_plan(&plan).unwrap();
        persist(&workspace, &execution).unwrap();
        assert_eq!(
            load(&workspace, &plan.id).unwrap().unwrap().plan_id,
            plan.id
        );
    }
}
