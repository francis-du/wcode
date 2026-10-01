//! Bounded, read-only discovery. Never guesses IDs or resurrects older snapshots.
use super::*;
use std::io::Read;

const MAX_SCAN_ENTRIES: usize = MAX_TASKS * 2;
const MAX_DISCOVERY_BYTES: u64 = 16 * 1024 * 1024;
pub(crate) const MAX_DISCOVERY_ITEMS: usize = 32;

pub(crate) struct TaskList {
    pub(crate) records: Vec<TaskRecord>,
    pub(crate) truncated: bool,
}

pub(crate) fn list_recent(
    workspace: &Workspace,
    workspace_id: &str,
    tool: &str,
    owner: Option<&str>,
) -> Result<TaskList> {
    let root = task_root(workspace)?;
    let mut listing = TaskList {
        records: Vec::new(),
        truncated: false,
    };
    if !root.exists() {
        return Ok(listing);
    }
    ensure_regular_directory(&root)?;
    let mut ids = Vec::new();
    for (index, entry) in fs::read_dir(&root)?.enumerate() {
        if index >= MAX_SCAN_ENTRIES {
            listing.truncated = true;
            break;
        }
        let entry = entry.context("task discovery entry unavailable")?;
        let name = entry.file_name();
        let Some(id) = name.to_str().filter(|id| valid_task_id(id)) else {
            continue;
        };
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            listing.truncated = true;
            continue;
        }
        ids.push(id.to_owned());
    }
    ids.sort();
    let mut remaining = MAX_DISCOVERY_BYTES;
    for id in ids.into_iter().rev() {
        if remaining == 0 || listing.records.len() == MAX_DISCOVERY_ITEMS {
            listing.truncated = true;
            break;
        }
        let mut record = match read_latest(workspace, &id, &mut remaining) {
            Ok(Some(record)) => record,
            Ok(None) => continue,
            Err(_) => {
                listing.truncated = true;
                continue;
            }
        };
        if record.workspace != workspace_id
            || record.tool_name != tool
            || owner.is_some_and(|owner| owner != record.owner)
            || record.expired(now_ms())
        {
            continue;
        }
        // Discovery consumers receive metadata only, not outputs or credentials.
        record.result = None;
        record.error = None;
        record.live_output = None;
        record.status_message.clear();
        listing.records.push(record);
    }
    Ok(listing)
}

/// Exact observation must fail closed on a damaged latest snapshot. Retention
/// recovery used elsewhere cannot substitute a prior success for current state.
pub(crate) fn load_for_observation(workspace: &Workspace, id: &str) -> Result<Option<TaskRecord>> {
    let mut remaining = MAX_TASK_RECORD_BYTES;
    read_latest(workspace, id, &mut remaining)
}

fn read_latest(workspace: &Workspace, id: &str, remaining: &mut u64) -> Result<Option<TaskRecord>> {
    if !valid_task_id(id) {
        return Ok(None);
    }
    let root = task_root(workspace)?;
    if !root.exists() {
        return Ok(None);
    }
    ensure_regular_directory(&root)?;
    let directory = task_directory(workspace, id)?;
    if !directory.exists() {
        return Ok(None);
    }
    ensure_regular_directory(&directory)?;
    let mut newest = None;
    for (index, entry) in fs::read_dir(&directory)?.enumerate() {
        if index >= MAX_TASK_SNAPSHOTS * 2 {
            bail!("task snapshot discovery bound exceeded");
        }
        let entry = entry.context("task snapshot entry unavailable")?;
        let name = entry.file_name();
        let Some(name) = name.to_str().filter(|name| name.ends_with(".json")) else {
            continue;
        };
        if name.len() != 50
            || name.as_bytes()[20] != b'-'
            || !name.as_bytes()[..20].iter().all(u8::is_ascii_digit)
            || !name.as_bytes()[21..45].iter().all(u8::is_ascii_hexdigit)
        {
            bail!("task snapshot identity invalid");
        }
        let path = entry.path();
        if newest.as_ref().is_none_or(|current| &path > current) {
            newest = Some(path);
        }
    }
    let Some(path) = newest else { return Ok(None) };
    let metadata = fs::symlink_metadata(&path)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_TASK_RECORD_BYTES
        || metadata.len() > *remaining
    {
        *remaining = 0;
        bail!("task snapshot unavailable within read bound");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            bail!("task snapshot cannot be multiply linked");
        }
    }
    let bound = (*remaining).min(MAX_TASK_RECORD_BYTES);
    let mut bytes = Vec::new();
    fs::File::open(&path)?
        .take(bound + 1)
        .read_to_end(&mut bytes)?;
    *remaining = remaining.saturating_sub(bytes.len() as u64);
    if bytes.len() as u64 > bound {
        bail!("task snapshot exceeded read bound");
    }
    let record: TaskRecord = serde_json::from_slice(&bytes).context("task snapshot corrupt")?;
    validate_record(&record)?;
    let expected = format!(
        "{:020}-{}.json",
        record.updated_at_ms,
        &digest_bytes(&bytes)[..24]
    );
    if record.task_id != id
        || path.file_name().and_then(|name| name.to_str()) != Some(expected.as_str())
    {
        bail!("task snapshot does not match bound identity");
    }
    Ok(Some(record))
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/mcp/task_listing.rs"]
mod tests;
