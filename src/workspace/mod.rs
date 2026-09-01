use crate::authorization::{AuthorizationKind, AuthorizationManager, AuthorizationRequest};
use anyhow::{anyhow, bail, Context, Result};
use memchr::memmem;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use uuid::Uuid;
use walkdir::{DirEntry, WalkDir};

const MAX_WORKSPACES: usize = 32;
const MAX_LIST_ENTRIES: usize = 10_000;
const MAX_READ_BYTES: u64 = 1024 * 1024;
const MAX_WRITE_BYTES: usize = 4 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_SAFE_REMOVAL_BYTES: usize = 4 * 1024;
const MAX_SAFE_REDUCTION_PERCENT: usize = 60;
const MAX_TEXT_EDITS: usize = 128;
const MAX_BATCH_WRITE_ITEMS: usize = 64;
pub(crate) const COMMAND_CATALOG: &[&str] = &[
    "cargo",
    "rustc",
    "git",
    "gh",
    "rg",
    "npm",
    "pnpm",
    "yarn",
    "bun",
    "node",
    "python3",
    "pytest",
    "go",
    "make",
    "just",
    "task",
    "uv",
    "ruff",
    "biome",
    "deno",
    "docker",
    "kubectl",
    "terraform",
    "fd",
    "jq",
    "cmake",
    "ninja",
    "dotnet",
    "mvn",
    "gradle",
    "swift",
    "zig",
    "pre-commit",
    "act",
];

