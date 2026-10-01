use crate::evidence::{Evidence, EvidenceResult, Revision};
use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

const STORE_VERSION: &str = "v1";
const MAX_STORED_EVIDENCE: usize = 4_096;
const MAX_EVIDENCE_BYTES: u64 = 128 * 1024;
static STORE_MUTATIONS: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub(crate) fn persist(workspace: &Workspace, evidence: &Evidence) -> Result<()> {
    evidence.validate()?;
    let _mutation = STORE_MUTATIONS
        .lock()
        .map_err(|_| anyhow::anyhow!("evidence store mutation lock poisoned"))?;
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
        if read_record(&path)? != *evidence {
            bail!("persistent evidence record has conflicting content");
        }
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
    drop(file);
    if let Err(error) = prune_directory(&directory, &path, &evidence.revision) {
        fs::remove_file(&path).context("cannot roll back rejected evidence append")?;
        return Err(error);
    }
    Ok(())
}

#[cfg(test)]
thread_local! {
    pub(crate) static LOAD_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(crate) fn load(workspace: &Workspace) -> Result<Vec<Evidence>> {
    #[cfg(test)]
    LOAD_CALLS.with(|count| count.set(count.get() + 1));
    load_bounded(workspace, MAX_STORED_EVIDENCE)
}

// A non-authoritative window for cost history, never a complete proof ledger.
pub(crate) fn load_recent(workspace: &Workspace, limit: usize) -> Result<Vec<Evidence>> {
    load_bounded(workspace, limit.clamp(1, MAX_STORED_EVIDENCE))
}

fn load_bounded(workspace: &Workspace, limit: usize) -> Result<Vec<Evidence>> {
    let directory = evidence_directory(workspace)?;
    let metadata = match fs::symlink_metadata(&directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("cannot inspect evidence store"),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("evidence store path is not a regular directory");
    }

    let mut paths = evidence_paths(&directory)?;
    if paths.len() > MAX_STORED_EVIDENCE {
        bail!("evidence store exceeds its record bound; proof loading is incomplete");
    }
    if paths.len() > limit {
        paths = paths.split_off(paths.len() - limit);
    }
    let loaded = crate::resource::parallel_io(&paths, |path| read_record(path))?;
    let mut by_id = std::collections::BTreeMap::<String, Evidence>::new();
    for record in loaded {
        let record = record?;
        if let Some(previous) = by_id.get(&record.id) {
            if previous != &record {
                bail!("conflicting persistent evidence identity: {}", record.id);
            }
        } else {
            by_id.insert(record.id.clone(), record);
        }
    }
    let mut evidence = by_id.into_values().collect::<Vec<_>>();
    evidence.sort_by_key(|record| record.timestamp_ms);
    Ok(evidence)
}

fn read_record(path: &Path) -> Result<Evidence> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("cannot inspect evidence record {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("evidence record is not a regular file: {}", path.display());
    }
    if metadata.len() > MAX_EVIDENCE_BYTES {
        bail!("evidence record exceeds its size bound: {}", path.display());
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .with_context(|| format!("cannot open evidence record {}", path.display()))?;
    if !file.metadata()?.is_file() {
        bail!("evidence record is not a regular file: {}", path.display());
    }
    let mut bytes = Vec::new();
    file.take(MAX_EVIDENCE_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("cannot read evidence record {}", path.display()))?;
    if bytes.len() as u64 > MAX_EVIDENCE_BYTES {
        bail!("evidence record exceeds its size bound: {}", path.display());
    }
    let record: Evidence = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid evidence record {}", path.display()))?;
    record
        .validate()
        .with_context(|| format!("invalid evidence record {}", path.display()))?;
    Ok(record)
}

// Change detection only, never verification evidence. Read immutable record
// identities and metadata rather than parsing every JSON body on every UI poll.
// Include all identities, not just the newest timestamp: late/backdated records
// and removals must also invalidate the observation.
pub(crate) fn change_fingerprint(workspace: &Workspace) -> Result<String> {
    let directory = evidence_directory(workspace)?;
    let metadata = match fs::symlink_metadata(&directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok("empty".into()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("evidence store path is not a regular directory");
    }
    let paths = evidence_paths(&directory)?;
    if paths.len() > MAX_STORED_EVIDENCE {
        bail!("evidence change signal exceeds its record bound");
    }
    let metadata = crate::resource::parallel_io(&paths, |path| evidence_record_metadata(path))?;
    let mut hasher = Sha256::new();
    for item in metadata {
        let (name, len, modified) = item?;
        hasher.update(name.as_bytes());
        hasher.update([0]);
        hasher.update(len.to_le_bytes());
        hasher.update(modified.to_le_bytes());
    }
    Ok(format!("metadata:{:x}", hasher.finalize()))
}

fn evidence_record_metadata(path: &Path) -> Result<(String, u64, u128)> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("evidence change signal requires regular records");
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_owned();
    let modified = metadata
        .modified()?
        .duration_since(std::time::UNIX_EPOCH)?
        .as_nanos();
    Ok((name, metadata.len(), modified))
}

pub(crate) fn capabilities() -> serde_json::Value {
    serde_json::json!({
        "persistent": true,
        "format": "immutable-json-records",
        "scope": "per-workspace",
        "max_records": MAX_STORED_EVIDENCE,
        "max_record_bytes": MAX_EVIDENCE_BYTES,
        "retention": "bounded-with-current-negative-protection",
        "authoritative_load": "complete-within-retention-or-error",
        "recent_load": "bounded-non-authoritative-window",
        "negative_protection": "native-bound-incoming-revision-only",
    })
}

fn evidence_directory(workspace: &Workspace) -> Result<PathBuf> {
    Ok(workspace_state_directory(workspace)?.join("evidence"))
}

pub(crate) fn workspace_state_directory(workspace: &Workspace) -> Result<PathBuf> {
    let workspace_key = workspace_root_key(workspace.root())?;
    Ok(state_root()?.join(STORE_VERSION).join(workspace_key))
}

fn workspace_root_key(root: &Path) -> Result<String> {
    let mut hasher = Sha256::new();
    if let Some(root) = root.to_str() {
        // Preserve the established storage identity for every UTF-8 root.
        hasher.update(root.as_bytes());
    } else {
        // Lossy text merges distinct native paths. Never read that legacy
        // shared directory for a non-UTF-8 root, even as a migration fallback.
        hasher.update(b"wcode-workspace-state-root-v2\0");
        #[cfg(unix)]
        {
            use std::os::unix::ffi::OsStrExt;
            hasher.update(b"unix\0");
            hasher.update(root.as_os_str().as_bytes());
        }
        #[cfg(windows)]
        {
            use std::os::windows::ffi::OsStrExt;
            hasher.update(b"windows\0");
            for unit in root.as_os_str().encode_wide() {
                hasher.update(unit.to_le_bytes());
            }
        }
        #[cfg(not(any(unix, windows)))]
        bail!("lossless non-UTF-8 Workspace identity is unavailable on this platform");
    }
    Ok(format!("{:x}", hasher.finalize()))
}

pub(crate) fn state_root() -> Result<PathBuf> {
    Ok(crate::core_types::intelligence_state_root()?)
}

fn evidence_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let entries = fs::read_dir(directory)
        .with_context(|| format!("cannot list evidence store {}", directory.display()))?;
    collect_record_paths(entries)
}

