use crate::evidence_store::workspace_state_directory;
use crate::verification::VerificationState;
use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use uuid::Uuid;

const MAX_STATE_BYTES: u64 = 2 * 1024 * 1024;
const MAX_SNAPSHOTS: usize = 256;
static STORE_MUTATIONS: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
enum SnapshotOrder {
    Legacy(u64),
    Generation(u64),
}

pub(crate) fn persist(workspace: &Workspace, state: &VerificationState) -> Result<()> {
    let _mutation = STORE_MUTATIONS
        .lock()
        .map_err(|_| anyhow::anyhow!("verification store mutation lock poisoned"))?;
    let directory = state_directory(workspace)?;
    fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create verification store {}", directory.display()))?;
    let mut validated = VerificationState::default();
    validated
        .restore_workspace(state.clone())
        .context("invalid verification state")?;
    let bytes = serde_json::to_vec(&validated).context("cannot encode verification state")?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        bail!("verification state exceeds the persistent store size bound");
    }
    let generation = validated.persistence_generation();
    if let Some((order, paths)) = highest_snapshots(&directory)? {
        if matches!(order, SnapshotOrder::Generation(current) if generation < current) {
            bail!("stale verification snapshot generation; captured state was not persisted");
        }
        // A genuinely newer native mutation can repair a broken older head.
        // Equal generations must be fully valid and identical; UUID is never order.
        let must_compare = matches!(order, SnapshotOrder::Generation(current) if generation == current)
            || matches!(order, SnapshotOrder::Legacy(_) if generation == 0);
        if must_compare {
            let current = read_head(&paths)?;
            if serde_json::to_vec(&current)? == bytes {
                return Ok(());
            }
            bail!("conflicting verification snapshots share one generation");
        }
        if matches!(order, SnapshotOrder::Legacy(_)) {
            if let Ok(current) = read_head(&paths) {
                let current_generation = current.persistence_generation();
                if generation < current_generation {
                    bail!(
                        "stale verification snapshot generation; captured state was not persisted"
                    );
                }
                if generation == current_generation {
                    if serde_json::to_vec(&current)? == bytes {
                        return Ok(());
                    }
                    bail!("conflicting verification snapshots share one generation");
                }
            }
        }
    }
    let path = directory.join(format!(
        "s{generation:020}-{}.json",
        Uuid::new_v4().simple()
    ));
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
    drop(file);
    prune_directory(&directory)?;
    // Another process can append while this process holds its own mutation lock.
    // Detect an already-visible stale/conflicting head before reporting success.
    let (_, heads) = highest_snapshots(&directory)?.context("verification snapshot disappeared")?;
    if serde_json::to_vec(&read_head(&heads)?)? != bytes {
        bail!("verification head changed concurrently; captured state is not authoritative");
    }
    Ok(())
}

pub(crate) fn load(workspace: &Workspace) -> Result<Option<VerificationState>> {
    let directory = state_directory(workspace)?;
    match fs::symlink_metadata(&directory) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).context("cannot inspect verification store"),
    }
    let paths = snapshot_paths(&directory)?;
    if paths.len() > MAX_SNAPSHOTS {
        bail!("verification store exceeds its snapshot bound");
    }
    let Some((_, heads)) = select_highest(paths)? else {
        return Ok(None);
    };
    read_head(&heads).map(Some)
}

fn highest_snapshots(directory: &Path) -> Result<Option<(SnapshotOrder, Vec<PathBuf>)>> {
    select_highest(snapshot_paths(directory)?)
}

fn select_highest(paths: Vec<PathBuf>) -> Result<Option<(SnapshotOrder, Vec<PathBuf>)>> {
    let Some(last) = paths.last() else {
        return Ok(None);
    };
    let order = snapshot_order(last)?;
    let mut heads = Vec::new();
    for path in paths.into_iter().rev() {
        if snapshot_order(&path)? != order {
            break;
        }
        heads.push(path);
    }
    Ok(Some((order, heads)))
}

fn read_head(paths: &[PathBuf]) -> Result<VerificationState> {
    let mut head = None;
    let mut canonical = None;
    for path in paths {
        let state = read_snapshot(path)?;
        let bytes = serde_json::to_vec(&state)?;
        if canonical
            .as_ref()
            .is_some_and(|previous| previous != &bytes)
        {
            bail!("conflicting verification snapshots share a generation or legacy timestamp");
        }
        canonical = Some(bytes);
        head = Some(state);
    }
    head.context("verification head is missing")
}

