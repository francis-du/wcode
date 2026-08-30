use crate::evidence_store::workspace_state_directory;
use crate::verification::VerificationState;
use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const MAX_STATE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_SNAPSHOTS: usize = 256;

pub(crate) fn persist(workspace: &Workspace, state: &VerificationState) -> Result<()> {
    let directory = state_directory(workspace)?;
    fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create verification store {}", directory.display()))?;
    let bytes = serde_json::to_vec(state).context("cannot encode verification state")?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        bail!("verification state exceeds the persistent store size bound");
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0);
    let path = directory.join(format!("{timestamp:020}-{}.json", Uuid::new_v4().simple()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&path)
        .with_context(|| format!("cannot create verification snapshot {}", path.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("cannot write verification snapshot {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("cannot sync verification snapshot {}", path.display()))?;
    prune_directory(&directory)?;
    Ok(())
}

pub(crate) fn load(workspace: &Workspace) -> Result<Option<VerificationState>> {
    let directory = state_directory(workspace)?;
    if !directory.exists() {
        return Ok(None);
    }
    for path in snapshot_paths(&directory)?.into_iter().rev() {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_STATE_BYTES
        {
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        if let Ok(state) = serde_json::from_slice(&bytes) {
            return Ok(Some(state));
        }
    }
    Ok(None)
}

pub(crate) fn capabilities() -> serde_json::Value {
    serde_json::json!({
        "persistent": true,
        "format": "immutable-json-snapshots",
        "scope": "per-workspace",
        "max_snapshots": MAX_SNAPSHOTS,
        "max_state_bytes": MAX_STATE_BYTES,
    })
}

fn state_directory(workspace: &Workspace) -> Result<PathBuf> {
    Ok(workspace_state_directory(workspace)?.join("verification"))
}

fn snapshot_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let metadata = fs::symlink_metadata(directory)
        .with_context(|| format!("cannot inspect verification store {}", directory.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("verification store path is not a regular directory");
    }
    let mut paths = fs::read_dir(directory)
        .with_context(|| format!("cannot list verification store {}", directory.display()))?
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

fn prune_directory(directory: &Path) -> Result<()> {
    let paths = snapshot_paths(directory)?;
    let excess = paths.len().saturating_sub(MAX_SNAPSHOTS);
    for path in paths.into_iter().take(excess) {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/verification/store.rs"]
mod tests;
