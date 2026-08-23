use anyhow::{anyhow, bail, Context, Result};
use memchr::memmem;
use rayon::prelude::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
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
    commands: HashSet<String>,
    write_locks: Arc<Mutex<HashMap<PathBuf, Weak<Mutex<()>>>>>,
}

#[derive(Clone)]
pub struct Workspaces {
    roots: Arc<Vec<WorkspaceRoot>>,
    default_id: String,
    security: WorkspaceSecurity,
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

impl Workspaces {
    #[cfg(test)]
    pub fn new<I, P>(roots: I, allow_write: bool, allow_exec: bool) -> Result<Self>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        Self::new_with_security(roots, allow_write, allow_exec, WorkspaceSecurity::default())
    }

    pub fn new_with_security<I, P>(
        roots: I,
        allow_write: bool,
        allow_exec: bool,
        security: WorkspaceSecurity,
    ) -> Result<Self>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut entries = Vec::<WorkspaceRoot>::new();
        let mut used_ids = HashMap::<String, usize>::new();
        let mut seen_roots = HashSet::<PathBuf>::new();

        for root in roots {
            if entries.len() >= MAX_WORKSPACES {
                bail!("at most {MAX_WORKSPACES} workspaces may be exposed by one process");
            }
            let workspace = Workspace::new_with_security(root, allow_write, allow_exec, security)?;
            if !seen_roots.insert(workspace.root.clone()) {
                continue;
            }
            if !security.allow_overlapping_workspaces {
                if let Some(existing) = entries.iter().find(|existing| {
                    workspace.root.starts_with(&existing.workspace.root)
                        || existing.workspace.root.starts_with(&workspace.root)
                }) {
                    bail!(
                        "overlapping workspace roots are blocked: {} and {}; expose only the narrow roots you need or restart with --allow-overlapping-workspaces",
                        existing.workspace.root.display(),
                        workspace.root.display()
                    );
                }
            }
            let base_id = workspace_id(&workspace.root);
            let count = used_ids.entry(base_id.clone()).or_insert(0);
            *count += 1;
            let id = if *count == 1 {
                base_id
            } else {
                format!("{base_id}-{}", *count)
            };
            entries.push(WorkspaceRoot { id, workspace });
        }

        let Some(first) = entries.first() else {
            bail!("at least one workspace is required");
        };
        Ok(Self {
            default_id: first.id.clone(),
            roots: Arc::new(entries),
            security,
        })
    }

    pub fn default_id(&self) -> &str {
        &self.default_id
    }

    pub fn select(&self, id: Option<&str>) -> Result<(String, Workspace)> {
        let id = id.unwrap_or(&self.default_id);
        let root = self
            .roots
            .iter()
            .find(|root| root.id == id)
            .ok_or_else(|| anyhow!("unknown workspace: {id}"))?;
        Ok((root.id.clone(), root.workspace.clone()))
    }

    pub fn capabilities(&self) -> serde_json::Value {
        serde_json::json!({
            "default_workspace": self.default_id,
            "security": {
                "delete_tool_exposed": false,
                "symlink_paths": "blocked",
                "protected_paths": "blocked",
                "overlapping_workspaces": self.security.allow_overlapping_workspaces,
                "broad_workspace_roots": self.security.allow_broad_workspace,
                "risky_exec_enabled": self.security.allow_risky_exec,
                "destructive_writes_enabled": self.security.allow_destructive_writes,
                "max_write_bytes": MAX_WRITE_BYTES,
            },
            "workspaces": self.roots.iter().map(|root| serde_json::json!({
                "id": root.id,
                "root": root.workspace.root,
                "write_enabled": root.workspace.allow_write,
                "exec_enabled": root.workspace.allow_exec,
                "risky_exec_enabled": root.workspace.security.allow_risky_exec,
                "destructive_writes_enabled": root.workspace.security.allow_destructive_writes,
                "allowed_commands": root.workspace.commands,
            })).collect::<Vec<_>>(),
        })
    }

    pub fn roots(&self) -> Vec<(String, PathBuf)> {
        self.roots
            .iter()
            .map(|root| (root.id.clone(), root.workspace.root.clone()))
            .collect()
    }
}

impl Workspace {
    #[cfg(test)]
    pub fn new(root: impl AsRef<Path>, allow_write: bool, allow_exec: bool) -> Result<Self> {
        Self::new_with_security(root, allow_write, allow_exec, WorkspaceSecurity::default())
    }

