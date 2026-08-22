use anyhow::{anyhow, bail, Context, Result};
use memchr::memmem;
use rayon::prelude::*;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use uuid::Uuid;
use walkdir::{DirEntry, WalkDir};

const MAX_READ_BYTES: u64 = 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 256 * 1024;

#[derive(Clone)]
pub struct Workspace {
    root: PathBuf,
    allow_write: bool,
    allow_exec: bool,
    commands: HashSet<String>,
    write_locks: Arc<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>>,
}

#[derive(Clone)]
pub struct Workspaces {
    roots: Arc<Vec<WorkspaceRoot>>,
    default_id: String,
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
}

impl Workspaces {
    pub fn new<I, P>(roots: I, allow_write: bool, allow_exec: bool) -> Result<Self>
    where
        I: IntoIterator<Item = P>,
        P: AsRef<Path>,
    {
        let mut entries = Vec::new();
        let mut used_ids = HashMap::<String, usize>::new();
        let mut seen_roots = HashSet::<PathBuf>::new();

        for root in roots {
            let workspace = Workspace::new(root, allow_write, allow_exec)?;
            if !seen_roots.insert(workspace.root.clone()) {
                continue;
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
            "workspaces": self.roots.iter().map(|root| serde_json::json!({
                "id": root.id,
                "root": root.workspace.root,
                "write_enabled": root.workspace.allow_write,
                "exec_enabled": root.workspace.allow_exec,
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
    pub fn new(root: impl AsRef<Path>, allow_write: bool, allow_exec: bool) -> Result<Self> {
        let root = root
            .as_ref()
            .canonicalize()
            .with_context(|| format!("workspace does not exist: {}", root.as_ref().display()))?;
        if !root.is_dir() {
            bail!("workspace is not a directory: {}", root.display());
        }
        let commands = [
            "cargo", "rustc", "git", "rg", "npm", "pnpm", "yarn", "bun", "node", "python3",
            "pytest", "go", "make",
        ]
        .into_iter()
        .map(str::to_owned)
        .collect();
        Ok(Self {
            root,
            allow_write,
            allow_exec,
            commands,
            write_locks: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    fn validate_relative(path: &str) -> Result<PathBuf> {
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
        }
        Ok(candidate)
    }

    fn existing_path(&self, path: &str) -> Result<PathBuf> {
        let relative = Self::validate_relative(path)?;
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
        let relative = Self::validate_relative(path)?;
        if relative.as_os_str().is_empty() {
            bail!("file path is required");
        }
        let target = self.root.join(relative);
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
        Ok(locks
            .entry(path.to_path_buf())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone())
    }

    pub fn list_files(&self, path: &str, max_entries: usize) -> Result<Vec<String>> {
        let start = self.existing_path(path)?;
        if !start.is_dir() {
            bail!("path is not a directory");
        }
        let max_entries = max_entries.clamp(1, 2000);
        let mut files = Vec::new();
        for entry in WalkDir::new(start)
            .follow_links(false)
            .into_iter()
            .filter_entry(visible_entry)
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
            bail!("writes are disabled; restart with --allow-write");
        }
        if old_text.is_empty() {
            bail!("old_text must not be empty");
        }
        let file = self.existing_path(path)?;
        let file_lock = self.write_lock_for(&file)?;
        let _write_guard = file_lock
            .lock()
            .map_err(|_| anyhow!("file write lock poisoned"))?;
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
            bail!("writes are disabled; restart with --allow-write");
        }
        let file = self.new_path(path)?;
        let file_lock = self.write_lock_for(&file)?;
        let _write_guard = file_lock
            .lock()
            .map_err(|_| anyhow!("file write lock poisoned"))?;
        if file.exists() {
            bail!("file already exists; use replace_text for existing files");
        }
        atomic_write(&file, content.as_bytes())?;
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
            bail!("command execution is disabled; restart with --allow-exec");
        }
        if !self.commands.contains(program) {
            bail!("program is not allowlisted: {program}");
        }
        reject_dangerous_command(program, args)?;
        let cwd = self.existing_path(cwd)?;
        if !cwd.is_dir() {
            bail!("cwd is not a directory");
        }
        let mut command = Command::new(program);
        command
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        scrub_sensitive_environment(&mut command);
        let output = timeout(
            Duration::from_secs(timeout_seconds.clamp(1, 300)),
            command.output(),
        )
        .await
        .map_err(|_| anyhow!("command timed out"))??;
        let (stdout, stdout_cut) = bounded_utf8(&output.stdout);
        let (stderr, stderr_cut) = bounded_utf8(&output.stderr);
        Ok(CommandResult {
            program: program.to_owned(),
            args: args.to_vec(),
            exit_code: output.status.code(),
            success: output.status.success(),
            stdout,
            stderr,
            truncated: stdout_cut || stderr_cut,
        })
    }
}

fn redact_sensitive_text(text: &str) -> (String, bool) {
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

fn visible_entry(entry: &DirEntry) -> bool {
    let Some(name) = entry.file_name().to_str() else {
        return false;
    };
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

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn atomic_write(path: &Path, content: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("target has no parent directory"))?;
    let temp = parent.join(format!(".wcode-{}.tmp", Uuid::new_v4()));
    fs::write(&temp, content)?;
    if let Ok(metadata) = fs::metadata(path) {
        fs::set_permissions(&temp, metadata.permissions())?;
    }
    if let Err(error) = replace_path(&temp, path) {
        let _ = fs::remove_file(&temp);
        return Err(error.into());
    }
    Ok(())
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

fn reject_dangerous_command(program: &str, args: &[String]) -> Result<()> {
    if program == "git" {
        let subcommand = args
            .iter()
            .find(|arg| !arg.starts_with('-'))
            .map(String::as_str);
        if matches!(
            subcommand,
            Some("push" | "clean" | "reset" | "checkout" | "restore" | "rebase")
        ) {
            bail!(
                "dangerous git subcommand is blocked: {}",
                subcommand.unwrap()
            );
        }
    }
    Ok(())
}

fn scrub_sensitive_environment(command: &mut Command) {
    for (key, _) in std::env::vars() {
        let upper = key.to_ascii_uppercase();
        if upper.contains("TOKEN")
            || upper.contains("SECRET")
            || upper.contains("PASSWORD")
            || upper.ends_with("_KEY")
        {
            command.env_remove(key);
        }
    }
}

fn bounded_utf8(bytes: &[u8]) -> (String, bool) {
    let truncated = bytes.len() > MAX_OUTPUT_BYTES;
    let slice = &bytes[..bytes.len().min(MAX_OUTPUT_BYTES)];
    (String::from_utf8_lossy(slice).to_string(), truncated)
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
    fn hides_common_noise_and_local_secrets() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".idea")).unwrap();
        fs::write(dir.path().join(".idea/workspace.xml"), "private IDE state").unwrap();
        fs::write(dir.path().join(".env"), "TOKEN=secret").unwrap();
        fs::write(dir.path().join("server.log"), "noise").unwrap();
        fs::write(dir.path().join("main.rs"), "fn main() {}\n").unwrap();

        let workspace = Workspace::new(dir.path(), false, false).unwrap();
        assert_eq!(workspace.list_files(".", 100).unwrap(), vec!["main.rs"]);
        assert!(workspace.search("secret", ".", 100).unwrap().is_empty());
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
