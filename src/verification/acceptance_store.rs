//! Bounded local Acceptance history. Historical JSON never becomes native authority.
use super::acceptance_native::NativeAcceptanceRecord;
use crate::evidence_store::workspace_state_directory;
use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const MAX_RECORDS: usize = 256;
const MAX_BYTES: u64 = 2 * 1024 * 1024;
static MUTATIONS: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct HistoricalRecord {
    schema_version: u32,
    workspace: String,
    root_digest: String,
    record: Value,
}

fn directory(workspace: &Workspace) -> Result<PathBuf> {
    Ok(workspace_state_directory(workspace)?.join("acceptance-history"))
}

fn root_digest(workspace: &Workspace) -> Result<String> {
    super::policy_store::workspace_root_digest(workspace)
}

fn paths(directory: &Path) -> Result<Vec<PathBuf>> {
    match fs::symlink_metadata(directory) {
        Ok(metadata) if metadata.is_dir() && !metadata.file_type().is_symlink() => {}
        Ok(_) => bail!("Acceptance history is not a regular directory"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("cannot inspect Acceptance history"),
    }
    let mut paths = Vec::new();
    for (index, entry) in fs::read_dir(directory)?.enumerate() {
        if index >= MAX_RECORDS {
            bail!("Acceptance history exceeds its bounded capacity");
        }
        let path = entry?.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            bail!("Acceptance history has an unexpected entry");
        }
        paths.push(path);
    }
    paths.sort();
    Ok(paths)
}

pub(crate) fn persist(
    workspace: &Workspace,
    workspace_id: &str,
    native: &NativeAcceptanceRecord,
) -> Result<()> {
    let _guard = MUTATIONS
        .lock()
        .map_err(|_| anyhow::anyhow!("Acceptance history lock poisoned"))?;
    let directory = directory(workspace)?;
    let existing = paths(&directory)?;
    let envelope = HistoricalRecord {
        schema_version: 1,
        workspace: workspace_id.into(),
        root_digest: root_digest(workspace)?,
        record: serde_json::to_value(native.record())?,
    };
    let bytes = serde_json::to_vec(&envelope)?;
    if bytes.len() as u64 > MAX_BYTES {
        bail!("Acceptance Record exceeds its bounded size");
    }
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let path = directory.join(format!("{digest}.json"));
    if existing.contains(&path) {
        read(&path, workspace_id, &envelope.root_digest)?;
        return Ok(());
    }
    if existing.len() == MAX_RECORDS {
        bail!("Acceptance history capacity reached; export retained records before operator-managed archival");
    }
    fs::create_dir_all(&directory)?;
    if fs::symlink_metadata(&directory)?.file_type().is_symlink() {
        bail!("Acceptance history directory is a symlink");
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(&path)
        .context("cannot append Acceptance Record")?;
    if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
        drop(file);
        fs::remove_file(&path).context("cannot remove incomplete Acceptance append")?;
        return Err(error).context("cannot persist Acceptance Record");
    }
    drop(file);
    #[cfg(unix)]
    std::fs::File::open(&directory)?.sync_all()?;
    // This same-process lock does not serialize independent runtime instances.
    // A visible over-capacity append is rejected rather than reported as durable history.
    if paths(&directory).is_err() {
        fs::remove_file(&path).context("cannot roll back over-capacity Acceptance append")?;
        bail!("Acceptance history changed concurrently");
    }
    Ok(())
}

fn read(path: &Path, workspace_id: &str, root: &str) -> Result<Value> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > MAX_BYTES {
        bail!("Acceptance history entry is not a bounded regular file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            bail!("Acceptance history hard links are rejected");
        }
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options.open(path)?;
    let opened = file.metadata()?;
    if !opened.is_file() || opened.len() > MAX_BYTES {
        bail!("Acceptance history changed while opening");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if opened.dev() != metadata.dev() || opened.ino() != metadata.ino() || opened.nlink() != 1 {
            bail!("Acceptance history identity changed while opening");
        }
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        bail!("Acceptance history entry exceeds its byte bound");
    }
    let name = format!("{:x}.json", Sha256::digest(&bytes));
    if path.file_name().and_then(|name| name.to_str()) != Some(&name) {
        bail!("Acceptance history checksum or filename does not match");
    }
    let record: HistoricalRecord = serde_json::from_slice(&bytes)?;
    if record.schema_version != 1 || record.workspace != workspace_id || record.root_digest != root
    {
        bail!("Acceptance history belongs to another workspace");
    }
    Ok(record.record)
}

/// Exported records are explicitly historical. Recompute current Acceptance before any gate.
pub(crate) fn history(workspace: &Workspace, workspace_id: &str) -> Result<Value> {
    let _guard = MUTATIONS
        .lock()
        .map_err(|_| anyhow::anyhow!("Acceptance history lock poisoned"))?;
    let root = root_digest(workspace)?;
    let records = paths(&directory(workspace)?)?
        .iter()
        .map(|path| read(path, workspace_id, &root))
        .collect::<Result<Vec<_>>>()?;
    Ok(serde_json::json!({
        "workspace":workspace_id, "authority":"historical_only", "current_acceptance":false,
        "retained":records.len(), "capacity":MAX_RECORDS, "records":records,
    }))
}
