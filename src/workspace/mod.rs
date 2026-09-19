use crate::authorization::{
    AuthorizationKind, AuthorizationManager, AuthorizationRequest, AuthorizationRequired,
};
use anyhow::{anyhow, bail, Context, Result};
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock, Weak};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use uuid::Uuid;

const MAX_WORKSPACES: usize = 32;
const MAX_LIST_ENTRIES: usize = 10_000;
const MAX_READ_BYTES: u64 = 1024 * 1024;
const MAX_MODEL_READ_LINES: usize = 1_000;
const MAX_WRITE_BYTES: usize = 4 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 256 * 1024;
const MAX_SAFE_REMOVAL_BYTES: usize = 4 * 1024;
const MAX_SAFE_REDUCTION_PERCENT: usize = 60;
const MAX_TEXT_EDITS: usize = 128;
const MAX_BATCH_WRITE_ITEMS: usize = 64;
const MAX_SEARCH_QUERIES: usize = 32;
pub(crate) const LANGUAGE_DEVELOPMENT_COMMANDS: &[&str] = &[
    "cargo",
    "cargo-audit",
    "cargo-mutants",
    "cargo-fuzz",
    "rustc",
    "go",
    "gofmt",
    "staticcheck",
    "govulncheck",
    "npm",
    "pnpm",
    "yarn",
    "bun",
    "node",
    "deno",
    "biome",
    "eslint",
    "prettier",
    "vitest",
    "jest",
    "tsc",
    "stylelint",
    "htmlhint",
    "python3",
    "pytest",
    "uv",
    "ruff",
    "mypy",
    "pyright",
    "bandit",
    "mutmut",
    "dart",
    "flutter",
    "mix",
    "elixir",
    "dune",
    "ocamlc",
    "ocamlopt",
    "ocamlformat",
    "bundle",
    "ruby",
    "rubocop",
    "rspec",
    "php",
    "composer",
    "phpstan",
    "psalm",
    "phpunit",
    "php-cs-fixer",
    "cmake",
    "ninja",
    "cc",
    "c++",
    "gcc",
    "g++",
    "clang",
    "clang++",
    "clang-format",
    "clang-tidy",
    "dotnet",
    "mvn",
    "gradle",
    "java",
    "javac",
    "dotnet-stryker",
    "swift",
    "swift-format",
    "swiftlint",
    "muter",
    "zig",
    "Rscript",
    "lua",
    "stylua",
    "luacheck",
    "busted",
    "infection",
    "shellcheck",
    "shfmt",
    "bats",
    "make",
    "just",
    "task",
    "bazel",
    "bazelisk",
    "buck2",
    "pants",
    "meson",
    "ctest",
    "sbt",
    "lein",
    "rebar3",
    "poetry",
    "pdm",
    "hatch",
    "tox",
    "nox",
    "nx",
    "turbo",
    "vite",
    "webpack",
    "rollup",
    "esbuild",
    "tsup",
    "parcel",
    "rspack",
    "rolldown",
    "rake",
    "cabal",
    "stack",
    "pre-commit",
    "act",
    "fd",
    "jq",
    "rg",
];

