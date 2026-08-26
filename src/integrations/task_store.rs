use crate::evidence_store::workspace_state_directory;
use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const TASK_SCHEMA_VERSION: u32 = 1;
const MAX_TASKS: usize = 256;
const MAX_TASK_SNAPSHOTS: usize = 32;
const MAX_TASK_RECORD_BYTES: u64 = 4 * 1024 * 1024;
pub(crate) const DEFAULT_TASK_TTL_MS: u64 = 24 * 60 * 60 * 1_000;
pub(crate) const DEFAULT_POLL_INTERVAL_MS: u64 = 1_000;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TaskStatus {
    Working,
    InputRequired,
    Completed,
    Cancelled,
    Failed,
}

impl TaskStatus {
    pub(crate) fn terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Failed)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct TaskRecord {
    pub schema_version: u32,
    pub task_id: String,
    pub owner: String,
    pub workspace: String,
    pub tool_name: String,
    pub runtime_instance_id: String,
    pub status: TaskStatus,
    pub status_message: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    pub ttl_ms: u64,
    pub poll_interval_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<Value>,
}

impl TaskRecord {
    pub(crate) fn working(
        owner: String,
        workspace: String,
        tool_name: String,
        runtime_instance_id: String,
    ) -> Self {
        let now = now_ms();
        Self {
            schema_version: TASK_SCHEMA_VERSION,
            task_id: format!("TASK-{now:020}-{}", Uuid::new_v4().simple()),
            owner,
            workspace,
            tool_name: tool_name.clone(),
            runtime_instance_id,
            status: TaskStatus::Working,
            status_message: format!("{tool_name} is running."),
            created_at_ms: now,
            updated_at_ms: now,
            ttl_ms: DEFAULT_TASK_TTL_MS,
            poll_interval_ms: DEFAULT_POLL_INTERVAL_MS,
            result: None,
            error: None,
        }
    }

    pub(crate) fn complete(&mut self, result: Value) {
        self.status = TaskStatus::Completed;
        self.status_message = format!("{} completed.", self.tool_name);
        self.updated_at_ms = next_update_ms(self.updated_at_ms);
        self.result = Some(result);
        self.error = None;
    }

    pub(crate) fn fail(&mut self, code: i64, message: String) {
        self.status = TaskStatus::Failed;
        self.status_message = bounded_message(&message);
        self.updated_at_ms = next_update_ms(self.updated_at_ms);
        self.result = None;
        self.error = Some(json!({"code": code, "message": message}));
    }

    pub(crate) fn cancel(&mut self) {
        self.status = TaskStatus::Cancelled;
        self.status_message = format!("{} was cancelled.", self.tool_name);
        self.updated_at_ms = next_update_ms(self.updated_at_ms);
        self.result = None;
        self.error = None;
    }

    pub(crate) fn expired(&self, now_ms: u64) -> bool {
        now_ms > self.created_at_ms.saturating_add(self.ttl_ms)
    }

    pub(crate) fn create_result(&self) -> Value {
        json!({
            "resultType": "task",
            "taskId": self.task_id,
            "status": self.status,
            "statusMessage": self.status_message,
            "createdAt": rfc3339_millis(self.created_at_ms),
            "lastUpdatedAt": rfc3339_millis(self.updated_at_ms),
            "ttlMs": self.ttl_ms,
            "pollIntervalMs": self.poll_interval_ms,
        })
    }

    pub(crate) fn get_result(&self) -> Value {
        let mut value = json!({
            "resultType": "complete",
            "taskId": self.task_id,
            "status": self.status,
            "statusMessage": self.status_message,
            "createdAt": rfc3339_millis(self.created_at_ms),
            "lastUpdatedAt": rfc3339_millis(self.updated_at_ms),
            "ttlMs": self.ttl_ms,
            "pollIntervalMs": self.poll_interval_ms,
        });
        if let Some(object) = value.as_object_mut() {
            if self.status == TaskStatus::Completed {
                if let Some(result) = &self.result {
                    object.insert("result".to_owned(), result.clone());
                }
            } else if self.status == TaskStatus::Failed {
                if let Some(error) = &self.error {
                    object.insert("error".to_owned(), error.clone());
                }
            }
        }
        value
    }
}

