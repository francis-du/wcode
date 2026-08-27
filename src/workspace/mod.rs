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
const MAX_MOVE_TREE_ENTRIES: usize = 50_000;
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

#[derive(Clone, Copy, Debug, Default, Serialize)]
pub struct WorkspaceSecurity {
    pub allow_risky_exec: bool,
    pub allow_destructive_writes: bool,
    pub allow_overlapping_workspaces: bool,
    pub allow_broad_workspace: bool,
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

#[path = "registry.rs"]
mod registry;

#[path = "roots.rs"]
mod roots;

#[path = "media.rs"]
mod media;

impl Workspace {
    pub fn list_files(&self, path: &str, max_entries: usize) -> Result<Vec<String>> {
        let start = self.existing_path(path)?;
        if !start.is_dir() {
            bail!("path is not a directory");
        }
        let max_entries = max_entries.clamp(1, MAX_LIST_ENTRIES);
        let mut files = Vec::new();
        for entry in WalkDir::new(start)
            .follow_links(false)
            .into_iter()
            .filter_entry(listable_entry)
            .filter_map(|entry| entry.ok())
        {
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
                if entry
                    .metadata()
                    .map(|metadata| metadata.len() > MAX_READ_BYTES)
                    .unwrap_or(true)
                {
                    return None;
                }
                let file = entry.path();
                let bytes = fs::read(file).ok()?;
                if !queries
                    .iter()
                    .any(|query| memmem::find(&bytes, query.as_bytes()).is_some())
                {
                    return None;
                }
                let content = std::str::from_utf8(&bytes).ok()?;
                let relative = portable_relative_path(file.strip_prefix(root).ok()?);
                let mut local = Vec::new();
                'lines: for (index, line) in content.lines().enumerate() {
                    for query in queries {
                        if !line.contains(query) {
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

    pub async fn run_command(
        &self,
        program: &str,
        args: &[String],
        cwd: &str,
        timeout_seconds: u64,
    ) -> Result<CommandResult> {
        if !self.allow_exec {
            bail!("command execution is disabled; restart without --no-exec");
        }
        validate_authorizable_program(program)?;
        if !self
            .commands
            .read()
            .expect("workspace command allowlist lock poisoned")
            .contains(program)
        {
            let fingerprint = self.command_access_fingerprint(program);
            let request = self.authorization.request_command(
                self.authorization_workspace_id(),
                program,
                fingerprint,
            );
            bail!(
                "authorization required: {} · {}. Approve this request in the TUI or Web UI, then retry the operation",
                request.id,
                request.summary
            );
        }
        if program == "cargo" && args == ["fmt"] && !self.allow_write {
            bail!("cargo fmt modifies source files and is blocked in a read-only workspace");
        }
        let dynamically_authorized = !COMMAND_CATALOG.contains(&program);
        let mut effective_security = self.security;
        if dynamically_authorized {
            effective_security.allow_risky_exec = true;
        }
        if !effective_security.allow_risky_exec
            && validate_command_policy(program, args, effective_security).is_err()
        {
            let mut elevated = effective_security;
            elevated.allow_risky_exec = true;
            if validate_command_policy(program, args, elevated).is_ok() {
                let operation = format!("run_command\0{program}\0{}\0{cwd}", args.join("\0"));
                self.authorize_risky_operation(
                    AuthorizationKind::RiskyExecution,
                    &operation,
                    &format!(
                        "allow repository-aware command: {program} {}",
                        args.join(" ")
                    ),
                )?;
                effective_security = elevated;
            }
        }
        validate_command_policy(program, args, effective_security)?;
        let cwd = self.existing_path(cwd)?;
        if !cwd.is_dir() {
            bail!("cwd is not a directory");
        }
        let effective_args = hardened_command_args(program, args);
        let mut command = Command::new(program);
        command
            .args(&effective_args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        scrub_sensitive_environment(
            &mut command,
            program,
            args,
            effective_security.allow_risky_exec,
        );
        if program == "git" {
            command
                .env("GIT_CEILING_DIRECTORIES", &self.root)
                .env("GIT_DISCOVERY_ACROSS_FILESYSTEM", "0");
        }

        let mut child = command.spawn().context("failed to start command")?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("command stdout is unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("command stderr is unavailable"))?;
        let stdout_task = tokio::spawn(read_bounded_stream(stdout));
        let stderr_task = tokio::spawn(read_bounded_stream(stderr));
        let status = match timeout(
            Duration::from_secs(timeout_seconds.clamp(1, 300)),
            child.wait(),
        )
        .await
        {
            Ok(result) => result?,
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                let _ = stdout_task.await;
                let _ = stderr_task.await;
                bail!("command timed out and was terminated");
            }
        };
        let (stdout, stdout_cut) = stdout_task
            .await
            .map_err(|error| anyhow!("stdout reader failed: {error}"))??;
        let (stderr, stderr_cut) = stderr_task
            .await
            .map_err(|error| anyhow!("stderr reader failed: {error}"))??;
        let (stdout, stdout_redacted) = redact_sensitive_text(&stdout);
        let (stderr, stderr_redacted) = redact_sensitive_text(&stderr);
        Ok(CommandResult {
            program: program.to_owned(),
            args: args.to_vec(),
            exit_code: status.code(),
            success: status.success(),
            stdout,
            stderr,
            truncated: stdout_cut || stderr_cut,
            redacted: stdout_redacted || stderr_redacted,
        })
    }

    pub(crate) async fn run_verification_command(
        &self,
        program: &str,
        args: &[String],
        cwd: &str,
        timeout_seconds: u64,
    ) -> Result<CommandResult> {
        validate_verification_command_shape(program, args)?;
        let mut verification_workspace = self.clone();
        verification_workspace.security.allow_risky_exec = true;
        verification_workspace
            .run_command(program, args, cwd, timeout_seconds)
            .await
    }

    pub(crate) fn workspace_program_available(&self, program: &str) -> bool {
        program.contains(['/', '\\'])
            && self.existing_path(program).is_ok_and(|path| path.is_file())
    }

    pub(crate) async fn run_trusted_runtime_command(
        &self,
        program: &str,
        args: &[String],
        cwd: &str,
        timeout_seconds: u64,
    ) -> Result<CommandResult> {
        if !self.allow_exec {
            bail!("runtime executor requires command execution; restart without --no-exec");
        }
        if !self.security.allow_risky_exec {
            let operation = format!("runtime_executor\0{program}\0{}\0{cwd}", args.join("\0"));
            self.authorize_risky_operation(
                AuthorizationKind::RuntimeExecutor,
                &operation,
                &format!(
                    "allow repository-defined executor: {program} {}",
                    args.join(" ")
                ),
            )?;
        }
        if program.trim().is_empty()
            || program.len() > 512
            || program.contains(['\0', '\n', '\r'])
            || Path::new(program).is_absolute()
            || program
                .split(['/', '\\'])
                .any(|component| component == "..")
        {
            bail!("runtime executor program is invalid or escapes the workspace");
        }
        let executable = if program.contains(['/', '\\']) {
            let executable = self.existing_path(program)?;
            if !executable.is_file() {
                bail!("runtime executor program is not a regular file");
            }
            executable
        } else {
            PathBuf::from(program)
        };
        validate_command_arguments(program, args)?;
        let cwd = self.existing_path(cwd)?;
        if !cwd.is_dir() {
            bail!("runtime executor cwd is not a directory");
        }
        let mut command = Command::new(executable);
        command
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        scrub_sensitive_environment(&mut command, program, args, false);
        let mut child = command
            .spawn()
            .with_context(|| format!("failed to start runtime executor {program}"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow!("runtime executor stdout is unavailable"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow!("runtime executor stderr is unavailable"))?;
        let stdout_task = tokio::spawn(read_bounded_stream(stdout));
        let stderr_task = tokio::spawn(read_bounded_stream(stderr));
        let status = match timeout(
            Duration::from_secs(timeout_seconds.clamp(1, 300)),
            child.wait(),
        )
        .await
        {
            Ok(result) => result?,
            Err(_) => {
                let _ = child.kill().await;
                let _ = child.wait().await;
                let _ = stdout_task.await;
                let _ = stderr_task.await;
                bail!("runtime executor timed out and was terminated");
            }
        };
        let (stdout, stdout_cut) = stdout_task
            .await
            .map_err(|error| anyhow!("runtime executor stdout reader failed: {error}"))??;
        let (stderr, stderr_cut) = stderr_task
            .await
            .map_err(|error| anyhow!("runtime executor stderr reader failed: {error}"))??;
        let (stdout, stdout_redacted) = redact_sensitive_text(&stdout);
        let (stderr, stderr_redacted) = redact_sensitive_text(&stderr);
        Ok(CommandResult {
            program: program.to_owned(),
            args: args.to_vec(),
            exit_code: status.code(),
            success: status.success(),
            stdout,
            stderr,
            truncated: stdout_cut || stderr_cut,
            redacted: stdout_redacted || stderr_redacted,
        })
    }
}

pub(crate) fn redact_sensitive_text(text: &str) -> (String, bool) {
    let mut redacted_any = false;
    let mut in_private_key = false;
    let mut output = Vec::new();
    for line in text.lines() {
        let upper = line.to_ascii_uppercase();
        if upper.contains("-----BEGIN") && upper.contains("PRIVATE KEY") {
            in_private_key = true;
            redacted_any = true;
            output.push("[REDACTED PRIVATE KEY]".to_owned());
            continue;
        }
        if in_private_key {
            redacted_any = true;
            if upper.contains("-----END") && upper.contains("PRIVATE KEY") {
                in_private_key = false;
            }
            continue;
        }
        let (safe, redacted) = redact_sensitive_line(line);
        redacted_any |= redacted;
        output.push(safe);
    }
    (output.join("\n"), redacted_any)
}

fn redact_sensitive_line(line: &str) -> (String, bool) {
    let sensitive = [
        "api_key",
        "apikey",
        "access_token",
        "auth_token",
        "token",
        "secret",
        "password",
        "passwd",
        "client_secret",
        "private_key",
    ];
    let lower = line.to_ascii_lowercase();
    let Some(separator) = line.find('=').or_else(|| line.find(':')) else {
        return (line.to_owned(), false);
    };
    let key_side = &lower[..separator.min(lower.len())];
    if !sensitive.iter().any(|needle| key_side.contains(needle)) {
        return (line.to_owned(), false);
    }
    let value = line[separator + 1..].trim();
    let looks_literal = value.starts_with('"')
        || value.starts_with('\'')
        || value.starts_with('`')
        || (!value.is_empty() && !value.contains(char::is_whitespace));
    if !looks_literal {
        return (line.to_owned(), false);
    }
    (
        format!(
            "{}{} [REDACTED]",
            &line[..separator],
            &line[separator..=separator]
        ),
        true,
    )
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_path_traversal_and_stale_writes() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("demo.txt"), "hello world\n").unwrap();
        let workspace = Workspace::new(dir.path(), true, false).unwrap();
        assert!(workspace.read_file("../secret", 1, None).is_err());
        assert!(workspace
            .replace_text("demo.txt", "hello", "hi", "bad-hash")
            .is_err());
        let view = workspace.read_file("demo.txt", 1, None).unwrap();
        workspace
            .replace_text("demo.txt", "hello", "hi", &view.sha256)
            .unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("demo.txt")).unwrap(),
            "hi world\n"
        );
    }

    #[test]
    fn write_lock_registry_prunes_inactive_paths() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(dir.path(), true, false).unwrap();
        let first_path = dir.path().join("first.txt");
        let second_path = dir.path().join("second.txt");

        let first = workspace.write_lock_for(&first_path).unwrap();
        assert_eq!(workspace.write_locks.lock().unwrap().len(), 1);
        drop(first);

        let second = workspace.write_lock_for(&second_path).unwrap();
        let locks = workspace.write_locks.lock().unwrap();
        assert_eq!(locks.len(), 1);
        assert!(locks.contains_key(&second_path));
        drop(locks);
        drop(second);
    }

    #[test]
    fn list_files_exposes_workspace_files_but_search_skips_noise_and_secrets() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".idea")).unwrap();
        fs::write(dir.path().join(".idea/workspace.xml"), "private IDE state").unwrap();
        fs::write(dir.path().join(".env"), "TOKEN=secret").unwrap();
        fs::write(dir.path().join("server.log"), "noise").unwrap();
        fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();

        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        assert_eq!(
            workspace.list_files(".", 100).unwrap(),
            vec![".idea/workspace.xml", "main.rs", "server.log"]
        );
        assert!(workspace.search("secret", ".", 100).unwrap().is_empty());
    }