fn portable_relative_path(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct WorkspaceSecurity {
    pub allow_risky_exec: bool,
    pub allow_semantic_exec: bool,
    pub allow_destructive_writes: bool,
    pub allow_overlapping_workspaces: bool,
    pub allow_broad_workspace: bool,
}

impl Default for WorkspaceSecurity {
    fn default() -> Self {
        Self {
            allow_risky_exec: false,
            allow_semantic_exec: true,
            allow_destructive_writes: false,
            allow_overlapping_workspaces: false,
            allow_broad_workspace: false,
        }
    }
}

#[derive(Clone)]
pub struct Workspace {
    root: PathBuf,
    root_identity: RootIdentity,
    allow_write: bool,
    allow_exec: bool,
    security: WorkspaceSecurity,
    authorization: AuthorizationManager,
    authorization_workspace: Arc<RwLock<String>>,
    commands: Arc<RwLock<HashSet<String>>>,
    write_locks: Arc<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>>,
}

#[derive(Clone)]
pub struct Workspaces {
    roots: Arc<RwLock<Vec<WorkspaceRoot>>>,
    default_id: String,
    allow_write: bool,
    allow_exec: bool,
    security: WorkspaceSecurity,
    authorization: AuthorizationManager,
}

#[derive(Clone)]
struct WorkspaceRoot {
    id: String,
    workspace: Workspace,
    parent_id: Option<String>,
    markers: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct FileView {
    pub path: String,
    pub sha256: String,
    pub start_line: usize,
    pub end_line: usize,
    pub total_lines: usize,
    pub content: String,
    pub redacted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct RootIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(not(unix))]
    canonical: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceStamp {
    len: u64,
    modified_nanos: u128,
}

#[derive(Debug)]
pub(crate) struct SourceDocument {
    pub path: String,
    pub content: String,
    pub sha256: String,
    pub stamp: SourceStamp,
}

#[derive(Debug, Serialize)]
pub struct EditResult {
    pub path: String,
    pub sha256_before: Option<String>,
    pub sha256_after: String,
    pub bytes_written: usize,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TextEdit {
    pub old_text: String,
    pub new_text: String,
    #[serde(default)]
    pub start_line: Option<usize>,
    #[serde(default)]
    pub end_line: Option<usize>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEditRequest {
    pub path: String,
    pub expected_sha256: String,
    pub edits: Vec<TextEdit>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateFileRequest {
    pub path: String,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MovePathRequest {
    pub source: String,
    pub destination: String,
    #[serde(default)]
    pub expected_source_sha256: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct DirectoryResult {
    pub path: String,
    pub created: bool,
}

#[derive(Debug, Serialize)]
pub struct MoveResult {
    pub source: String,
    pub destination: String,
    pub kind: String,
}

#[derive(Debug, Serialize)]
pub struct DeleteResult {
    pub path: String,
    pub kind: String,
}

#[derive(Debug, Serialize)]
pub struct PathInfo {
    pub path: String,
    pub kind: String,
    pub size: u64,
    pub sha256: Option<String>,
    pub readonly: bool,
    pub modified_at_ms: Option<u64>,
    pub hard_links: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct BatchEditItem {
    pub path: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<EditResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct BatchMoveItem {
    pub source: String,
    pub destination: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<MoveResult>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CommandResult {
    pub program: String,
    pub args: Vec<String>,
    pub exit_code: Option<i32>,
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub truncated: bool,
    pub redacted: bool,
}

#[path = "media.rs"]
mod media;
#[path = "registry.rs"]
mod registry;
#[path = "roots.rs"]
mod roots;

impl Workspace {
    pub fn list_files(&self, path: &str, max_entries: usize) -> Result<Vec<String>> {
        let start = self.existing_path(path)?;
        if !start.is_dir() {
            bail!("path is not a directory");
        }
        let max_entries = max_entries.clamp(1, MAX_LIST_ENTRIES);
        let mut files = Vec::new();
        let mut visited = 0usize;
        let mut cpu_slice = Some(crate::resource::cpu_work(
            crate::resource::WorkClass::Interactive,
        ));
        for entry in WalkDir::new(start)
            .follow_links(false)
            .into_iter()
            .filter_entry(listable_entry)
            .filter_map(|entry| entry.ok())
        {
            visited = visited.saturating_add(1);
            if visited.is_multiple_of(64) {
                drop(cpu_slice.take());
                cpu_slice = Some(crate::resource::cpu_work(
                    crate::resource::WorkClass::Interactive,
                ));
            }
            if entry.file_type().is_file() {
                let relative = portable_relative_path(entry.path().strip_prefix(&self.root)?);
                files.push(relative);
                if files.len() >= max_entries {
                    break;
                }
            }
        }
        files.sort();
        Ok(files)
    }

    pub fn search(
        &self,
        query: &str,
        path: &str,
        max_results: usize,
    ) -> Result<Vec<serde_json::Value>> {
        if query.is_empty() {
            bail!("query must not be empty");
        }
        self.search_queries(&[query.to_owned()], path, max_results.clamp(1, 500), false)
    }

    pub fn search_many(
        &self,
        queries: &[String],
        path: &str,
        max_results: usize,
    ) -> Result<Vec<serde_json::Value>> {
        if queries.is_empty() {
            bail!("queries must not be empty");
        }
        if queries.iter().any(|query| query.is_empty()) {
            bail!("queries must not contain empty strings");
        }
        self.search_queries(queries, path, max_results.clamp(1, 1000), true)
    }

    fn search_queries(
        &self,
        queries: &[String],
        path: &str,
        limit: usize,
        include_query: bool,
    ) -> Result<Vec<serde_json::Value>> {
        let start = self.existing_path(path)?;
        let finders = queries
            .iter()
            .map(|query| memmem::Finder::new(query.as_bytes()))
            .collect::<Vec<_>>();
        let found = AtomicUsize::new(0);
        let root = &self.root;
        let mut results = WalkDir::new(start)
            .follow_links(false)
            .into_iter()
            .filter_entry(visible_entry)
            .filter_map(|entry| entry.ok())
            .par_bridge()
            .filter_map(|entry| {
                if found.load(Ordering::Relaxed) >= limit || !entry.file_type().is_file() {
                    return None;
                }
                let _cpu = crate::resource::cpu_work(crate::resource::WorkClass::Interactive);
                if entry
                    .metadata()
                    .map(|metadata| metadata.len() > MAX_READ_BYTES)
                    .unwrap_or(true)
                {
                    return None;
                }
                let file = entry.path();
                let bytes = fs::read(file).ok()?;
                let matching_queries = finders
                    .iter()
                    .enumerate()
                    .filter_map(|(index, finder)| finder.find(&bytes).is_some().then_some(index))
                    .collect::<Vec<_>>();
                if matching_queries.is_empty() {
                    return None;
                }
                let content = std::str::from_utf8(&bytes).ok()?;
                let relative = portable_relative_path(file.strip_prefix(root).ok()?);
                let mut local = Vec::new();
                'lines: for (index, line) in content.lines().enumerate() {
                    for &query_index in &matching_queries {
                        let query = &queries[query_index];
                        if finders[query_index].find(line.as_bytes()).is_none() {
                            continue;
                        }
                        let slot = found.fetch_add(1, Ordering::Relaxed);
                        if slot >= limit {
                            break 'lines;
                        }
                        let (safe_line, redacted) = redact_sensitive_line(line);
                        if include_query {
                            local.push(serde_json::json!({
                                "query": query,
                                "path": relative,
                                "line": index + 1,
                                "text": safe_line,
                                "redacted": redacted,
                            }));
                        } else {
                            local.push(serde_json::json!({
                                "path": relative,
                                "line": index + 1,
                                "text": safe_line,
                                "redacted": redacted,
                            }));
                        }
                    }
                }
                (!local.is_empty()).then_some(local)
            })
            .flatten()
            .collect::<Vec<_>>();

        results.sort_by(|left, right| {
            let left_path = left["path"].as_str().unwrap_or_default();
            let right_path = right["path"].as_str().unwrap_or_default();
            left_path
                .cmp(right_path)
                .then_with(|| left["line"].as_u64().cmp(&right["line"].as_u64()))
                .then_with(|| left["query"].as_str().cmp(&right["query"].as_str()))
        });
        results.truncate(limit);
        Ok(results)
    }

    pub fn read_files(
        &self,
        paths: &[String],
        start_line: usize,
        end_line: Option<usize>,
    ) -> Result<Vec<serde_json::Value>> {
        if paths.is_empty() || paths.len() > 32 {
            bail!("paths must contain between 1 and 32 files");
        }
        Ok(paths
            .par_iter()
            .map(|path| match self.read_file(path, start_line, end_line) {
                Ok(file) => serde_json::json!({"path": path, "ok": true, "file": file}),
                Err(error) => {
                    serde_json::json!({"path": path, "ok": false, "error": error.to_string()})
                }
            })
            .collect())
    }

    pub fn read_file(
        &self,
        path: &str,
        start_line: usize,
        end_line: Option<usize>,
    ) -> Result<FileView> {
        let file = self.existing_path(path)?;
        let metadata = fs::metadata(&file)?;
        if !metadata.is_file() {
            bail!("path is not a file");
        }
        if metadata.len() > MAX_READ_BYTES {
            bail!("file exceeds 1 MiB read limit");
        }
        let content = fs::read_to_string(&file).context("file is not valid UTF-8 text")?;
        let _cpu = crate::resource::cpu_work(crate::resource::WorkClass::Interactive);
        let hash = sha256(content.as_bytes());
        let total = content.lines().count();
        let start = start_line.max(1);
        let end = end_line
            .unwrap_or(start.saturating_add(499))
            .min(total)
            .max(start.saturating_sub(1));
        let selected = if total == 0 || start > total {
            String::new()
        } else {
            content
                .lines()
                .skip(start - 1)
                .take(end - start + 1)
                .collect::<Vec<_>>()
                .join("\n")
        };
        let (selected, redacted) = redact_sensitive_text(&selected);
        Ok(FileView {
            path: portable_relative_path(file.strip_prefix(&self.root)?),
            sha256: hash,
            start_line: start,
            end_line: end,
            total_lines: total,
            content: selected,
            redacted,
        })
    }

    pub fn replace_text(
        &self,
        path: &str,
        old_text: &str,
        new_text: &str,
        expected_sha256: &str,
    ) -> Result<EditResult> {
        if !self.allow_write {
            bail!("writes are disabled; restart without --read-only");
        }
        if old_text.is_empty() {
            bail!("old_text must not be empty");
        }
        let file = self.existing_path(path)?;
        let file_lock = self.write_lock_for(&file)?;
        let _write_guard = file_lock
            .lock()
            .map_err(|_| anyhow!("file write lock poisoned"))?;
        let locked_file = self.existing_path(path)?;
        if locked_file != file {
            bail!("path target changed while waiting for the write lock; retry the edit");
        }
        ensure_single_link_file(&file)?;
        let content = fs::read_to_string(&file).context("file is not valid UTF-8 text")?;
        let before = sha256(content.as_bytes());
        if before != expected_sha256 {
            bail!("stale file: expected sha256 {expected_sha256}, current sha256 is {before}");
        }
        let count = content.matches(old_text).count();
        if count != 1 {
            bail!("old_text must occur exactly once; found {count} matches");
        }
        let updated = content.replacen(old_text, new_text, 1);
        validate_write_content(&updated)?;
        reject_destructive_replacement(&content, &updated, self.security)?;
        atomic_write(&file, updated.as_bytes())?;
        Ok(EditResult {
            path: portable_relative_path(file.strip_prefix(&self.root)?),
            sha256_before: Some(before),
            sha256_after: sha256(updated.as_bytes()),
            bytes_written: updated.len(),
        })
    }

    pub fn create_directory(&self, path: &str) -> Result<DirectoryResult> {
        if !self.allow_write {
            bail!("writes are disabled; restart without --read-only");
        }
        self.ensure_root_intact()?;
        let relative = Self::validate_relative(path)?;
        if relative.as_os_str().is_empty() {
            return Ok(DirectoryResult {
                path: ".".to_owned(),
                created: false,
            });
        }

        let mut current = self.root.clone();
        let mut created = false;
        for component in relative.components() {
            let Component::Normal(value) = component else {
                continue;
            };
            current.push(value);
            match fs::symlink_metadata(&current) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    bail!(
                        "symlink paths are blocked to preserve workspace isolation: {}",
                        current.display()
                    )
                }
                Ok(metadata) if !metadata.is_dir() => {
                    bail!(
                        "directory path collides with a non-directory: {}",
                        current.display()
                    )
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    fs::create_dir(&current).with_context(|| {
                        format!("cannot create directory {}", current.display())
                    })?;
                    created = true;
                }
                Err(error) => return Err(error.into()),
            }
        }

        Ok(DirectoryResult {
            path: portable_relative_path(current.strip_prefix(&self.root)?),
            created,
        })
    }

    pub(crate) fn ensure_directory(&self, path: &str) -> Result<()> {
        self.create_directory(path).map(|_| ())
    }

    pub fn path_info(&self, path: &str) -> Result<PathInfo> {
        let resolved = self.existing_path(path)?;
        let metadata = fs::symlink_metadata(&resolved)?;
        let kind = if metadata.is_file() {
            "file"
        } else if metadata.is_dir() {
            "directory"
        } else {
            bail!("path is neither a regular file nor a directory");
        };
        let digest = metadata
            .is_file()
            .then(|| sha256_file(&resolved))
            .transpose()?;
        let modified_at_ms = metadata.modified().ok().and_then(|modified| {
            modified
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        });
        Ok(PathInfo {
            path: portable_relative_path(resolved.strip_prefix(&self.root)?),
            kind: kind.to_owned(),
            size: metadata.len(),
            sha256: digest,
            readonly: metadata.permissions().readonly(),
            modified_at_ms,
            hard_links: hard_link_count(&metadata),
        })
    }

    pub fn write_file(
        &self,
        path: &str,
        content: &str,
        expected_sha256: Option<&str>,
    ) -> Result<EditResult> {
        if !self.allow_write {
            bail!("writes are disabled; restart without --read-only");
        }
        validate_write_content(content)?;
        self.ensure_root_intact()?;
        let relative = Self::validate_relative(path)?;
        if relative.as_os_str().is_empty() {
            bail!("file path is required");
        }
        let candidate = self.root.join(&relative);
        match fs::symlink_metadata(&candidate) {
            Ok(_) => {
                let expected = expected_sha256.ok_or_else(|| {
                    anyhow!("expected_sha256 is required when overwriting an existing file")
                })?;
                let file = self.existing_path(path)?;
                let file_lock = self.write_lock_for(&file)?;
                let _write_guard = file_lock
                    .lock()
                    .map_err(|_| anyhow!("file write lock poisoned"))?;
                let locked_file = self.existing_path(path)?;
                if locked_file != file {
                    bail!("path target changed while waiting for the write lock; retry the write");
                }
                ensure_single_link_file(&file)?;
                let before_content =
                    fs::read_to_string(&file).context("file is not valid UTF-8 text")?;
                let before = sha256(before_content.as_bytes());
                if before != expected {
                    bail!("stale file: expected sha256 {expected}, current sha256 is {before}");
                }
                reject_destructive_replacement(&before_content, content, self.security)?;
                atomic_write(&file, content.as_bytes())?;
                Ok(EditResult {
                    path: portable_relative_path(file.strip_prefix(&self.root)?),
                    sha256_before: Some(before),
                    sha256_after: sha256(content.as_bytes()),
                    bytes_written: content.len(),
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if expected_sha256.is_some() {
                    bail!("expected_sha256 was supplied but the target file does not exist");
                }
                self.create_file(path, content)
            }
            Err(error) => Err(error.into()),
        }
    }

    pub fn apply_edits(
        &self,
        path: &str,
        edits: &[TextEdit],
        expected_sha256: &str,
    ) -> Result<EditResult> {
        if !self.allow_write {
            bail!("writes are disabled; restart without --read-only");
        }
        if edits.is_empty() || edits.len() > MAX_TEXT_EDITS {
            bail!("edits must contain between 1 and {MAX_TEXT_EDITS} entries");
        }
        let file = self.existing_path(path)?;
        let file_lock = self.write_lock_for(&file)?;
        let _write_guard = file_lock
            .lock()
            .map_err(|_| anyhow!("file write lock poisoned"))?;
        let locked_file = self.existing_path(path)?;
        if locked_file != file {
            bail!("path target changed while waiting for the write lock; retry the edits");
        }
        ensure_single_link_file(&file)?;
        let content = fs::read_to_string(&file).context("file is not valid UTF-8 text")?;
        let before = sha256(content.as_bytes());
        if before != expected_sha256 {
            bail!("stale file: expected sha256 {expected_sha256}, current sha256 is {before}");
        }
        let updated = apply_text_edits(&content, edits)?;
        validate_write_content(&updated)?;
        reject_destructive_replacement(&content, &updated, self.security)?;
        atomic_write(&file, updated.as_bytes())?;
        Ok(EditResult {
            path: portable_relative_path(file.strip_prefix(&self.root)?),
            sha256_before: Some(before),
            sha256_after: sha256(updated.as_bytes()),
            bytes_written: updated.len(),
        })
    }

    pub fn create_file(&self, path: &str, content: &str) -> Result<EditResult> {
        if !self.allow_write {
            bail!("writes are disabled; restart without --read-only");
        }
        validate_write_content(content)?;
        let file = self.new_path(path)?;
        let file_lock = self.write_lock_for(&file)?;
        let _write_guard = file_lock
            .lock()
            .map_err(|_| anyhow!("file write lock poisoned"))?;
        let locked_file = self.new_path(path)?;
        if locked_file != file {
            bail!("path target changed while waiting for the write lock; retry the create");
        }
        atomic_create_new(&file, content.as_bytes())?;
        Ok(EditResult {
            path: portable_relative_path(file.strip_prefix(&self.root)?),
            sha256_before: None,
            sha256_after: sha256(content.as_bytes()),
            bytes_written: content.len(),
        })
    }

    pub fn create_files(&self, files: &[CreateFileRequest]) -> Result<Vec<BatchEditItem>> {
        validate_batch_paths(files.iter().map(|file| file.path.as_str()))?;
        Ok(files
            .par_iter()
            .map(|file| match self.create_file(&file.path, &file.content) {
                Ok(result) => BatchEditItem {
                    path: file.path.clone(),
                    ok: true,
                    result: Some(result),
                    error: None,
                },
                Err(error) => BatchEditItem {
                    path: file.path.clone(),
                    ok: false,
                    result: None,
                    error: Some(error.to_string()),
                },
            })
            .collect())
    }

    pub fn apply_file_edits(&self, files: &[FileEditRequest]) -> Result<Vec<BatchEditItem>> {
        validate_batch_paths(files.iter().map(|file| file.path.as_str()))?;
        Ok(files
            .par_iter()
            .map(
                |file| match self.apply_edits(&file.path, &file.edits, &file.expected_sha256) {
                    Ok(result) => BatchEditItem {
                        path: file.path.clone(),
                        ok: true,
                        result: Some(result),
                        error: None,
                    },
                    Err(error) => BatchEditItem {
                        path: file.path.clone(),
                        ok: false,
                        result: None,
                        error: Some(error.to_string()),
                    },
                },
            )
            .collect())
    }

    pub fn move_path(&self, source: &str, destination: &str) -> Result<MoveResult> {
        self.move_path_checked(source, destination, None)
    }

    pub fn move_path_checked(
        &self,
        source: &str,
        destination: &str,
        expected_source_sha256: Option<&str>,
    ) -> Result<MoveResult> {
        if !self.allow_write {
            bail!("writes are disabled; restart without --read-only");
        }
        let source_relative = Self::validate_relative(source)?;
        let destination_relative = Self::validate_relative(destination)?;
        if source_relative.as_os_str().is_empty() || destination_relative.as_os_str().is_empty() {
            bail!("source and destination paths are required");
        }
        let source_path = self.existing_path(source)?;
        let destination_path = self.new_path(destination)?;
        if fs::symlink_metadata(&destination_path).is_ok() {
            bail!("move destination already exists; overwriting is not allowed");
        }

        let (first_path, second_path) = if source_path <= destination_path {
            (&source_path, &destination_path)
        } else {
            (&destination_path, &source_path)
        };
        let first_lock = self.write_lock_for(first_path)?;
        let second_lock = self.write_lock_for(second_path)?;
        let _first_guard = first_lock
            .lock()
            .map_err(|_| anyhow!("path write lock poisoned"))?;
        let _second_guard = second_lock
            .lock()
            .map_err(|_| anyhow!("path write lock poisoned"))?;

        let locked_source = self.existing_path(source)?;
        let locked_destination = self.new_path(destination)?;
        if locked_source != source_path || locked_destination != destination_path {
            bail!("move path changed while waiting for locks; retry the move");
        }
        if fs::symlink_metadata(&locked_destination).is_ok() {
            bail!("move destination already exists; overwriting is not allowed");
        }
        let metadata = fs::symlink_metadata(&locked_source)?;
        let kind = if metadata.is_file() {
            ensure_single_link_file(&locked_source)?;
            if let Some(expected) = expected_source_sha256 {
                let current = sha256_file(&locked_source)?;
                if current != expected {
                    bail!("stale move source: expected sha256 {expected}, current sha256 is {current}");
                }
            }
            "file"
        } else if metadata.is_dir() {
            if expected_source_sha256.is_some() {
                bail!("expected_source_sha256 is only valid when moving a regular file");
            }
            if locked_destination.starts_with(&locked_source) {
                bail!("cannot move a directory into itself or one of its descendants");
            }
            validate_movable_directory(&self.root, &locked_source)?;
            "directory"
        } else {
            bail!("move source is neither a regular file nor a directory");
        };
        fs::rename(&locked_source, &locked_destination).with_context(|| {
            format!(
                "failed to move {} to {}",
                locked_source.display(),
                locked_destination.display()
            )
        })?;
        Ok(MoveResult {
            source: portable_relative_path(&source_relative),
            destination: portable_relative_path(&destination_relative),
            kind: kind.to_owned(),
        })
    }

    pub fn move_paths(&self, moves: &[MovePathRequest]) -> Result<Vec<BatchMoveItem>> {
        validate_independent_moves(moves)?;
        Ok(moves
            .par_iter()
            .map(|request| {
                match self.move_path_checked(
                    &request.source,
                    &request.destination,
                    request.expected_source_sha256.as_deref(),
                ) {
                    Ok(result) => BatchMoveItem {
                        source: request.source.clone(),
                        destination: request.destination.clone(),
                        ok: true,
                        result: Some(result),
                        error: None,
                    },
                    Err(error) => BatchMoveItem {
                        source: request.source.clone(),
                        destination: request.destination.clone(),
                        ok: false,
                        result: None,
                        error: Some(error.to_string()),
                    },
                }
            })
            .collect())
    }

    pub fn delete_path(&self, path: &str, expected_sha256: Option<&str>) -> Result<DeleteResult> {
        if !self.allow_write {
            bail!("writes are disabled; restart without --read-only");
        }
        let relative = Self::validate_relative(path)?;
        if relative.as_os_str().is_empty() {
            bail!("workspace root deletion is permanently blocked");
        }
        let resolved = self.existing_path(path)?;
        let metadata = fs::symlink_metadata(&resolved)?;
        let (kind, operation) = if metadata.is_file() {
            ensure_single_link_file(&resolved)?;
            let expected = expected_sha256
                .ok_or_else(|| anyhow!("expected_sha256 is required when deleting a file"))?;
            let current = sha256_file(&resolved)?;
            if current != expected {
                bail!("stale file: expected sha256 {expected}, current sha256 is {current}");
            }
            (
                "file",
                format!(
                    "delete_path\0file\0{}\0{current}",
                    portable_relative_path(&relative)
                ),
            )
        } else if metadata.is_dir() {
            if expected_sha256.is_some() {
                bail!("expected_sha256 is not accepted when deleting an empty directory");
            }
            if fs::read_dir(&resolved)?.next().is_some() {
                bail!("recursive directory deletion is not exposed; directory must be empty");
            }
            (
                "directory",
                format!(
                    "delete_path\0directory\0{}",
                    portable_relative_path(&relative)
                ),
            )
        } else {
            bail!("delete target is neither a regular file nor an empty directory");
        };

        let fingerprint = operation_fingerprint(&self.root, &operation);
        if !self.authorization.consume_one_shot_grant(&fingerprint) {
            let request = self.authorization.request(
                self.authorization_workspace_id(),
                AuthorizationKind::DestructiveDelete,
                format!("delete {kind}: {}", portable_relative_path(&relative)),
                fingerprint,
            );
            bail!(
                "authorization required: {} · {}. Approve this one-shot destructive request in the TUI, then retry the operation",
                request.id,
                request.summary
            );
        }

        let locked = self.existing_path(path)?;
        if locked != resolved {
            bail!("delete target changed after authorization; request approval again");
        }
        if kind == "file" {
            ensure_single_link_file(&locked)?;
            let expected = expected_sha256.expect("file deletion checked expected hash above");
            let current = sha256_file(&locked)?;
            if current != expected {
                bail!("delete target changed after authorization; request approval again");
            }
            fs::remove_file(&locked)?;
        } else {
            if fs::read_dir(&locked)?.next().is_some() {
                bail!("directory changed after authorization and is no longer empty");
            }
            fs::remove_dir(&locked)?;
        }
        Ok(DeleteResult {
            path: portable_relative_path(&relative),
            kind: kind.to_owned(),
        })
    }
}

#[path = "fs_safety.rs"]
mod fs_safety;
use fs_safety::{
    apply_text_edits, atomic_create_new, atomic_write, ensure_single_link_file, hard_link_count,
    listable_entry, operation_fingerprint, reject_destructive_replacement, reject_protected_path,
    root_identity, sha256, sha256_file, source_stamp, validate_batch_paths,
    validate_independent_moves, validate_movable_directory, validate_source_metadata,
    validate_workspace_root, validate_write_content, visible_entry, workspace_id,
};

#[path = "command_policy.rs"]
mod command_policy;
use command_policy::*;

#[path = "operations/execution.rs"]
mod execution;
use execution::redact_sensitive_line;
pub(crate) use execution::redact_sensitive_text;

#[cfg(test)]
#[path = "../../tests/unit/workspace/mod.rs"]
mod tests;
