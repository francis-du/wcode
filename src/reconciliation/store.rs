use crate::evidence_store::workspace_state_directory;
use crate::reconcile::ReconciliationPlan;
use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_STORED_PLANS: usize = 512;
const MAX_PLAN_BYTES: u64 = 512 * 1024;

pub(crate) fn persist(workspace: &Workspace, plan: &ReconciliationPlan) -> Result<()> {
    plan.validate()?;
    let directory = plan_directory(workspace)?;
    fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create reconciliation store {}", directory.display()))?;
    let bytes = serde_json::to_vec(plan).context("cannot encode reconciliation plan")?;
    if bytes.len() as u64 > MAX_PLAN_BYTES {
        bail!("reconciliation plan exceeds the persistent store size bound");
    }
    let digest = digest_bytes(&bytes);
    let path = directory.join(format!("{}-{}.json", plan.id, &digest[..16]));
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
    let mut file = options
        .open(&path)
        .with_context(|| format!("cannot create reconciliation plan {}", path.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("cannot write reconciliation plan {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("cannot sync reconciliation plan {}", path.display()))?;
    prune_directory(&directory)?;
    Ok(())
}

pub(crate) fn load(workspace: &Workspace, plan_id: &str) -> Result<Option<ReconciliationPlan>> {
    if plan_id.trim().is_empty()
        || plan_id.len() > 160
        || plan_id.contains('/')
        || plan_id.contains('\\')
    {
        bail!("reconciliation plan id is invalid");
    }
    let directory = plan_directory(workspace)?;
    if !directory.exists() {
        return Ok(None);
    }
    let paths = plan_paths(&directory)?;
    for path in paths.into_iter().rev() {
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if !name.starts_with(&format!("{plan_id}-")) {
            continue;
        }
        if let Some(plan) = read_plan(&path)? {
            return Ok(Some(plan));
        }
    }
    Ok(None)
}

pub(crate) fn recent(workspace: &Workspace, limit: usize) -> Result<Vec<ReconciliationPlan>> {
    let directory = plan_directory(workspace)?;
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let mut plans = Vec::new();
    for path in plan_paths(&directory)?.into_iter().rev() {
        if let Some(plan) = read_plan(&path)? {
            plans.push(plan);
            if plans.len() >= limit.clamp(1, 100) {
                break;
            }
        }
    }
    Ok(plans)
}

pub(crate) fn capabilities() -> serde_json::Value {
    serde_json::json!({
        "persistent": true,
        "format": "immutable-json-plan",
        "scope": "per-workspace",
        "max_plans": MAX_STORED_PLANS,
        "max_plan_bytes": MAX_PLAN_BYTES,
    })
}

fn plan_directory(workspace: &Workspace) -> Result<PathBuf> {
    Ok(workspace_state_directory(workspace)?.join("reconciliation"))
}

fn plan_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let metadata = fs::symlink_metadata(directory).with_context(|| {
        format!(
            "cannot inspect reconciliation store {}",
            directory.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("reconciliation store path is not a regular directory");
    }
    let mut paths = fs::read_dir(directory)
        .with_context(|| format!("cannot list reconciliation store {}", directory.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            name.ends_with(".json").then(|| entry.path())
        })
        .collect::<Vec<_>>();
    paths.sort_by(|left, right| {
        let left_modified = fs::metadata(left)
            .and_then(|metadata| metadata.modified())
            .ok();
        let right_modified = fs::metadata(right)
            .and_then(|metadata| metadata.modified())
            .ok();
        left_modified
            .cmp(&right_modified)
            .then_with(|| left.cmp(right))
    });
    Ok(paths)
}

fn read_plan(path: &Path) -> Result<Option<ReconciliationPlan>> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("cannot inspect reconciliation plan {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > MAX_PLAN_BYTES {
        return Ok(None);
    }
    let bytes = fs::read(path)
        .with_context(|| format!("cannot read reconciliation plan {}", path.display()))?;
    let plan: ReconciliationPlan = match serde_json::from_slice(&bytes) {
        Ok(plan) => plan,
        Err(_) => return Ok(None),
    };
    if plan.validate().is_err() {
        return Ok(None);
    }
    Ok(Some(plan))
}

fn prune_directory(directory: &Path) -> Result<()> {
    let paths = plan_paths(directory)?;
    let excess = paths.len().saturating_sub(MAX_STORED_PLANS);
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
#[path = "../../tests/unit/reconciliation/store.rs"]
mod tests;