    #[test]
    fn model_facing_paths_use_forward_slashes() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("nested").join("deeper");
        fs::create_dir_all(&nested).unwrap();
        fs::write(nested.join("main.rs"), "fn needle() {}\n").unwrap();

        let workspace = Workspace::new(dir.path(), true, false).unwrap();
        let expected = "nested/deeper/main.rs";

        assert_eq!(workspace.list_files(".", 100).unwrap(), vec![expected]);
        assert_eq!(
            workspace.read_file(expected, 1, None).unwrap().path,
            expected
        );
        assert_eq!(
            workspace.search("needle", ".", 10).unwrap()[0]["path"],
            expected
        );
        let (source_files, truncated) = workspace.source_files(".", 100).unwrap();
        assert!(!truncated);
        assert_eq!(source_files, vec![expected]);

        let created = workspace
            .create_file("nested/deeper/created.rs", "fn created() {}\n")
            .unwrap();
        assert_eq!(created.path, "nested/deeper/created.rs");
    }

    #[test]
    fn rejects_overlapping_workspaces_by_default() {
        let root = tempfile::tempdir().unwrap();
        let child = root.path().join("child");
        fs::create_dir(&child).unwrap();

        assert!(Workspaces::new([root.path(), child.as_path()], false, false).is_err());

        let security = WorkspaceSecurity {
            allow_overlapping_workspaces: true,
            ..WorkspaceSecurity::default()
        };
        assert!(Workspaces::new_with_security(
            [root.path(), child.as_path()],
            false,
            false,
            security,
        )
        .is_ok());
    }

    #[test]
    fn blocks_protected_paths_and_destructive_replacements() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".env"), "TOKEN=secret\n").unwrap();
        fs::write(dir.path().join(".env.example"), "TOKEN=example\n").unwrap();
        let original = "x".repeat(10_000);
        fs::write(dir.path().join("large.txt"), &original).unwrap();
        let workspace = Workspace::new(dir.path(), true, false).unwrap();

        assert!(workspace.read_file(".env", 1, None).is_err());
        assert!(workspace.read_file(".env.example", 1, None).is_ok());
        let view = workspace.read_file("large.txt", 1, None).unwrap();
        assert!(workspace
            .replace_text("large.txt", &original, "small", &view.sha256)
            .is_err());

        let security = WorkspaceSecurity {
            allow_destructive_writes: true,
            ..WorkspaceSecurity::default()
        };
        let permissive = Workspace::new_with_security(dir.path(), true, false, security).unwrap();
        permissive
            .replace_text("large.txt", &original, "small", &view.sha256)
            .unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("large.txt")).unwrap(),
            "small"
        );
    }

    #[test]
    fn coding_primitives_create_write_edit_move_and_batch_safely() {
        let dir = tempfile::tempdir().unwrap();
        let workspace = Workspace::new(dir.path(), true, false).unwrap();

        let created = workspace.create_directory("src/domain/models").unwrap();
        assert!(created.created);
        let initial = workspace
            .write_file("src/domain/models/user.rs", "alpha beta gamma\n", None)
            .unwrap();
        let _edited = workspace
            .apply_edits(
                "src/domain/models/user.rs",
                &[
                    TextEdit {
                        old_text: "alpha".into(),
                        new_text: "ALPHA".into(),
                        start_line: None,
                        end_line: None,
                    },
                    TextEdit {
                        old_text: "gamma".into(),
                        new_text: "GAMMA".into(),
                        start_line: Some(1),
                        end_line: Some(1),
                    },
                ],
                &initial.sha256_after,
            )
            .unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("src/domain/models/user.rs")).unwrap(),
            "ALPHA beta GAMMA\n"
        );

        let source_sha = workspace
            .path_info("src/domain/models/user.rs")
            .unwrap()
            .sha256
            .unwrap();
        workspace
            .move_path_checked(
                "src/domain/models/user.rs",
                "src/domain/user.rs",
                Some(&source_sha),
            )
            .unwrap();

        workspace
            .create_files(&[
                CreateFileRequest {
                    path: "src/domain/a.rs".into(),
                    content: "a\n".into(),
                },
                CreateFileRequest {
                    path: "src/domain/b.rs".into(),
                    content: "b\n".into(),
                },
            ])
            .unwrap();
        let a = workspace.read_file("src/domain/a.rs", 1, None).unwrap();
        let b = workspace.read_file("src/domain/b.rs", 1, None).unwrap();
        let edited = workspace
            .apply_file_edits(&[
                FileEditRequest {
                    path: "src/domain/a.rs".into(),
                    expected_sha256: a.sha256,
                    edits: vec![TextEdit {
                        old_text: "a".into(),
                        new_text: "A".into(),
                        start_line: Some(1),
                        end_line: Some(1),
                    }],
                },
                FileEditRequest {
                    path: "src/domain/b.rs".into(),
                    expected_sha256: b.sha256,
                    edits: vec![TextEdit {
                        old_text: "b".into(),
                        new_text: "B".into(),
                        start_line: None,
                        end_line: None,
                    }],
                },
            ])
            .unwrap();
        assert!(edited.iter().all(|item| item.ok));
        let moved = workspace
            .move_paths(&[
                MovePathRequest {
                    source: "src/domain/a.rs".into(),
                    destination: "src/domain/a_model.rs".into(),
                    expected_source_sha256: None,
                },
                MovePathRequest {
                    source: "src/domain/b.rs".into(),
                    destination: "src/domain/b_model.rs".into(),
                    expected_source_sha256: None,
                },
            ])
            .unwrap();
        assert!(moved.iter().all(|item| item.ok));
    }

    #[test]
    fn apply_edits_pin_original_lines_and_reject_overlap_or_stale_revision() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("demo.txt"), "same\nmiddle\nsame\n").unwrap();
        let workspace = Workspace::new(dir.path(), true, false).unwrap();
        let view = workspace.read_file("demo.txt", 1, None).unwrap();
        workspace
            .apply_edits(
                "demo.txt",
                &[TextEdit {
                    old_text: "same".into(),
                    new_text: "FIRST".into(),
                    start_line: Some(1),
                    end_line: Some(1),
                }],
                &view.sha256,
            )
            .unwrap();
        assert_eq!(
            fs::read_to_string(dir.path().join("demo.txt")).unwrap(),
            "FIRST\nmiddle\nsame\n"
        );

        let view = workspace.read_file("demo.txt", 1, None).unwrap();
        assert!(workspace
            .apply_edits(
                "demo.txt",
                &[
                    TextEdit {
                        old_text: "FIRST\nmiddle".into(),
                        new_text: "x".into(),
                        start_line: Some(1),
                        end_line: Some(2),
                    },
                    TextEdit {
                        old_text: "middle\nsame".into(),
                        new_text: "y".into(),
                        start_line: Some(2),
                        end_line: Some(3),
                    },
                ],
                &view.sha256,
            )
            .is_err());
        assert!(workspace
            .apply_edits(
                "demo.txt",
                &[TextEdit {
                    old_text: "FIRST".into(),
                    new_text: "x".into(),
                    start_line: Some(1),
                    end_line: Some(1),
                }],
                "stale",
            )
            .is_err());
    }

    #[test]
    fn delete_path_requires_one_shot_human_authorization() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("obsolete.txt"), "remove me\n").unwrap();
        let workspace = Workspace::new(dir.path(), true, false).unwrap();
        let sha = workspace.path_info("obsolete.txt").unwrap().sha256.unwrap();

        let first = workspace
            .delete_path("obsolete.txt", Some(&sha))
            .unwrap_err();
        assert!(first.to_string().contains("authorization required"));
        let request = workspace.authorization.latest_pending().unwrap();
        assert_eq!(request.kind, AuthorizationKind::DestructiveDelete);
        assert!(workspace.authorization.approve_session(&request.id));
        workspace.delete_path("obsolete.txt", Some(&sha)).unwrap();
        assert!(!dir.path().join("obsolete.txt").exists());

        fs::write(dir.path().join("obsolete.txt"), "remove me\n").unwrap();
        assert!(workspace.delete_path("obsolete.txt", Some(&sha)).is_err());
        fs::create_dir(dir.path().join("nonempty")).unwrap();
        fs::write(dir.path().join("nonempty/file.txt"), "x").unwrap();
        assert!(workspace.delete_path("nonempty", None).is_err());
    }

    #[test]
    fn command_policy_allows_standard_cargo_verification_but_blocks_arbitrary_execution() {
        let safe = WorkspaceSecurity::default();
        assert!(validate_command_policy("git", &["status".to_owned()], safe).is_ok());
        assert!(
            validate_command_policy("git", &["clean".to_owned(), "-fd".to_owned()], safe).is_err()
        );
        assert!(validate_command_policy(
            "python3",
            &["-c".to_owned(), "print('unsafe')".to_owned()],
            safe,
        )
        .is_err());
        assert!(validate_command_policy("cargo", &["fmt".to_owned()], safe).is_ok());
        assert!(
            validate_command_policy("cargo", &["fmt".to_owned(), "--check".to_owned()], safe,)
                .is_ok()
        );
        assert!(validate_command_policy("cargo", &["check".to_owned()], safe).is_ok());
        assert!(validate_command_policy(
            "cargo",
            &["check".to_owned(), "--locked".to_owned()],
            safe,
        )
        .is_ok());
        assert!(validate_command_policy("cargo", &["test".to_owned()], safe).is_ok());
        assert!(validate_command_policy(
            "cargo",
            &["test".to_owned(), "--locked".to_owned()],
            safe,
        )
        .is_ok());
        assert!(validate_command_policy(
            "cargo",
            &[
                "clippy".to_owned(),
                "--locked".to_owned(),
                "--".to_owned(),
                "-D".to_owned(),
                "warnings".to_owned(),
            ],
            safe,
        )
        .is_ok());
        assert!(validate_command_policy(
            "cargo",
            &[
                "build".to_owned(),
                "--release".to_owned(),
                "--locked".to_owned()
            ],
            safe,
        )
        .is_ok());
        assert!(validate_command_policy(
            "cargo",
            &["check".to_owned(), "--workspace".to_owned()],
            safe,
        )
        .is_err());
        assert!(validate_command_policy("cargo", &["metadata".to_owned()], safe).is_err());
        assert!(validate_command_policy("go", &["list".to_owned()], safe).is_err());
        assert!(validate_command_policy("npm", &["list".to_owned()], safe).is_err());
        assert!(
            validate_command_policy("rg", &["needle".to_owned(), "--hidden".to_owned()], safe,)
                .is_err()
        );
        for arguments in [
            vec!["-f".to_owned(), ".env".to_owned()],
            vec!["--file".to_owned(), ".env".to_owned()],
            vec!["--ignore-file".to_owned(), ".env".to_owned()],
            vec!["--glob".to_owned(), ".env".to_owned(), "needle".to_owned()],
            vec!["--type-add=secret:*.env".to_owned(), "needle".to_owned()],
        ] {
            assert!(
                validate_command_policy("rg", &arguments, safe).is_err(),
                "ripgrep helper/file-selection bypass was accepted: {arguments:?}"
            );
        }
        assert!(
            validate_command_policy("git", &["show".to_owned(), "HEAD:.env".to_owned()], safe,)
                .is_err()
        );
        assert!(validate_command_policy(
            "git",
            &["log".to_owned(), "--show-signature".to_owned()],
            safe,
        )
        .is_err());
        assert!(validate_command_policy(
            "git",
            &["log".to_owned(), "--format=%G?".to_owned()],
            safe,
        )
        .is_err());
        assert!(
            validate_command_policy("rg", &["TOKEN".to_owned(), ".env".to_owned()], safe,).is_err()
        );

        let trusted = WorkspaceSecurity {
            allow_risky_exec: true,
            ..WorkspaceSecurity::default()
        };
        assert!(validate_command_policy("cargo", &["test".to_owned()], trusted).is_ok());
        assert!(validate_command_policy("cargo", &["metadata".to_owned()], trusted).is_ok());
        assert!(validate_command_policy("go", &["list".to_owned()], trusted).is_ok());
        assert!(validate_command_policy("npm", &["list".to_owned()], trusted).is_ok());
        assert!(validate_command_policy(
            "cargo",
            &[
                "metadata".to_owned(),
                "--config".to_owned(),
                "build.rustc-wrapper=tool".to_owned(),
            ],
            trusted,
        )
        .is_err());
        assert!(validate_command_policy(
            "go",
            &["list".to_owned(), "-C".to_owned(), "subdir".to_owned()],
            trusted,
        )
        .is_err());
        assert!(validate_command_policy(
            "npm",
            &[
                "list".to_owned(),
                "--prefix".to_owned(),
                "subdir".to_owned()
            ],
            trusted,
        )
        .is_err());
        assert!(validate_command_policy("rustc", &["@args.txt".to_owned()], trusted).is_err());
    }

    #[test]
    fn git_arguments_disable_repository_helpers_and_external_config_paths() {
        let status = hardened_command_args("git", &["status".to_owned(), "--short".to_owned()]);
        assert!(status
            .windows(2)
            .any(|pair| pair[0] == "-c" && pair[1] == "core.fsmonitor=false"));
        assert!(status.iter().any(|arg| arg.starts_with("core.hooksPath=")));
        assert!(status
            .iter()
            .any(|arg| arg.starts_with("core.attributesFile=")));
        assert!(status
            .iter()
            .any(|arg| arg.starts_with("core.excludesFile=")));
        for blocked_helper in [
            "credential.helper=",
            "core.askPass=",
            "core.sshCommand=false",
            "core.gitProxy=",
            "http.extraHeader=",
        ] {
            assert!(status.iter().any(|arg| arg == blocked_helper));
        }
        assert_eq!(
            status
                .get(status.len().saturating_sub(2))
                .map(String::as_str),
            Some("status")
        );
        assert_eq!(status.last().map(String::as_str), Some("--short"));

        let diff = hardened_command_args("git", &["diff".to_owned(), "--cached".to_owned()]);
        let subcommand = diff.iter().position(|arg| arg == "diff").unwrap();
        assert_eq!(diff[subcommand + 1], "--no-ext-diff");
        assert_eq!(diff[subcommand + 2], "--no-textconv");
        assert_eq!(diff.last().map(String::as_str), Some("--cached"));

        let push = hardened_command_args(
            "git",
            &[
                "push".to_owned(),
                "origin".to_owned(),
                "HEAD:main".to_owned(),
            ],
        );
        assert!(push.iter().any(|arg| {
            arg == "core.sshCommand=ssh -oBatchMode=yes -oStrictHostKeyChecking=accept-new"
        }));
        assert!(!push.iter().any(|arg| arg == "core.sshCommand=false"));
        assert!(push.iter().any(|arg| arg == "credential.helper="));

        let lfs_push = hardened_command_args(
            "git",
            &[
                "lfs".to_owned(),
                "push".to_owned(),
                "origin".to_owned(),
                "main".to_owned(),
            ],
        );
        assert!(lfs_push.iter().any(|arg| {
            arg == "core.sshCommand=ssh -oBatchMode=yes -oStrictHostKeyChecking=accept-new"
        }));
        assert!(!lfs_push.iter().any(|arg| arg == "core.sshCommand=false"));
    }

    #[test]
    fn verification_policy_allows_only_inferred_quality_shapes() {
        assert!(validate_verification_command_shape(
            "cargo",
            &["check".to_owned(), "--locked".to_owned()],
        )
        .is_ok());
        assert!(validate_verification_command_shape(
            "cargo",
            &[
                "clippy".to_owned(),
                "--locked".to_owned(),
                "--".to_owned(),
                "-D".to_owned(),
                "warnings".to_owned(),
            ],
        )
        .is_ok());
        assert!(validate_verification_command_shape(
            "cargo",
            &[
                "build".to_owned(),
                "--release".to_owned(),
                "--locked".to_owned(),
            ],
        )
        .is_ok());
        assert!(validate_verification_command_shape(
            "cargo",
            &[
                "nextest".to_owned(),
                "run".to_owned(),
                "--locked".to_owned(),
            ],
        )
        .is_ok());
        assert!(validate_verification_command_shape(
            "cargo",
            &[
                "nextest".to_owned(),
                "run".to_owned(),
                "name(test)".to_owned(),
            ],
        )
        .is_err());
        assert!(validate_verification_command_shape(
            "pnpm",
            &["run".to_owned(), "typecheck".to_owned()],
        )
        .is_ok());
        assert!(validate_verification_command_shape(
            "go",
            &["test".to_owned(), "./...".to_owned()],
        )
        .is_ok());
        assert!(validate_verification_command_shape("cargo", &["run".to_owned()]).is_err());
        assert!(validate_verification_command_shape(
            "npm",
            &["run".to_owned(), "postinstall".to_owned()],
        )
        .is_err());
        assert!(validate_verification_command_shape(
            "python3",
            &["-c".to_owned(), "print('no')".to_owned()],
        )
        .is_err());
    }

    #[tokio::test]
    async fn bounded_direct_and_harness_checks_run_without_risky_exec() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("Cargo.toml"),
            "[package]\nname='demo'\nversion='0.1.0'\nedition='2021'\n",
        )
        .unwrap();
        fs::write(
            dir.path().join("src/lib.rs"),
            "pub fn value() -> u8 { 1 }\n",
        )
        .unwrap();
        let workspace = Workspace::new(dir.path(), false, true).unwrap();

        let direct = workspace
            .run_command("cargo", &["check".to_owned()], ".", 30)
            .await
            .expect("bounded direct cargo check should not require risky-exec");
        assert!(direct.success, "cargo check failed: {}", direct.stderr);

        let verified = workspace
            .run_verification_command("cargo", &["check".to_owned()], ".", 30)
            .await
            .expect("exact Harness verification shape may run without the global risky flag");
        assert!(verified.success, "cargo check failed: {}", verified.stderr);
    }

    #[tokio::test]
    async fn trusted_runtime_executor_requires_explicit_repository_trust() {
        let dir = tempfile::tempdir().unwrap();
        let blocked = Workspace::new(dir.path(), false, true).unwrap();
        let error = blocked
            .run_trusted_runtime_command("rustc", &["--version".to_owned()], ".", 10)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("authorization required"));
        let request = blocked.authorization.latest_pending().unwrap();
        assert_eq!(request.kind, AuthorizationKind::RuntimeExecutor);
        assert!(blocked.authorization.approve_session(&request.id));
        let approved = blocked
            .run_trusted_runtime_command("rustc", &["--version".to_owned()], ".", 10)
            .await
            .unwrap();
        assert!(approved.success);

        let trusted = Workspace::new_with_security(
            dir.path(),
            false,
            true,
            WorkspaceSecurity {
                allow_risky_exec: true,
                ..WorkspaceSecurity::default()
            },
        )
        .unwrap();
        let result = trusted
            .run_trusted_runtime_command("rustc", &["--version".to_owned()], ".", 10)
            .await
            .unwrap();
        assert!(result.success);
    }

    #[tokio::test]
    async fn revoked_catalog_command_becomes_selectively_authorizable() {
        let dir = tempfile::tempdir().unwrap();
        let workspaces = Workspaces::new([dir.path()], true, true).unwrap();
        let workspace_id = workspaces.default_id().to_owned();
        workspaces
            .revoke_command(Some(&workspace_id), "git")
            .unwrap();
        let (_, workspace) = workspaces.select(Some(&workspace_id)).unwrap();

        let error = workspace
            .run_command("git", &["status".to_owned()], ".", 10)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("authorization required"));
        let request = workspaces.latest_pending_authorization().unwrap();
        assert_eq!(request.kind, AuthorizationKind::CommandAccess);
        assert_eq!(request.workspace, workspace_id);
        assert_eq!(request.program.as_deref(), Some("git"));
        assert!(workspaces.approve_authorization_session(&request.id));
        assert!(workspace
            .allowed_commands()
            .iter()
            .any(|command| command == "git"));

        workspaces
            .revoke_command(Some(&workspace_id), "git")
            .unwrap();
        let denied = workspace
            .run_command("git", &["status".to_owned()], ".", 10)
            .await
            .unwrap_err();
        assert!(denied.to_string().contains("authorization required"));
        let request = workspaces.latest_pending_authorization().unwrap();
        assert!(workspaces.deny_authorization(&request.id));
        assert!(!workspace
            .allowed_commands()
            .iter()
            .any(|command| command == "git"));

        let arbitrary = workspace
            .run_command("hugo", &["--version".to_owned()], ".", 10)
            .await
            .unwrap_err();
        assert!(arbitrary.to_string().contains("authorization required"));
        let request = workspaces.latest_pending_authorization().unwrap();
        assert_eq!(request.kind, AuthorizationKind::CommandAccess);
        assert_eq!(request.program.as_deref(), Some("hugo"));
        assert!(workspaces.approve_authorization_session(&request.id));
        assert!(workspace
            .allowed_commands()
            .iter()
            .any(|command| command == "hugo"));

        let hard_denied = workspace
            .run_command("bash", &["-lc".to_owned(), "echo no".to_owned()], ".", 10)
            .await
            .unwrap_err();
        assert!(hard_denied.to_string().contains("no-shell"));
        assert!(workspaces.latest_pending_authorization().is_none());
    }

    #[test]
    fn workspace_command_allowlist_supports_defaults_and_operator_authorized_programs() {
        let dir = tempfile::tempdir().unwrap();
        let workspaces = Workspaces::new([dir.path()], true, true).unwrap();
        let workspace_id = workspaces.default_id().to_owned();
        let (_, selected_before) = workspaces.select(Some(&workspace_id)).unwrap();
        assert!(selected_before
            .allowed_commands()
            .iter()
            .any(|command| command == "git"));

        let revoked = workspaces
            .revoke_command(Some(&workspace_id), "git")
            .unwrap();
        assert_eq!(revoked["changed"].as_bool(), Some(true));
        assert!(!selected_before
            .allowed_commands()
            .iter()
            .any(|command| command == "git"));
        assert!(selected_before
            .available_commands()
            .iter()
            .any(|command| command == "git"));

        let restored = workspaces
            .allow_command(Some(&workspace_id), "git")
            .unwrap();
        assert_eq!(restored["changed"].as_bool(), Some(true));
        assert!(selected_before
            .allowed_commands()
            .iter()
            .any(|command| command == "git"));
        assert!(workspaces
            .allow_command(Some(&workspace_id), "hugo")
            .is_ok());
        assert!(selected_before
            .allowed_commands()
            .iter()
            .any(|command| command == "hugo"));
        assert!(workspaces
            .allow_command(Some(&workspace_id), "bash")
            .is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_workspace_root_replaced_at_same_path() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("workspace");
        let old_root = parent.path().join("workspace-old");
        fs::create_dir(&root).unwrap();
        fs::write(root.join("before.txt"), "before\n").unwrap();
        let workspace = Workspace::new(&root, false, false).unwrap();

        fs::rename(&root, &old_root).unwrap();
        fs::create_dir(&root).unwrap();
        fs::write(root.join("after.txt"), "after\n").unwrap();

        assert!(workspace.read_file("after.txt", 1, None).is_err());
    }

    #[cfg(unix)]
    #[test]
    fn blocks_symlink_and_hardlink_aliases() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("real.txt"), "hello\n").unwrap();
        symlink("real.txt", dir.path().join("alias.txt")).unwrap();
        fs::hard_link(dir.path().join("real.txt"), dir.path().join("hard.txt")).unwrap();
        let workspace = Workspace::new(dir.path(), true, false).unwrap();

        assert!(workspace.read_file("alias.txt", 1, None).is_err());
        let view = workspace.read_file("real.txt", 1, None).unwrap();
        assert!(workspace
            .replace_text("real.txt", "hello", "hi", &view.sha256)
            .is_err());
    }

    #[test]
    fn redacts_high_confidence_secrets_from_model_context() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config.txt"),
            "endpoint=https://example.com\napi_key=super-secret-value\npassword = \"hunter2\"\n",
        )
        .unwrap();
        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        let view = workspace.read_file("config.txt", 1, None).unwrap();
        assert!(view.redacted);
        assert!(view.content.contains("api_key= [REDACTED]"));
        assert!(view.content.contains("password = [REDACTED]"));
        assert!(!view.content.contains("super-secret-value"));
        assert!(!view.content.contains("hunter2"));
    }
}