pub(crate) const COMMAND_CATALOG: &[&str] = &[
    "cargo",
    "cargo-audit",
    "cargo-mutants",
    "cargo-fuzz",
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
    "mypy",
    "pyright",
    "bandit",
    "mutmut",
    "go",
    "make",
    "just",
    "task",
    "bazel",
    "bazelisk",
    "buck2",
    "pants",
    "meson",
    "ctest",
    "sbt",
    "lein",
    "rebar3",
    "poetry",
    "pdm",
    "hatch",
    "tox",
    "nox",
    "nx",
    "turbo",
    "vite",
    "webpack",
    "rollup",
    "esbuild",
    "tsup",
    "parcel",
    "rspack",
    "rolldown",
    "rake",
    "cabal",
    "stack",
    "uv",
    "ruff",
    "biome",
    "deno",
    "dart",
    "flutter",
    "mix",
    "elixir",
    "dune",
    "bundle",
    "phpstan",
    "psalm",
    "phpunit",
    "php-cs-fixer",
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
    "dotnet-stryker",
    "swift",
    "swift-format",
    "swiftlint",
    "muter",
    "zig",
    "pre-commit",
    "act",
    "gofmt",
    "staticcheck",
    "govulncheck",
    "eslint",
    "prettier",
    "vitest",
    "jest",
    "tsc",
    "stylelint",
    "htmlhint",
    "mypy",
    "pyright",
    "bandit",
    "ocamlc",
    "ocamlopt",
    "ocamlformat",
    "ruby",
    "rubocop",
    "rspec",
    "php",
    "composer",
    "cc",
    "c++",
    "gcc",
    "g++",
    "clang",
    "clang++",
    "clang-format",
    "clang-tidy",
    "java",
    "javac",
    "swift-format",
    "Rscript",
    "lua",
    "stylua",
    "luacheck",
    "busted",
    "infection",
    "shellcheck",
    "shfmt",
    "bats",
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
    pub allow_unrestricted_commands: bool,
    pub allow_semantic_exec: bool,
    pub allow_destructive_writes: bool,
    pub allow_overlapping_workspaces: bool,
    pub allow_user_home_workspace: bool,
    pub allow_broad_workspace: bool,
}

