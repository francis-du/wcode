use crate::evidence::Evidence;
use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
#[cfg(not(test))]
use std::env;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const STORE_VERSION: &str = "v1";
const MAX_STORED_EVIDENCE: usize = 4_096;
const MAX_EVIDENCE_BYTES: u64 = 128 * 1024;

pub(crate) fn persist(workspace: &Workspace, evidence: &Evidence) -> Result<()> {
    evidence.validate()?;
    let directory = evidence_directory(workspace)?;
    fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create evidence store {}", directory.display()))?;
    let bytes = serde_json::to_vec(evidence).context("cannot encode evidence for persistence")?;
    if bytes.len() as u64 > MAX_EVIDENCE_BYTES {
        bail!("evidence record exceeds the persistent store size bound");
    }

    let digest = digest_bytes(&bytes);
    let filename = format!("{:020}-{}.json", evidence.timestamp_ms, &digest[..24]);
    let path = directory.join(filename);
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
        .with_context(|| format!("cannot create evidence record {}", path.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("cannot write evidence record {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("cannot sync evidence record {}", path.display()))?;
    prune_directory(&directory)?;
    Ok(())
}

pub(crate) fn load(workspace: &Workspace) -> Result<Vec<Evidence>> {
    let directory = evidence_directory(workspace)?;
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let metadata = fs::symlink_metadata(&directory)
        .with_context(|| format!("cannot inspect evidence store {}", directory.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("evidence store path is not a regular directory");
    }

    let mut paths = evidence_paths(&directory)?;
    if paths.len() > MAX_STORED_EVIDENCE {
        paths = paths.split_off(paths.len() - MAX_STORED_EVIDENCE);
    }
    let mut evidence = Vec::with_capacity(paths.len());
    for path in paths {
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("cannot inspect evidence record {}", path.display()))?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_EVIDENCE_BYTES
        {
            continue;
        }
        let bytes = fs::read(&path)
            .with_context(|| format!("cannot read evidence record {}", path.display()))?;
        let record: Evidence = match serde_json::from_slice(&bytes) {
            Ok(record) => record,
            Err(_) => continue,
        };
        if record.validate().is_ok() {
            evidence.push(record);
        }
    }
    evidence.sort_by_key(|record| record.timestamp_ms);
    Ok(evidence)
}

pub(crate) fn capabilities() -> serde_json::Value {
    serde_json::json!({
        "persistent": true,
        "format": "immutable-json-records",
        "scope": "per-workspace",
        "max_records": MAX_STORED_EVIDENCE,
        "max_record_bytes": MAX_EVIDENCE_BYTES,
    })
}

fn evidence_directory(workspace: &Workspace) -> Result<PathBuf> {
    Ok(workspace_state_directory(workspace)?.join("evidence"))
}

pub(crate) fn workspace_state_directory(workspace: &Workspace) -> Result<PathBuf> {
    let mut hasher = Sha256::new();
    hasher.update(workspace.root().to_string_lossy().as_bytes());
    let workspace_key = format!("{:x}", hasher.finalize());
    Ok(state_root()?.join(STORE_VERSION).join(workspace_key))
}

#[cfg(test)]
fn state_root() -> Result<PathBuf> {
    Ok(std::env::temp_dir()
        .join("wcode-test-intelligence")
        .join(std::process::id().to_string()))
}

#[cfg(not(test))]
fn state_root() -> Result<PathBuf> {
    if let Some(path) = env::var_os("WCODE_STATE_DIR").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(path).join("intelligence"));
    }
    if cfg!(target_os = "windows") {
        let base = env::var_os("LOCALAPPDATA")
            .or_else(|| env::var_os("USERPROFILE"))
            .context("LOCALAPPDATA and USERPROFILE are not set")?;
        return Ok(PathBuf::from(base).join("wcode/intelligence"));
    }
    if let Some(base) = env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()) {
        return Ok(PathBuf::from(base).join("wcode/intelligence"));
    }
    let home = env::var_os("HOME").context("HOME is not set")?;
    Ok(PathBuf::from(home).join(".local/state/wcode/intelligence"))
}

fn evidence_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(directory)
        .with_context(|| format!("cannot list evidence store {}", directory.display()))?
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
    let paths = evidence_paths(directory)?;
    let excess = paths.len().saturating_sub(MAX_STORED_EVIDENCE);
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
    use crate::evidence::{Confidence, EvidenceKind, EvidenceResult, Revision};

    #[test]
    fn evidence_survives_a_fresh_load_and_is_workspace_scoped() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        let first_workspace = Workspace::new(first.path(), false, false).unwrap();
        let second_workspace = Workspace::new(second.path(), false, false).unwrap();
        let evidence = Evidence::new(
            "EV-PERSIST-1".into(),
            "REQ-1".into(),
            EvidenceKind::UnitTest,
            "cargo-test".into(),
            Revision {
                design: None,
                code: "sha256:fixture".into(),
            },
            EvidenceResult::Pass,
            Confidence::Deterministic,
        )
        .unwrap();

        persist(&first_workspace, &evidence).unwrap();
        assert_eq!(load(&first_workspace).unwrap(), vec![evidence]);
        assert!(load(&second_workspace).unwrap().is_empty());
    }
}