    pub fn new_with_security(
        root: impl AsRef<Path>,
        allow_write: bool,
        allow_exec: bool,
        security: WorkspaceSecurity,
    ) -> Result<Self> {
        let root = root
            .as_ref()
            .canonicalize()
            .with_context(|| format!("workspace does not exist: {}", root.as_ref().display()))?;
        if !root.is_dir() {
            bail!("workspace is not a directory: {}", root.display());
        }
        validate_workspace_root(&root, security)?;
        let root_identity = root_identity(&root)?;
        let commands = [
            "cargo", "rustc", "git", "rg", "npm", "pnpm", "yarn", "bun", "node", "python3",
            "pytest", "go", "make",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        Ok(Self {
            root,
            root_identity,
            allow_write,
            allow_exec,
            security,
            commands,
            write_locks: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn write_enabled(&self) -> bool {
        self.allow_write
    }

    pub(crate) fn exec_enabled(&self) -> bool {
        self.allow_exec
    }

    pub(crate) fn source_stamp(&self, path: &str) -> Result<SourceStamp> {
        let file = self.existing_path(path)?;
        let metadata = fs::metadata(&file)?;
        validate_source_metadata(&metadata)?;
        Ok(source_stamp(&metadata))
    }

    pub(crate) fn load_source(&self, path: &str) -> Result<SourceDocument> {
        let file = self.existing_path(path)?;
        let metadata_before = fs::metadata(&file)?;
        validate_source_metadata(&metadata_before)?;
        let stamp_before = source_stamp(&metadata_before);
        let content = fs::read_to_string(&file).context("file is not valid UTF-8 text")?;
        let metadata_after = fs::metadata(&file)?;
        validate_source_metadata(&metadata_after)?;
        let stamp_after = source_stamp(&metadata_after);
        if stamp_before != stamp_after {
            bail!("source file changed while it was being read; retry the request");
        }
        Ok(SourceDocument {
            path: file.strip_prefix(&self.root)?.to_string_lossy().to_string(),
            sha256: sha256(content.as_bytes()),
            content,
            stamp: stamp_after,
        })
    }

    pub(crate) fn source_files(
        &self,
        path: &str,
        max_entries: usize,
    ) -> Result<(Vec<String>, bool)> {
        let start = self.existing_path(path)?;
        if start.is_file() {
            let relative = start
                .strip_prefix(&self.root)?
                .to_string_lossy()
                .to_string();
            return Ok((vec![relative], false));
        }
        if !start.is_dir() {
            bail!("path is not a file or directory");
        }

        let limit = max_entries.clamp(1, 50_000);
        let mut files = Vec::new();
        let mut truncated = false;
        for entry in WalkDir::new(start)
            .follow_links(false)
            .into_iter()
            .filter_entry(visible_entry)
            .filter_map(|entry| entry.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            if entry
                .metadata()
                .map(|metadata| metadata.len() > MAX_READ_BYTES)
                .unwrap_or(true)
            {
                continue;
            }
            if files.len() == limit {
                truncated = true;
                break;
            }
            files.push(
                entry
                    .path()
                    .strip_prefix(&self.root)?
                    .to_string_lossy()
                    .to_string(),
            );
        }
        files.sort();
        Ok((files, truncated))
    }

    fn validate_relative(path: &str) -> Result<PathBuf> {
        if path.contains('\0') || path.contains(['\n', '\r']) {
            bail!("path contains forbidden control characters");
        }
        if path.trim().is_empty() || path == "." {
            return Ok(PathBuf::new());
        }
        let candidate = PathBuf::from(path);
        if candidate.is_absolute() {
            bail!("absolute paths are not allowed");
        }
        for component in candidate.components() {
            if matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            ) {
                bail!("path traversal is not allowed");
            }
            if let Component::Normal(value) = component {
                let value = value.to_string_lossy();
                if value.contains(':') {
                    bail!("alternate data streams and colon-bearing path components are blocked");
                }
            }
        }
        reject_protected_path(&candidate)?;
        Ok(candidate)
    }

    fn ensure_root_intact(&self) -> Result<()> {
        let current = self
            .root
            .canonicalize()
            .context("workspace root is no longer accessible")?;
        let identity = root_identity(&current)?;
        if current != self.root || identity != self.root_identity {
            bail!("workspace root identity changed after startup; restart wcode");
        }
        Ok(())
    }

    fn ensure_no_symlink_components(
        &self,
        relative: &Path,
        allow_missing_leaf: bool,
    ) -> Result<()> {
        let mut current = self.root.clone();
        let components = relative.components().collect::<Vec<_>>();
        for (index, component) in components.iter().enumerate() {
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
                Ok(_) => {}
                Err(error)
                    if error.kind() == std::io::ErrorKind::NotFound
                        && allow_missing_leaf
                        && index + 1 == components.len() =>
                {
                    return Ok(())
                }
                Err(error) => return Err(error.into()),
            }
        }
        Ok(())
    }

    fn existing_path(&self, path: &str) -> Result<PathBuf> {
        self.ensure_root_intact()?;
        let relative = Self::validate_relative(path)?;
        self.ensure_no_symlink_components(&relative, false)?;
        let resolved = self
            .root
            .join(relative)
            .canonicalize()
            .with_context(|| format!("path not found: {path}"))?;
        if !resolved.starts_with(&self.root) {
            bail!("path escapes workspace");
        }
        Ok(resolved)
    }

    fn new_path(&self, path: &str) -> Result<PathBuf> {
        self.ensure_root_intact()?;
        let relative = Self::validate_relative(path)?;
        if relative.as_os_str().is_empty() {
            bail!("file path is required");
        }
        self.ensure_no_symlink_components(&relative, true)?;
        let target = self.root.join(&relative);
        let parent = target
            .parent()
            .ok_or_else(|| anyhow!("invalid target path"))?;
        let resolved_parent = parent
            .canonicalize()
            .with_context(|| format!("parent directory not found: {}", parent.display()))?;
        if !resolved_parent.starts_with(&self.root) {
            bail!("path escapes workspace");
        }
        Ok(target)
    }

    fn write_lock_for(&self, path: &Path) -> Result<Arc<Mutex<()>>> {
        let mut locks = self
            .write_locks
            .lock()
            .map_err(|_| anyhow!("workspace write lock registry poisoned"))?;
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(path).and_then(Weak::upgrade) {
            return Ok(lock);
        }
        let lock = Arc::new(Mutex::new(()));
        locks.insert(path.to_path_buf(), Arc::downgrade(&lock));
        Ok(lock)
    }

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
                let relative = entry
                    .path()
                    .strip_prefix(&self.root)?
                    .to_string_lossy()
                    .to_string();
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
        let files = WalkDir::new(start)
            .follow_links(false)
            .into_iter()
            .filter_entry(visible_entry)
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                entry.file_type().is_file()
                    && entry
                        .metadata()
                        .map(|metadata| metadata.len() <= MAX_READ_BYTES)
                        .unwrap_or(false)
            })
            .map(|entry| entry.into_path())
            .collect::<Vec<_>>();