fn read_snapshot(path: &Path) -> Result<VerificationState> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("cannot inspect verification snapshot {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!(
            "verification snapshot is not a regular file: {}",
            path.display()
        );
    }
    if metadata.len() > MAX_STATE_BYTES {
        bail!(
            "verification snapshot exceeds its size bound: {}",
            path.display()
        );
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
        .with_context(|| format!("cannot open verification snapshot {}", path.display()))?;
    if !file.metadata()?.is_file() {
        bail!(
            "verification snapshot is not a regular file: {}",
            path.display()
        );
    }
    let mut bytes = Vec::new();
    file.take(MAX_STATE_BYTES + 1)
        .read_to_end(&mut bytes)
        .with_context(|| format!("cannot read verification snapshot {}", path.display()))?;
    if bytes.len() as u64 > MAX_STATE_BYTES {
        bail!(
            "verification snapshot exceeds its size bound: {}",
            path.display()
        );
    }
    let state: VerificationState = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid verification snapshot {}", path.display()))?;
    if matches!(snapshot_order(path)?, SnapshotOrder::Generation(generation)
        if generation != state.persistence_generation())
    {
        bail!("verification snapshot generation header does not match its body");
    }
    let mut validated = VerificationState::default();
    validated
        .restore_workspace(state)
        .context("invalid persisted verification state")?;
    Ok(validated)
}

pub(crate) fn capabilities() -> serde_json::Value {
    serde_json::json!({
        "persistent": true,
        "format": "immutable-json-snapshots",
        "scope": "per-workspace",
        "max_snapshots": MAX_SNAPSHOTS,
        "max_state_bytes": MAX_STATE_BYTES,
        "retention": "newest-snapshots",
        "authoritative_load": "highest-generation-or-error",
        "ordering": "native-mutation-generation",
        "same_generation": "identical-or-error",
        "legacy_timestamp_ties": "identical-or-error",
    })
}

fn state_directory(workspace: &Workspace) -> Result<PathBuf> {
    Ok(workspace_state_directory(workspace)?.join("verification"))
}

fn snapshot_order(path: &Path) -> Result<SnapshotOrder> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .context("verification snapshot filename is not UTF-8")?;
    let modern = name.starts_with('s');
    let offset = usize::from(modern);
    let number = name
        .get(offset..offset + 20)
        .filter(|number| number.bytes().all(|byte| byte.is_ascii_digit()))
        .filter(|_| name.as_bytes().get(offset + 20) == Some(&b'-'))
        .and_then(|number| number.parse::<u64>().ok());
    if modern {
        let generation = number.context("invalid verification snapshot generation header")?;
        return Ok(SnapshotOrder::Generation(generation));
    }
    // Actual legacy snapshots have a 20-digit millisecond timestamp. A single
    // renamed legacy file remains readable; ambiguous timestamp-zero ties fail.
    Ok(SnapshotOrder::Legacy(number.unwrap_or(0)))
}

fn snapshot_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let metadata = fs::symlink_metadata(directory)
        .with_context(|| format!("cannot inspect verification store {}", directory.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("verification store path is not a regular directory");
    }
    let entries = fs::read_dir(directory)
        .with_context(|| format!("cannot list verification store {}", directory.display()))?;
    collect_snapshot_paths(entries)
}

fn collect_snapshot_paths(
    entries: impl Iterator<Item = std::io::Result<fs::DirEntry>>,
) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for (index, entry) in entries.enumerate() {
        if index >= (MAX_SNAPSHOTS + 1) * 2 {
            bail!("verification store enumeration exceeds its entry bound");
        }
        let entry = entry.context("cannot enumerate verification store entry")?;
        if entry
            .path()
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            paths.push(entry.path());
        }
        if paths.len() > MAX_SNAPSHOTS + 1 {
            bail!("verification store enumeration exceeds its snapshot bound");
        }
    }
    let mut ordered = paths
        .into_iter()
        .map(|path| Ok((snapshot_order(&path)?, path)))
        .collect::<Result<Vec<_>>>()?;
    ordered.sort();
    Ok(ordered.into_iter().map(|(_, path)| path).collect())
}

fn prune_directory(directory: &Path) -> Result<()> {
    let paths = snapshot_paths(directory)?;
    let excess = paths.len().saturating_sub(MAX_SNAPSHOTS);
    for path in paths.into_iter().take(excess) {
        fs::remove_file(&path)
            .with_context(|| format!("cannot prune verification snapshot {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/verification/store.rs"]
mod tests;