fn collect_record_paths(
    entries: impl Iterator<Item = std::io::Result<fs::DirEntry>>,
) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for (index, entry) in entries.enumerate() {
        // Persistence temporarily appends one record before pruning.
        if index >= (MAX_STORED_EVIDENCE + 1) * 2 {
            bail!("evidence store enumeration exceeds its entry bound");
        }
        let entry = entry.context("cannot enumerate evidence store entry")?;
        if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            paths.push(entry.path());
        }
        if paths.len() > MAX_STORED_EVIDENCE + 1 {
            bail!("evidence store enumeration exceeds its record bound");
        }
    }
    paths.sort();
    Ok(paths)
}

fn prune_directory(directory: &Path, incoming: &Path, revision: &Revision) -> Result<()> {
    let paths = evidence_paths(directory)?;
    if paths.len() <= MAX_STORED_EVIDENCE {
        return Ok(());
    }
    // Avoid entering the shared I/O pool while holding the mutation lock.
    let records = paths
        .iter()
        .map(|path| read_record(path))
        .collect::<Result<Vec<_>>>()?;
    let mut by_id = std::collections::BTreeMap::new();
    for record in &records {
        if by_id
            .insert(record.id.as_str(), record)
            .is_some_and(|previous| previous != record)
        {
            bail!("conflicting persistent evidence identity: {}", record.id);
        }
    }
    // Incoming revisions and MCP namespaces are bound by native runtime callers.
    // Reuse effective proof selection so a genuine retry can release an obsolete
    // negative while a narrow Pass cannot erase a broad failure. This is bounded
    // current-revision protection, not a complete lifetime audit.
    let protected = crate::evidence::latest_current(
        records
            .iter()
            .filter(|record| record.authority != crate::evidence::EvidenceAuthority::SelfReported),
        revision,
    )
    .into_iter()
    .chain(records.iter().filter(|record| {
        record.revision == *revision
            && record.authority != crate::evidence::EvidenceAuthority::SelfReported
            && matches!(
                record.kind,
                crate::evidence::EvidenceKind::HumanApproval
                    | crate::evidence::EvidenceKind::Reconciliation
            )
    }))
    .filter(|record| {
        matches!(
            record.result,
            EvidenceResult::Fail | EvidenceResult::Disagree | EvidenceResult::Inconclusive
        )
    })
    .map(|record| record.id.as_str())
    .collect::<std::collections::BTreeSet<_>>();
    let mut fallback = None;
    let mut preferred = None;
    for (path, record) in paths.into_iter().zip(&records) {
        if path == incoming || protected.contains(record.id.as_str()) {
            continue;
        }
        // Reclaim advisory or old revisions before current positive proof.
        if record.authority == crate::evidence::EvidenceAuthority::SelfReported
            || record.revision != *revision
        {
            preferred = Some(path);
            break;
        }
        if fallback.is_none() {
            fallback = Some(path);
        }
    }
    let victim = preferred.or(fallback).context(
        "evidence capacity is occupied by protected current negative proof; append rejected",
    )?;
    fs::remove_file(&victim)
        .with_context(|| format!("cannot prune evidence record {}", victim.display()))?;
    Ok(())
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
#[path = "../../tests/unit/evidence/store.rs"]
mod tests;