        let found = AtomicUsize::new(0);
        let root = &self.root;
        let mut results = files
            .par_iter()
            .filter_map(|file| {
                if found.load(Ordering::Relaxed) >= limit {
                    return None;
                }
                let bytes = fs::read(file).ok()?;
                if !queries
                    .iter()
                    .any(|query| memmem::find(&bytes, query.as_bytes()).is_some())
                {
                    return None;
                }
                let content = std::str::from_utf8(&bytes).ok()?;
                let relative = file.strip_prefix(root).ok()?.to_string_lossy().to_string();
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
        let lines: Vec<&str> = content.lines().collect();
        let total = lines.len();
        let start = start_line.max(1);
        let end = end_line
            .unwrap_or(start.saturating_add(499))
            .min(total)
            .max(start.saturating_sub(1));
        let selected = if total == 0 || start > total {
            String::new()
        } else {
            lines[start - 1..end].join("\n")
        };
        let (selected, redacted) = redact_sensitive_text(&selected);
        Ok(FileView {
            path: file.strip_prefix(&self.root)?.to_string_lossy().to_string(),
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
            path: file.strip_prefix(&self.root)?.to_string_lossy().to_string(),
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
            path: file.strip_prefix(&self.root)?.to_string_lossy().to_string(),
            sha256_before: None,
            sha256_after: sha256(content.as_bytes()),
            bytes_written: content.len(),
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
        if !self.commands.contains(program) {
            bail!("program is not allowlisted: {program}");
        }
        validate_command_policy(program, args, self.security)?;
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
        scrub_sensitive_environment(&mut command, program);
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

#[cfg(unix)]
fn root_identity(path: &Path) -> Result<RootIdentity> {
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        bail!("workspace root is not a stable directory");
    }
    Ok(RootIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}

#[cfg(not(unix))]
fn root_identity(path: &Path) -> Result<RootIdentity> {
    let canonical = path.canonicalize()?;
    if !canonical.is_dir() {
        bail!("workspace root is not a stable directory");
    }
    Ok(RootIdentity { canonical })
}

fn validate_workspace_root(root: &Path, security: WorkspaceSecurity) -> Result<()> {
    if security.allow_broad_workspace {
        return Ok(());
    }
    if root.parent().is_none() {
        bail!(
            "filesystem roots are too broad to expose as a workspace; choose a project directory or restart with --allow-broad-workspace"
        );
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .and_then(|path| path.canonicalize().ok());
    if home.as_deref() == Some(root) {
        bail!(
            "the user home directory is too broad to expose as a workspace; choose a project directory or restart with --allow-broad-workspace"
        );
    }
    Ok(())
}

fn reject_protected_path(path: &Path) -> Result<()> {
    for component in path.components() {
        let Component::Normal(value) = component else {
            continue;
        };
        let name = value.to_string_lossy().to_ascii_lowercase();
        if matches!(
            name.as_str(),
            ".git"
                | ".hg"
                | ".svn"
                | ".ssh"
                | ".aws"
                | ".gnupg"
                | ".azure"
                | ".kube"
                | ".git-credentials"
                | ".netrc"
                | ".npmrc"
                | ".pypirc"
                | "credentials"
                | "credentials.json"
                | "service-account.json"
                | "id_rsa"
                | "id_dsa"
                | "id_ecdsa"
                | "id_ed25519"
                | "authorized_keys"
        ) {
            bail!("protected credential or repository-control path is not accessible: {name}");
        }
        if name.starts_with(".wcode-") || name == ".wcode-security" {
            bail!("wcode internal paths are not accessible");
        }
        if name == ".env"
            || (name.starts_with(".env.")
                && !name.ends_with(".example")
                && !name.ends_with(".sample")
                && !name.ends_with(".template"))
        {
            bail!("environment secret files are not accessible through MCP tools");
        }
    }
    Ok(())
}

fn validate_write_content(content: &str) -> Result<()> {
    if content.len() > MAX_WRITE_BYTES {
        bail!("write exceeds the {MAX_WRITE_BYTES}-byte safety limit");
    }
    if content.contains('\0') {
        bail!("NUL bytes are not allowed in UTF-8 text writes");
    }
    Ok(())
}

fn reject_destructive_replacement(
    before: &str,
    after: &str,
    security: WorkspaceSecurity,
) -> Result<()> {
    if security.allow_destructive_writes || after.len() >= before.len() {
        return Ok(());
    }
    if !before.trim().is_empty() && after.trim().is_empty() {
        bail!(
            "refusing to empty a non-empty file; restart with --allow-destructive-writes for an intentional destructive replacement"
        );
    }
    let removed = before.len().saturating_sub(after.len());
    let reduction_percent = removed.saturating_mul(100) / before.len().max(1);
    if removed >= MAX_SAFE_REMOVAL_BYTES && reduction_percent >= MAX_SAFE_REDUCTION_PERCENT {
        bail!(
            "refusing a replacement that removes {removed} bytes ({reduction_percent}% of the file); split the edit or restart with --allow-destructive-writes"
        );
    }
    Ok(())
}

#[cfg(unix)]
fn ensure_single_link_file(path: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() {
        bail!("write target is not a regular file");
    }
    if metadata.nlink() > 1 {
        bail!("hard-linked files are blocked from modification to prevent alias-based writes");
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_single_link_file(path: &Path) -> Result<()> {
    if !fs::symlink_metadata(path)?.file_type().is_file() {
        bail!("write target is not a regular file");
    }
    Ok(())
}

fn workspace_id(root: &Path) -> String {
    let raw = root
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("workspace");
    let mut id = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>();
    while id.contains("--") {
        id = id.replace("--", "-");
    }
    let id = id.trim_matches('-');
    if id.is_empty() {
        "workspace".to_owned()
    } else {
        id.to_owned()
    }
}

fn listable_entry(entry: &DirEntry) -> bool {
    if entry.file_type().is_symlink() {
        return false;
    }
    let Some(name) = entry.file_name().to_str() else {
        return false;
    };
    reject_protected_path(Path::new(name)).is_ok()
}

fn visible_entry(entry: &DirEntry) -> bool {
    let Some(name) = entry.file_name().to_str() else {
        return false;
    };
    if reject_protected_path(Path::new(name)).is_err() {
        return false;
    }
    if matches!(
        name,
        ".git"
            | ".idea"
            | ".vscode"
            | "node_modules"
            | "target"
            | ".venv"
            | "__pycache__"
            | ".DS_Store"
    ) {
        return false;
    }
    if name.starts_with(".env")
        || name.ends_with(".log")
        || (name.starts_with(".wcode-") && name.ends_with(".tmp"))
    {
        return false;
    }
    true
}

fn validate_source_metadata(metadata: &fs::Metadata) -> Result<()> {
    if !metadata.is_file() {
        bail!("path is not a file");
    }
    if metadata.len() > MAX_READ_BYTES {
        bail!("file exceeds 1 MiB read limit");
    }
    Ok(())
}

fn source_stamp(metadata: &fs::Metadata) -> SourceStamp {
    let modified_nanos = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    SourceStamp {
        len: metadata.len(),
        modified_nanos,
    }
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn write_temp_file(parent: &Path, content: &[u8]) -> Result<PathBuf> {
    let temp = parent.join(format!(".wcode-{}.tmp", Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)?;
    file.write_all(content)?;
    file.sync_all()?;
    Ok(temp)
}

fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("target has no parent directory"))?;
    let temp = write_temp_file(parent, content)?;
    if let Ok(metadata) = fs::metadata(path) {
        fs::set_permissions(&temp, metadata.permissions())?;
    }
    if let Err(error) = replace_path(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error.into());
    }
    let _ = OpenOptions::new()
        .read(true)
        .open(parent)
        .and_then(|dir| dir.sync_all());
    Ok(())
}

fn atomic_create_new(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("target has no parent directory"))?;
    let temp = write_temp_file(parent, content)?;
    match fs::hard_link(&temp, path) {
        Ok(()) => {
            fs::remove_file(&temp)?;
            let _ = OpenOptions::new()
                .read(true)
                .open(parent)
                .and_then(|dir| dir.sync_all());
            Ok(())
        }
        Err(error) => {
            let _ = fs::remove_file(&temp);
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                bail!("file already exists; use replace_text for existing files");
            }
            Err(error.into())
        }
    }
}

#[cfg(not(windows))]
fn replace_path(source: &Path, destination: &Path) -> std::io::Result<()> {
    fs::rename(source, destination)
}

#[cfg(windows)]
fn replace_path(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{
        MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
    };

    let source = source
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let destination = destination
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn validate_command_policy(
    program: &str,
    args: &[String],
    security: WorkspaceSecurity,
) -> Result<()> {
    validate_command_arguments(program, args)?;
    if matches!(args, [flag] if matches!(flag.as_str(), "--version" | "-V" | "version")) {
        return Ok(());
    }
    match program {
        "git" => validate_git_command(args),
        "rg" => validate_rg_command(args),
        "cargo" => validate_cargo_command(args, security.allow_risky_exec),
        "go" => validate_go_command(args, security.allow_risky_exec),
        "npm" | "pnpm" | "yarn" | "bun" => {
            validate_package_command(program, args, security.allow_risky_exec)
        }
        "rustc" | "node" | "python3" | "pytest" | "make" => {
            require_risky_exec(program, security.allow_risky_exec)
        }
        _ => bail!("command policy is missing for allowlisted program: {program}"),
    }
}

fn validate_command_arguments(program: &str, args: &[String]) -> Result<()> {
    let rg_pattern_index = (program == "rg")
        .then(|| args.iter().position(|arg| !arg.starts_with('-')))
        .flatten();
    for (index, arg) in args.iter().enumerate() {
        if arg.contains('\0') || arg.contains(['\n', '\r']) {
            bail!("command arguments contain forbidden control characters");
        }
        if program == "rustc" && arg.contains('@') {
            bail!(
                "rustc response-file arguments are blocked because they bypass argument inspection"
            );
        }
        let value = arg.split_once('=').map(|(_, value)| value).unwrap_or(arg);
        if value.starts_with("file://") {
            bail!("file:// arguments are blocked");
        }
        if value.starts_with("http://") || value.starts_with("https://") {
            continue;
        }
        if rg_pattern_index == Some(index) {
            continue;
        }
        reject_protected_command_argument(value)?;
        let windows_absolute = value.len() >= 3
            && value.as_bytes()[1] == b':'
            && matches!(value.as_bytes()[2], b'\\' | b'/');
        let parent_component = value.split(['/', '\\']).any(|component| component == "..");
        if Path::new(value).is_absolute() || windows_absolute || parent_component {
            bail!("command argument may escape the selected workspace: {arg}");
        }
    }
    Ok(())
}

fn reject_protected_command_argument(value: &str) -> Result<()> {
    let mut candidates = vec![value];
    if let Some((_, suffix)) = value.rsplit_once(':') {
        if !suffix.is_empty() {
            candidates.push(suffix);
        }
    }
    for candidate in candidates {
        let candidate = candidate
            .trim_start_matches(":(glob)")
            .trim_start_matches(":(literal)")
            .trim_start_matches(":(top)")
            .trim_start_matches(':');
        if candidate.is_empty() || candidate.starts_with('-') {
            continue;
        }
        if let Err(error) = reject_protected_path(Path::new(candidate)) {
            bail!("command argument targets a protected path: {value} ({error})");
        }
    }
    Ok(())
}

fn validate_git_command(args: &[String]) -> Result<()> {
    let subcommand_index = args
        .iter()
        .position(|arg| !arg.starts_with('-'))
        .ok_or_else(|| anyhow!("git subcommand is required"))?;
    for option in &args[..subcommand_index] {
        if !matches!(option.as_str(), "--no-pager" | "--literal-pathspecs") {
            bail!("git global option is blocked by the workspace policy: {option}");
        }
    }
    let subcommand = args[subcommand_index].as_str();
    if !matches!(
        subcommand,
        "status" | "diff" | "log" | "show" | "rev-parse" | "ls-files"
    ) {
        bail!("git subcommand is not read-only and is blocked: {subcommand}");
    }
    for arg in &args[subcommand_index + 1..] {
        if arg == "--ext-diff"
            || arg == "--textconv"
            || arg == "--open-files-in-pager"
            || arg == "--show-signature"
            || arg == "--output"
            || arg.starts_with("--output=")
            || arg.starts_with("--git-dir")
            || arg.starts_with("--work-tree")
            || arg.contains("%G")
        {
            bail!("git option can execute helpers or write outside the result stream: {arg}");
        }
    }
    Ok(())
}

fn validate_rg_command(args: &[String]) -> Result<()> {
    for arg in args {
        if matches!(
            arg.as_str(),
            "--pre"
                | "--pre-glob"
                | "-L"
                | "--follow"
                | "--hidden"
                | "-u"
                | "-uu"
                | "-uuu"
                | "--no-ignore"
                | "--no-ignore-vcs"
                | "--no-ignore-dot"
                | "--no-ignore-global"
                | "--no-ignore-parent"
                | "--no-ignore-files"
                | "-f"
                | "--file"
                | "--ignore-file"
                | "-g"
                | "--glob"
                | "--iglob"
                | "--type-add"
                | "--type-clear"
        ) || arg.starts_with("--pre=")
            || arg.starts_with("--pre-glob=")
            || arg.starts_with("--file=")
            || arg.starts_with("--ignore-file=")
            || arg.starts_with("--glob=")
            || arg.starts_with("--iglob=")
            || arg.starts_with("--type-add=")
            || arg.starts_with("--type-clear=")
            || (arg.starts_with("-f") && arg.len() > 2)
            || (arg.starts_with("-g") && arg.len() > 2)
        {
            bail!("ripgrep option is blocked because it can read helper files or bypass protected paths: {arg}");
        }
    }
    Ok(())
}

fn validate_verification_command_shape(program: &str, args: &[String]) -> Result<()> {
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    let allowed = matches!(
        (program, args.as_slice()),
        ("git", ["diff", "--check"])
            | ("cargo", ["fmt", "--check"])
            | ("cargo", ["check"])
            | ("cargo", ["check", "--locked"])
            | ("cargo", ["test"])
            | ("cargo", ["test", "--locked"])
            | ("cargo", ["clippy", "--", "-D", "warnings"])
            | ("cargo", ["clippy", "--locked", "--", "-D", "warnings"])
            | ("cargo", ["build", "--release"])
            | ("cargo", ["build", "--release", "--locked"])
            | ("go", ["test", "./..."])
            | ("pytest", ["-q"])
            | ("make", ["check" | "lint" | "test"])
            | (
                "npm" | "pnpm" | "yarn" | "bun",
                [
                    "run",
                    "lint" | "typecheck" | "check" | "format:check" | "test" | "build"
                ],
            )
    );
    if allowed {
        Ok(())
    } else {
        bail!(
            "command is not an approved inferred verification shape: {}",
            format_command(program, &args)
        )
    }
}

fn format_command(program: &str, args: &[&str]) -> String {
    std::iter::once(program)
        .chain(args.iter().copied())
        .collect::<Vec<_>>()
        .join(" ")
}

fn is_default_safe_cargo_command(args: &[String]) -> bool {
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    matches!(
        args.as_slice(),
        ["fmt", "--check"] | ["check"] | ["check", "--locked"]
    )
}

fn validate_cargo_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    for arg in args {
        if matches!(
            arg.as_str(),
            "--config" | "--manifest-path" | "--target-dir" | "-C"
        ) || arg.starts_with("--config=")
            || arg.starts_with("--manifest-path=")
            || arg.starts_with("--target-dir=")
        {
            bail!("cargo option is blocked because it can redirect configuration or filesystem access: {arg}");
        }
    }
    if is_default_safe_cargo_command(args) {
        return Ok(());
    }
    let subcommand = args
        .iter()
        .find(|arg| !arg.starts_with('-'))
        .map(String::as_str)
        .ok_or_else(|| anyhow!("cargo subcommand is required"))?;
    match subcommand {
        "metadata" => require_risky_exec("cargo metadata", allow_risky_exec),
        "fmt" if args.iter().any(|arg| arg == "--check") => {
            require_risky_exec("cargo formatting", allow_risky_exec)
        }
        "check" | "test" | "clippy" | "build" => {
            require_risky_exec("cargo project execution", allow_risky_exec)
        }
        _ => bail!("cargo subcommand is blocked by the safe execution policy: {subcommand}"),
    }
}

fn validate_go_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    for arg in args {
        if matches!(
            arg.as_str(),
            "-C" | "-exec" | "-toolexec" | "-overlay" | "-modfile"
        ) || arg.starts_with("-C=")
            || arg.starts_with("-exec=")
            || arg.starts_with("-toolexec=")
            || arg.starts_with("-overlay=")
            || arg.starts_with("-modfile=")
        {
            bail!("go option is blocked because it can redirect execution or filesystem access: {arg}");
        }
    }
    let subcommand = args.first().map(String::as_str).unwrap_or_default();
    match subcommand {
        "list" | "test" | "vet" | "build" => {
            require_risky_exec("go project inspection/execution", allow_risky_exec)
        }
        _ => bail!("go subcommand is blocked by the safe execution policy: {subcommand}"),
    }
}

fn validate_package_command(program: &str, args: &[String], allow_risky_exec: bool) -> Result<()> {
    for arg in args {
        if matches!(
            arg.as_str(),
            "--prefix"
                | "--cwd"
                | "--dir"
                | "--global"
                | "-g"
                | "--userconfig"
                | "--config"
                | "--global-dir"
        ) || arg.starts_with("--prefix=")
            || arg.starts_with("--cwd=")
            || arg.starts_with("--dir=")
            || arg.starts_with("--userconfig=")
            || arg.starts_with("--config=")
            || arg.starts_with("--global-dir=")
        {
            bail!("{program} option is blocked because it can redirect configuration or filesystem access: {arg}");
        }
    }
    let subcommand = args.first().map(String::as_str).unwrap_or_default();
    if matches!(
        subcommand,
        "list" | "ls" | "why" | "run" | "test" | "build" | "lint" | "check" | "typecheck"
    ) {
        return require_risky_exec(
            &format!("{program} project inspection/script"),
            allow_risky_exec,
        );
    }
    bail!("{program} subcommand is blocked by the safe execution policy: {subcommand}")
}

fn require_risky_exec(label: &str, enabled: bool) -> Result<()> {
    if enabled {
        Ok(())
    } else {
        bail!(
            "{label} can execute project-controlled code and is disabled by default; restart with --allow-risky-exec only for a trusted repository"
        )
    }
}

fn hardened_command_args(program: &str, args: &[String]) -> Vec<String> {
    if program != "git" {
        return args.to_vec();
    }
    let Some(subcommand_index) = args.iter().position(|arg| !arg.starts_with('-')) else {
        return args.to_vec();
    };

    let null_path = if cfg!(windows) { "NUL" } else { "/dev/null" };
    let overrides = [
        "core.fsmonitor=false".to_owned(),
        "core.untrackedCache=false".to_owned(),
        format!("core.hooksPath={null_path}"),
        format!("core.attributesFile={null_path}"),
        format!("core.excludesFile={null_path}"),
        "diff.external=".to_owned(),
        "maintenance.auto=false".to_owned(),
        "gc.auto=0".to_owned(),
    ];
    let extra_diff_args = if matches!(args[subcommand_index].as_str(), "diff" | "log" | "show") {
        2
    } else {
        0
    };
    let mut hardened = Vec::with_capacity(args.len() + overrides.len() * 2 + extra_diff_args);
    for override_value in overrides {
        hardened.push("-c".to_owned());
        hardened.push(override_value);
    }
    hardened.extend_from_slice(&args[..=subcommand_index]);
    if matches!(args[subcommand_index].as_str(), "diff" | "log" | "show") {
        hardened.push("--no-ext-diff".to_owned());
        hardened.push("--no-textconv".to_owned());
    }
    hardened.extend_from_slice(&args[subcommand_index + 1..]);
    hardened
}

fn scrub_sensitive_environment(command: &mut Command, program: &str) {
    for (key, _) in std::env::vars() {
        let upper = key.to_ascii_uppercase();
        if (program == "git" && upper.starts_with("GIT_"))
            || upper.contains("TOKEN")
            || upper.contains("SECRET")
            || upper.contains("PASSWORD")
            || upper.ends_with("_KEY")
            || upper.starts_with("AWS_")
            || upper.starts_with("AZURE_")
            || upper.starts_with("GOOGLE_")
            || upper.starts_with("GITHUB_")
            || upper.starts_with("GITLAB_")
            || matches!(
                upper.as_str(),
                "SSH_AUTH_SOCK" | "KUBECONFIG" | "DOCKER_CONFIG" | "NETRC" | "GIT_ASKPASS"
            )
        {
            command.env_remove(key);
        }
    }
    command.env("NO_COLOR", "1");
    if program == "git" {
        let null_config = if cfg!(windows) { "NUL" } else { "/dev/null" };
        command
            .env("GIT_TERMINAL_PROMPT", "0")
            .env("GIT_OPTIONAL_LOCKS", "0")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_SYSTEM", null_config)
            .env("GIT_CONFIG_GLOBAL", null_config)
            .env("GIT_ATTR_NOSYSTEM", "1")
            .env("GIT_PROTOCOL_FROM_USER", "0")
            .env("GIT_PAGER", "cat")
            .env("PAGER", "cat")
            .env("GIT_EDITOR", "false")
            .env("GIT_SEQUENCE_EDITOR", "false")
            .env("GIT_EXTERNAL_DIFF", "");
    }
}

async fn read_bounded_stream<R>(mut reader: R) -> std::io::Result<(String, bool)>
where
    R: AsyncRead + Unpin,
{
    let mut stored = Vec::with_capacity(MAX_OUTPUT_BYTES.min(16 * 1024));
    let mut buffer = [0u8; 8192];
    let mut truncated = false;
    loop {
        let read = reader.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        let remaining = MAX_OUTPUT_BYTES.saturating_sub(stored.len());
        let keep = remaining.min(read);
        stored.extend_from_slice(&buffer[..keep]);
        truncated |= keep < read;
    }
    Ok((String::from_utf8_lossy(&stored).to_string(), truncated))
}

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
    fn command_policy_allows_bounded_checks_but_blocks_mutation_and_project_code() {
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
        assert!(validate_command_policy(
            "cargo",
            &["check".to_owned(), "--workspace".to_owned()],
            safe,
        )
        .is_err());
        assert!(validate_command_policy("cargo", &["test".to_owned()], safe).is_err());
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