impl Default for WorkspaceSecurity {
    fn default() -> Self {
        Self {
            allow_risky_exec: false,
            allow_unrestricted_commands: false,
            allow_semantic_exec: true,
            allow_destructive_writes: false,
            allow_overlapping_workspaces: false,
            allow_user_home_workspace: false,
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
    full_access: Arc<AtomicBool>,
    authorization: AuthorizationManager,
}

#[derive(Clone)]
struct WorkspaceRoot {
    id: String,
    workspace: Workspace,
    parent_id: Option<String>,
    markers: Vec<&'static str>,
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

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct SourceMetadataStamp {
    len: u64,
    modified_nanos: u128,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    changed_nanos: i128,
}

impl SourceMetadataStamp {
    pub(crate) fn len(self) -> u64 {
        self.len
    }

    pub(crate) fn update_sha256(self, hasher: &mut Sha256) {
        hasher.update(self.len.to_le_bytes());
        hasher.update(self.modified_nanos.to_le_bytes());
        #[cfg(unix)]
        {
            hasher.update(self.device.to_le_bytes());
            hasher.update(self.inode.to_le_bytes());
            hasher.update(self.changed_nanos.to_le_bytes());
        }
    }
}

pub(crate) type StampedSourcePath = (String, SourceMetadataStamp);

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SourceStamp {
    len: u64,
    modified_nanos: u128,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(unix)]
    changed_nanos: i128,
}

impl SourceStamp {
    fn metadata_stamp(&self) -> SourceMetadataStamp {
        SourceMetadataStamp {
            len: self.len,
            modified_nanos: self.modified_nanos,
            #[cfg(unix)]
            device: self.device,
            #[cfg(unix)]
            inode: self.inode,
            #[cfg(unix)]
            changed_nanos: self.changed_nanos,
        }
    }
}

#[derive(Debug)]
pub(crate) struct SourceDocument {
    pub path: String,
    pub content: String,
    pub sha256: String,
    pub readonly: bool,
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

#[path = "models.rs"]
mod models;
pub use models::{CommandResult, FileView};

#[path = "edits.rs"]
mod edits;
#[path = "media.rs"]
mod media;
#[path = "ignore.rs"]
mod walk_ignore;
pub(crate) use walk_ignore::{repository_ignore_builder, repository_walk_builder};
#[path = "registry.rs"]
mod registry;
#[path = "roots.rs"]
mod roots;
#[path = "search.rs"]
mod search;
pub(crate) use search::{SearchMode, SearchReport, SearchRequest};

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
        let honor_parent_ignores = start == self.root;
        for entry in repository_walk_builder(&start, honor_parent_ignores)
            .build()
            .filter_map(std::result::Result::ok)
        {
            visited = visited.saturating_add(1);
            if visited.is_multiple_of(64) {
                drop(cpu_slice.take());
                cpu_slice = Some(crate::resource::cpu_work(
                    crate::resource::WorkClass::Interactive,
                ));
            }
            if entry.file_type().is_some_and(|kind| kind.is_file()) {
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

    pub fn read_files(
        &self,
        paths: &[String],
        start_line: usize,
        end_line: Option<usize>,
    ) -> Result<Vec<serde_json::Value>> {
        if paths.is_empty() || paths.len() > 32 {
            bail!("paths must contain between 1 and 32 files");
        }
        // Reads hash and transform text; keep them on the CPU pool. A wider
        // I/O fanout regressed the paired warm-read benchmark.
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
        let reader = fs::File::open(&file)?;
        let metadata = reader.metadata()?;
        if !metadata.is_file() {
            bail!("path is not a file");
        }
        if metadata.len() > MAX_READ_BYTES {
            bail!("file exceeds 1 MiB read limit");
        }
        let capacity = usize::try_from(metadata.len().min(MAX_READ_BYTES)).unwrap_or_default();
        let mut content = String::with_capacity(capacity);
        let mut limited = reader.take(MAX_READ_BYTES.saturating_add(1));
        limited
            .read_to_string(&mut content)
            .context("file is not valid UTF-8 text")?;
        if content.len() as u64 > MAX_READ_BYTES {
            bail!("file exceeds 1 MiB read limit");
        }
        let _cpu = crate::resource::cpu_work(crate::resource::WorkClass::Interactive);
        let hash = sha256(content.as_bytes());
        let start = start_line.max(1);
        let requested_end = end_line
            .unwrap_or_else(|| start.saturating_add(MAX_MODEL_READ_LINES.saturating_sub(1)));
        let bounded_end =
            requested_end.min(start.saturating_add(MAX_MODEL_READ_LINES.saturating_sub(1)));
        let mut total = 0usize;
        let mut selected = String::new();
        for (index, line) in content.split_inclusive('\n').enumerate() {
            let line_number = index + 1;
            total = line_number;
            if line_number < start || line_number > bounded_end {
                continue;
            }
            selected.push_str(line);
        }
        // Keep the legacy final-line terminator convention, but never
        // normalize separators inside the selected original-byte window.
        if selected.ends_with("\r\n") {
            selected.truncate(selected.len() - 2);
        } else if selected.ends_with('\n') {
            selected.pop();
        }
        let end = bounded_end.min(total).max(start.saturating_sub(1));
        let (safe, redacted) = redact_sensitive_text(&selected);
        // The redactor normalizes lines. Its sanitized output is mandatory
        // whenever it changed sensitive text; unchanged source keeps its bytes.
        let selected = if redacted { safe } else { selected };
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
        crate::resource::parallel_io(moves, |request| {
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
            return Err(AuthorizationRequired::new(request).into());
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
    operation_fingerprint, protected_component_kind, reject_destructive_replacement,
    reject_protected_path, root_identity, sha256, sha256_file, source_stamp, validate_batch_paths,
    validate_independent_moves, validate_movable_directory, validate_source_metadata,
    validate_workspace_root, validate_write_content, workspace_id,
};

#[path = "command_policy.rs"]
mod command_policy;
use command_policy::*;
#[path = "sandbox.rs"]
mod sandbox;
pub(crate) use sandbox::status as execution_sandbox_status;

pub(crate) fn command_writes_workspace(program: &str, args: &[String]) -> bool {
    command_requires_workspace_write(program, args)
}

#[path = "operations/execution.rs"]
mod execution;
use execution::redact_sensitive_line;
pub(crate) use execution::redact_sensitive_text;

#[cfg(test)]
#[path = "../../tests/unit/workspace/mod.rs"]
mod tests;
#[cfg(test)]
#[path = "../../tests/unit/workspace/throughput.rs"]
mod throughput_tests;