pub(crate) fn persist(workspace: &Workspace, record: &TaskRecord) -> Result<()> {
    validate_record(record)?;
    let root = task_root(workspace)?;
    fs::create_dir_all(&root)
        .with_context(|| format!("cannot create MCP task store {}", root.display()))?;
    ensure_regular_directory(&root)?;
    let directory = task_directory(workspace, &record.task_id)?;
    if !directory.exists() {
        ensure_task_capacity(workspace, &root)?;
        fs::create_dir(&directory)
            .with_context(|| format!("cannot create MCP task directory {}", directory.display()))?;
    }
    ensure_regular_directory(&directory)?;
    let bytes = serde_json::to_vec(record).context("cannot encode MCP task state")?;
    if bytes.len() as u64 > MAX_TASK_RECORD_BYTES {
        bail!("MCP task record exceeds persistent store size bound");
    }
    let digest = digest_bytes(&bytes);
    let path = directory.join(format!(
        "{:020}-{}.json",
        record.updated_at_ms,
        &digest[..24]
    ));
    if !path.exists() {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options
            .open(&path)
            .with_context(|| format!("cannot create MCP task record {}", path.display()))?;
        file.write_all(&bytes)
            .with_context(|| format!("cannot write MCP task record {}", path.display()))?;
        file.sync_all()
            .with_context(|| format!("cannot sync MCP task record {}", path.display()))?;
    }
    prune_snapshots(&directory)?;
    Ok(())
}

