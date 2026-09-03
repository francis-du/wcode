use crate::evidence_store::workspace_state_directory;
use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const WORKLIST_SCHEMA_VERSION: u32 = 1;
const MAX_WORKLIST_ITEMS: usize = 64;
const MAX_WORKLIST_DEPENDENCIES: usize = 16;
const MAX_WORKLIST_SNAPSHOTS: usize = 128;
const MAX_WORKLIST_BYTES: u64 = 128 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum WorkItemStatus {
    Pending,
    InProgress,
    Done,
    Blocked,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct WorkItemPatch {
    pub id: String,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub status: Option<WorkItemStatus>,
    #[serde(default)]
    pub depends_on: Option<Vec<String>>,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct WorklistUpdate {
    pub expected_revision: u64,
    #[serde(default)]
    pub goal: Option<String>,
    #[serde(default)]
    pub restart: bool,
    #[serde(default)]
    pub items: Vec<WorkItemPatch>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct WorkItem {
    pub id: String,
    pub title: String,
    pub status: WorkItemStatus,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Worklist {
    pub schema_version: u32,
    pub revision: u64,
    pub goal: String,
    pub updated_at_ms: u64,
    pub items: Vec<WorkItem>,
}

pub(crate) fn status(workspace: &Workspace) -> Result<Value> {
    let Some(worklist) = load(workspace)? else {
        return Ok(json!({
            "exists": false,
            "revision": 0,
            "complete": false,
            "runnable": [],
            "parallel_runnable": [],
            "items": []
        }));
    };
    Ok(status_value(&worklist, true))
}

pub(crate) fn active_summary(workspace: &Workspace) -> Result<Option<Value>> {
    let Some(worklist) = load(workspace)? else {
        return Ok(None);
    };
    if is_complete(&worklist) {
        return Ok(None);
    }
    let open = worklist
        .items
        .iter()
        .filter(|item| item.status != WorkItemStatus::Done)
        .take(16)
        .map(|item| {
            json!({
                "id": item.id,
                "title": item.title,
                "status": item.status,
                "depends_on": item.depends_on,
            })
        })
        .collect::<Vec<_>>();
    let runnable = runnable_ids(&worklist);
    Ok(Some(json!({
        "revision": worklist.revision,
        "goal": worklist.goal,
        "open_items": worklist.items.iter().filter(|item| item.status != WorkItemStatus::Done).count(),
        "runnable": runnable,
        "parallel_runnable": if runnable.len() > 1 { runnable.clone() } else { Vec::<String>::new() },
        "items": open,
        "guidance": "Resume runnable incomplete items before inventing a new sequence. Update this worklist as items start/finish; a stale revision must be reread rather than overwritten."
    })))
}

pub(crate) fn update(workspace: &Workspace, update: WorklistUpdate) -> Result<Value> {
    let _guard = update_lock()
        .lock()
        .map_err(|_| anyhow::anyhow!("worklist update lock poisoned"))?;
    let current = load(workspace)?;
    let actual_revision = current.as_ref().map_or(0, |worklist| worklist.revision);
    if update.expected_revision != actual_revision {
        bail!(
            "worklist revision changed: expected {}, current {}; reread worklist_status and merge unfinished items before retrying",
            update.expected_revision,
            actual_revision
        );
    }

    let mut worklist = if update.restart {
        if current
            .as_ref()
            .is_some_and(|worklist| !is_complete(worklist))
        {
            bail!(
                "cannot restart worklist while unfinished items remain; blocked items are preserved until explicitly completed"
            );
        }
        Worklist {
            schema_version: WORKLIST_SCHEMA_VERSION,
            revision: actual_revision,
            goal: update
                .goal
                .as_deref()
                .map(str::trim)
                .filter(|goal| !goal.is_empty())
                .ok_or_else(|| anyhow::anyhow!("restart requires a non-empty goal"))?
                .to_owned(),
            updated_at_ms: now_ms(),
            items: Vec::new(),
        }
    } else if let Some(worklist) = current {
        worklist
    } else {
        Worklist {
            schema_version: WORKLIST_SCHEMA_VERSION,
            revision: 0,
            goal: update
                .goal
                .as_deref()
                .map(str::trim)
                .filter(|goal| !goal.is_empty())
                .ok_or_else(|| anyhow::anyhow!("creating a worklist requires a non-empty goal"))?
                .to_owned(),
            updated_at_ms: now_ms(),
            items: Vec::new(),
        }
    };

    if !update.restart {
        if let Some(goal) = update
            .goal
            .as_deref()
            .map(str::trim)
            .filter(|goal| !goal.is_empty())
        {
            worklist.goal = goal.to_owned();
        }
    }
    apply_patches(&mut worklist, update.items)?;
    worklist.revision = actual_revision.saturating_add(1);
    worklist.updated_at_ms = now_ms();
    validate(&worklist)?;
    persist(workspace, &worklist)?;
    Ok(status_value(&worklist, true))
}

fn apply_patches(worklist: &mut Worklist, patches: Vec<WorkItemPatch>) -> Result<()> {
    if patches.len() > MAX_WORKLIST_ITEMS {
        bail!("worklist update contains too many items");
    }
    let mut by_id = worklist
        .items
        .drain(..)
        .map(|item| (item.id.clone(), item))
        .collect::<BTreeMap<_, _>>();
    for patch in patches {
        validate_id(&patch.id)?;
        if let Some(item) = by_id.get_mut(&patch.id) {
            if let Some(title) = patch.title {
                item.title = title;
            }
            if let Some(status) = patch.status {
                item.status = status;
            }
            if let Some(depends_on) = patch.depends_on {
                item.depends_on = depends_on;
            }
            if let Some(note) = patch.note {
                item.note = (!note.trim().is_empty()).then_some(note);
            }
        } else {
            let title = patch
                .title
                .as_deref()
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .ok_or_else(|| anyhow::anyhow!("new worklist item {} requires title", patch.id))?
                .to_owned();
            by_id.insert(
                patch.id.clone(),
                WorkItem {
                    id: patch.id,
                    title,
                    status: patch.status.unwrap_or(WorkItemStatus::Pending),
                    depends_on: patch.depends_on.unwrap_or_default(),
                    note: patch.note.filter(|note| !note.trim().is_empty()),
                },
            );
        }
    }
    worklist.items = by_id.into_values().collect();
    Ok(())
}

fn status_value(worklist: &Worklist, include_items: bool) -> Value {
    let runnable = runnable_ids(worklist);
    let mut counts = BTreeMap::<&'static str, usize>::new();
    for item in &worklist.items {
        let key = match item.status {
            WorkItemStatus::Pending => "pending",
            WorkItemStatus::InProgress => "in_progress",
            WorkItemStatus::Done => "done",
            WorkItemStatus::Blocked => "blocked",
        };
        *counts.entry(key).or_default() += 1;
    }
    json!({
        "exists": true,
        "revision": worklist.revision,
        "goal": worklist.goal,
        "updated_at_ms": worklist.updated_at_ms,
        "complete": is_complete(worklist),
        "counts": counts,
        "runnable": runnable,
        "parallel_runnable": if runnable.len() > 1 { runnable.clone() } else { Vec::<String>::new() },
        "items": if include_items { json!(worklist.items) } else { json!([]) },
    })
}

fn runnable_ids(worklist: &Worklist) -> Vec<String> {
    let done = worklist
        .items
        .iter()
        .filter(|item| item.status == WorkItemStatus::Done)
        .map(|item| item.id.as_str())
        .collect::<BTreeSet<_>>();
    worklist
        .items
        .iter()
        .filter(|item| {
            matches!(
                item.status,
                WorkItemStatus::Pending | WorkItemStatus::InProgress
            )
        })
        .filter(|item| {
            item.depends_on
                .iter()
                .all(|dependency| done.contains(dependency.as_str()))
        })
        .map(|item| item.id.clone())
        .collect()
}

fn is_complete(worklist: &Worklist) -> bool {
    !worklist.items.is_empty()
        && worklist
            .items
            .iter()
            .all(|item| item.status == WorkItemStatus::Done)
}

fn validate(worklist: &Worklist) -> Result<()> {
    if worklist.schema_version != WORKLIST_SCHEMA_VERSION
        || worklist.revision == 0
        || worklist.goal.trim().is_empty()
        || worklist.goal.len() > 1_000
        || worklist.items.len() > MAX_WORKLIST_ITEMS
    {
        bail!("invalid worklist header or size");
    }
    let ids = worklist
        .items
        .iter()
        .map(|item| item.id.as_str())
        .collect::<BTreeSet<_>>();
    if ids.len() != worklist.items.len() {
        bail!("worklist item ids must be unique");
    }
    for item in &worklist.items {
        validate_id(&item.id)?;
        if item.title.trim().is_empty() || item.title.len() > 300 {
            bail!("worklist item {} has invalid title", item.id);
        }
        if item.note.as_ref().is_some_and(|note| note.len() > 1_000) {
            bail!("worklist item {} note is too long", item.id);
        }
        if item.depends_on.len() > MAX_WORKLIST_DEPENDENCIES {
            bail!("worklist item {} has too many dependencies", item.id);
        }
        let mut unique = BTreeSet::new();
        for dependency in &item.depends_on {
            if dependency == &item.id
                || !ids.contains(dependency.as_str())
                || !unique.insert(dependency)
            {
                bail!(
                    "worklist item {} has invalid dependency {}",
                    item.id,
                    dependency
                );
            }
        }
    }
    validate_acyclic(worklist)?;
    Ok(())
}

fn validate_acyclic(worklist: &Worklist) -> Result<()> {
    let mut remaining = worklist
        .items
        .iter()
        .map(|item| {
            (
                item.id.as_str(),
                item.depends_on
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut completed = BTreeSet::new();
    while !remaining.is_empty() {
        let ready = remaining
            .iter()
            .filter(|(_, dependencies)| dependencies.is_subset(&completed))
            .map(|(id, _)| *id)
            .collect::<Vec<_>>();
        if ready.is_empty() {
            bail!("worklist dependencies contain a cycle");
        }
        for id in ready {
            remaining.remove(id);
            completed.insert(id);
        }
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<()> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("worklist item id is invalid: {id}");
    }
    Ok(())
}

fn load(workspace: &Workspace) -> Result<Option<Worklist>> {
    let directory = worklist_directory(workspace)?;
    if !directory.exists() {
        return Ok(None);
    }
    ensure_regular_directory(&directory)?;
    for path in snapshot_paths(&directory)?.into_iter().rev() {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_WORKLIST_BYTES
        {
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let worklist: Worklist = match serde_json::from_slice(&bytes) {
            Ok(worklist) => worklist,
            Err(_) => continue,
        };
        if validate(&worklist).is_ok() {
            return Ok(Some(worklist));
        }
    }
    Ok(None)
}

fn persist(workspace: &Workspace, worklist: &Worklist) -> Result<()> {
    let directory = worklist_directory(workspace)?;
    fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create worklist store {}", directory.display()))?;
    ensure_regular_directory(&directory)?;
    let bytes = serde_json::to_vec(worklist).context("cannot encode worklist")?;
    if bytes.len() as u64 > MAX_WORKLIST_BYTES {
        bail!("worklist exceeds persistent store size bound");
    }
    let target = directory.join(format!("{:020}.json", worklist.revision));
    let temp = directory.join(format!(".worklist-{}.tmp", Uuid::new_v4().simple()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temp)
        .with_context(|| format!("cannot create worklist temp {}", temp.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("cannot write worklist temp {}", temp.display()))?;
    file.sync_all()
        .with_context(|| format!("cannot sync worklist temp {}", temp.display()))?;
    match fs::hard_link(&temp, &target) {
        Ok(()) => {
            let _ = fs::remove_file(&temp);
        }
        Err(error) => {
            let _ = fs::remove_file(&temp);
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                bail!("worklist revision changed concurrently; reread worklist_status and retry");
            }
            return Err(error).context("cannot atomically publish worklist snapshot");
        }
    }
    prune(&directory)?;
    Ok(())
}

fn worklist_directory(workspace: &Workspace) -> Result<PathBuf> {
    Ok(workspace_state_directory(workspace)?.join("worklist"))
}

fn snapshot_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(directory)
        .with_context(|| format!("cannot list worklist store {}", directory.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?;
            (name.ends_with(".json")
                && name[..name.len().saturating_sub(5)]
                    .bytes()
                    .all(|byte| byte.is_ascii_digit()))
            .then(|| entry.path())
        })
        .collect::<Vec<_>>();
    paths.sort();
    Ok(paths)
}

fn prune(directory: &Path) -> Result<()> {
    let paths = snapshot_paths(directory)?;
    let excess = paths.len().saturating_sub(MAX_WORKLIST_SNAPSHOTS);
    for path in paths.into_iter().take(excess) {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

fn ensure_regular_directory(directory: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(directory)
        .with_context(|| format!("cannot inspect worklist store {}", directory.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("worklist store path is not a regular directory");
    }
    Ok(())
}

fn update_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "../../tests/unit/runtime/worklist.rs"]
mod tests;
