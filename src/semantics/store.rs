use crate::evidence_store::workspace_state_directory;
use crate::semantic::SemanticFact;
use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

const MAX_STORED_SEMANTIC_RECORDS: usize = 4_096;
const MAX_SEMANTIC_RECORD_BYTES: u64 = 128 * 1024;

pub(crate) fn persist(workspace: &Workspace, fact: &SemanticFact) -> Result<()> {
    fact.validate()?;
    let directory = semantic_directory(workspace)?;
    fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create semantic store {}", directory.display()))?;
    let bytes = serde_json::to_vec(fact).context("cannot encode semantic fact")?;
    if bytes.len() as u64 > MAX_SEMANTIC_RECORD_BYTES {
        bail!("semantic fact exceeds the persistent store size bound");
    }
    let digest = digest_bytes(&bytes);
    let filename = format!("{:020}-{}.json", fact.timestamp_ms, &digest[..24]);
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
        .with_context(|| format!("cannot create semantic record {}", path.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("cannot write semantic record {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("cannot sync semantic record {}", path.display()))?;
    prune_directory(&directory)?;
    Ok(())
}

pub(crate) fn load(workspace: &Workspace) -> Result<Vec<SemanticFact>> {
    let directory = semantic_directory(workspace)?;
    if !directory.exists() {
        return Ok(Vec::new());
    }
    let metadata = fs::symlink_metadata(&directory)
        .with_context(|| format!("cannot inspect semantic store {}", directory.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("semantic store path is not a regular directory");
    }
    let paths = semantic_paths(&directory)?;
    let mut latest = BTreeMap::<String, SemanticFact>::new();
    for path in paths
        .into_iter()
        .rev()
        .take(MAX_STORED_SEMANTIC_RECORDS)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_SEMANTIC_RECORD_BYTES
        {
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let fact: SemanticFact = match serde_json::from_slice(&bytes) {
            Ok(fact) => fact,
            Err(_) => continue,
        };
        if fact.validate().is_err() {
            continue;
        }
        let should_replace = latest
            .get(&fact.id)
            .is_none_or(|existing| existing.timestamp_ms <= fact.timestamp_ms);
        if should_replace {
            latest.insert(fact.id.clone(), fact);
        }
    }
    Ok(latest.into_values().collect())
}

pub(crate) fn load_one(workspace: &Workspace, id: &str) -> Result<Option<SemanticFact>> {
    Ok(load(workspace)?.into_iter().find(|fact| fact.id == id))
}

pub(crate) fn capabilities() -> serde_json::Value {
    serde_json::json!({
        "persistent": true,
        "format": "immutable-json-revisions",
        "scope": "per-workspace",
        "max_records": MAX_STORED_SEMANTIC_RECORDS,
        "max_record_bytes": MAX_SEMANTIC_RECORD_BYTES,
        "authoritative_status": "confirmed",
        "candidate_status": "candidate",
        "retired_status": "retired"
    })
}

fn semantic_directory(workspace: &Workspace) -> Result<PathBuf> {
    Ok(workspace_state_directory(workspace)?.join("semantics"))
}

fn semantic_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(directory)
        .with_context(|| format!("cannot list semantic store {}", directory.display()))?
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
    let paths = semantic_paths(directory)?;
    let excess = paths.len().saturating_sub(MAX_STORED_SEMANTIC_RECORDS);
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
#[path = "../../tests/unit/semantics/store.rs"]
mod tests;