pub(crate) fn load(workspace: &Workspace, task_id: &str) -> Result<Option<TaskRecord>> {
    if !valid_task_id(task_id) {
        return Ok(None);
    }
    let directory = task_directory(workspace, task_id)?;
    if !directory.exists() {
        return Ok(None);
    }
    let metadata = fs::symlink_metadata(&directory)
        .with_context(|| format!("cannot inspect MCP task directory {}", directory.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("MCP task path is not a regular directory");
    }
    for path in snapshot_paths(&directory)?.into_iter().rev() {
        let metadata = match fs::symlink_metadata(&path) {
            Ok(metadata) => metadata,
            Err(_) => continue,
        };
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_TASK_RECORD_BYTES
        {
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        let record: TaskRecord = match serde_json::from_slice(&bytes) {
            Ok(record) => record,
            Err(_) => continue,
        };
        if validate_record(&record).is_ok() && record.task_id == task_id {
            return Ok(Some(record));
        }
    }
    Ok(None)
}

pub(crate) fn find(
    workspaces: &crate::workspace::Workspaces,
    task_id: &str,
) -> Result<Option<(String, Workspace, TaskRecord)>> {
    if !valid_task_id(task_id) {
        return Ok(None);
    }
    for (workspace_id, _) in workspaces.roots() {
        let (_, workspace) = workspaces.select(Some(&workspace_id))?;
        if let Some(record) = load(&workspace, task_id)? {
            return Ok(Some((workspace_id, workspace, record)));
        }
    }
    Ok(None)
}

pub(crate) fn capabilities() -> Value {
    json!({
        "persistent": true,
        "scope": "oauth-client + workspace",
        "max_tasks_per_workspace": MAX_TASKS,
        "max_snapshots_per_task": MAX_TASK_SNAPSHOTS,
        "default_ttl_ms": DEFAULT_TASK_TTL_MS,
        "default_poll_interval_ms": DEFAULT_POLL_INTERVAL_MS,
    })
}

fn validate_record(record: &TaskRecord) -> Result<()> {
    if record.schema_version != TASK_SCHEMA_VERSION
        || !valid_task_id(&record.task_id)
        || record.owner.len() != 64
        || !record.owner.bytes().all(|byte| byte.is_ascii_hexdigit())
        || record.workspace.trim().is_empty()
        || record.workspace.len() > 200
        || record.tool_name.trim().is_empty()
        || record.tool_name.len() > 200
        || record.runtime_instance_id.trim().is_empty()
        || record.runtime_instance_id.len() > 128
        || record.status_message.len() > 2_000
        || record.ttl_ms == 0
        || record.poll_interval_ms == 0
        || record.created_at_ms > record.updated_at_ms
    {
        bail!("invalid MCP task record");
    }
    Ok(())
}

fn valid_task_id(task_id: &str) -> bool {
    task_id.starts_with("TASK-")
        && task_id.len() <= 96
        && task_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

fn task_root(workspace: &Workspace) -> Result<PathBuf> {
    Ok(workspace_state_directory(workspace)?.join("mcp-tasks"))
}

fn task_directory(workspace: &Workspace, task_id: &str) -> Result<PathBuf> {
    if !valid_task_id(task_id) {
        bail!("invalid MCP task id");
    }
    Ok(task_root(workspace)?.join(task_id))
}

fn snapshot_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(directory)
        .with_context(|| format!("cannot list MCP task directory {}", directory.display()))?
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

fn prune_snapshots(directory: &Path) -> Result<()> {
    let paths = snapshot_paths(directory)?;
    let excess = paths.len().saturating_sub(MAX_TASK_SNAPSHOTS);
    for path in paths.into_iter().take(excess) {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

fn ensure_regular_directory(directory: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(directory)
        .with_context(|| format!("cannot inspect MCP task directory {}", directory.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("MCP task store path is not a regular directory");
    }
    Ok(())
}

fn task_directories(root: &Path) -> Result<Vec<(String, PathBuf)>> {
    if !root.exists() {
        return Ok(Vec::new());
    }
    ensure_regular_directory(root)?;
    let mut directories = fs::read_dir(root)
        .with_context(|| format!("cannot list MCP task store {}", root.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_str()?.to_owned();
            let metadata = fs::symlink_metadata(entry.path()).ok()?;
            if valid_task_id(&name) && metadata.is_dir() && !metadata.file_type().is_symlink() {
                Some((name, entry.path()))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    directories.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(directories)
}

fn ensure_task_capacity(workspace: &Workspace, root: &Path) -> Result<()> {
    prune_tasks(workspace, root)?;
    if task_directories(root)?.len() >= MAX_TASKS {
        bail!("MCP task store is at capacity with active tasks; retry after a task completes");
    }
    Ok(())
}

fn prune_tasks(workspace: &Workspace, root: &Path) -> Result<()> {
    let directories = task_directories(root)?;
    let mut excess = directories
        .len()
        .saturating_sub(MAX_TASKS.saturating_sub(1));
    if excess == 0 {
        return Ok(());
    }
    for (task_id, path) in directories {
        if excess == 0 {
            break;
        }
        let terminal = load(workspace, &task_id)?.is_some_and(|record| record.status.terminal());
        if terminal {
            let _ = fs::remove_dir_all(path);
            excess = excess.saturating_sub(1);
        }
    }
    Ok(())
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

fn next_update_ms(previous: u64) -> u64 {
    now_ms().max(previous.saturating_add(1))
}

fn bounded_message(message: &str) -> String {
    message.chars().take(2_000).collect()
}

fn rfc3339_millis(timestamp_ms: u64) -> String {
    let seconds = timestamp_ms / 1_000;
    let millis = timestamp_ms % 1_000;
    let days = i64::try_from(seconds / 86_400).unwrap_or(i64::MAX);
    let seconds_of_day = seconds % 86_400;
    let hour = seconds_of_day / 3_600;
    let minute = (seconds_of_day % 3_600) / 60;
    let second = seconds_of_day % 60;
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z")
}

fn civil_from_days(days_since_epoch: i64) -> (i64, i64, i64) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let mut year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = month_prime + if month_prime < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc3339_timestamp_formatter_matches_epoch_and_leap_day() {
        assert_eq!(rfc3339_millis(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(
            rfc3339_millis(1_709_164_800_123),
            "2024-02-29T00:00:00.123Z"
        );
    }

    #[test]
    fn task_state_is_durable_and_keeps_only_latest_semantics() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let mut record = TaskRecord::working(
            "a".repeat(64),
            "demo".into(),
            "semantic_provider_refresh".into(),
            "runtime-a".into(),
        );
        persist(&workspace, &record).unwrap();
        let loaded = load(&workspace, &record.task_id).unwrap().unwrap();
        assert_eq!(loaded.status, TaskStatus::Working);
        record.complete(json!({"content":[],"structuredContent":{"ok":true},"isError":false}));
        persist(&workspace, &record).unwrap();
        let loaded = load(&workspace, &record.task_id).unwrap().unwrap();
        assert_eq!(loaded.status, TaskStatus::Completed);
        assert_eq!(loaded.result.unwrap()["structuredContent"]["ok"], true);
    }
}
