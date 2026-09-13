use crate::evidence_store::workspace_state_directory;
use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const JOURNAL_VERSION: u8 = 1;
const MAX_ENGINEERING_MILESTONES: usize = 512;
const MAX_MILESTONE_BYTES: u64 = 16 * 1024;
const MAX_MILESTONE_PATHS: usize = 32;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EngineeringMilestone {
    pub version: u8,
    pub timestamp_ms: u64,
    pub tool: String,
    pub stage: String,
    pub outcome: String,
    pub duration_ms: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_level: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checks_run: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checks_failed: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retrieval_intent: Option<String>,
}

impl EngineeringMilestone {
    pub(crate) fn new(
        tool: impl Into<String>,
        stage: impl Into<String>,
        outcome: impl Into<String>,
        duration_ms: u64,
        paths: Vec<String>,
    ) -> Result<Self> {
        let milestone = Self {
            version: JOURNAL_VERSION,
            timestamp_ms: now_ms(),
            tool: tool.into(),
            stage: stage.into(),
            outcome: outcome.into(),
            duration_ms,
            paths,
            verification_level: None,
            checks_run: None,
            checks_failed: None,
            retrieval_intent: None,
        };
        milestone.validate()?;
        Ok(milestone)
    }

    fn validate(&self) -> Result<()> {
        if self.version != JOURNAL_VERSION {
            bail!("unsupported engineering journal record version");
        }
        if self.timestamp_ms == 0
            || !bounded_token(&self.tool, 80)
            || !matches!(
                self.stage.as_str(),
                "understand" | "change" | "prove" | "model" | "converge"
            )
            || !matches!(
                self.outcome.as_str(),
                "succeeded" | "partial" | "blocked" | "failed"
            )
            || self.paths.len() > MAX_MILESTONE_PATHS
        {
            bail!("invalid engineering journal record");
        }
        if self
            .verification_level
            .as_ref()
            .is_some_and(|level| !matches!(level.as_str(), "quick" | "full"))
        {
            bail!("invalid engineering journal verification level");
        }
        if self.retrieval_intent.as_ref().is_some_and(|intent| {
            !matches!(
                intent.as_str(),
                "balanced_context"
                    | "trace_to_code"
                    | "code_to_test"
                    | "comment_to_context"
                    | "failure_trace_to_code"
                    | "edit_to_ripple"
            )
        }) {
            bail!("invalid engineering journal retrieval intent");
        }
        for path in &self.paths {
            if !valid_repository_path(path) {
                bail!("invalid engineering journal repository path");
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(crate) struct EngineeringJournalHistory {
    pub records: Vec<EngineeringMilestone>,
    pub retained_records: usize,
    pub truncated: bool,
}

pub(crate) fn persist(workspace: &Workspace, milestone: &EngineeringMilestone) -> Result<()> {
    milestone.validate()?;
    let directory = journal_directory(workspace)?;
    ensure_directory(&directory)?;
    let bytes = serde_json::to_vec(milestone).context("cannot encode engineering milestone")?;
    if bytes.len() as u64 > MAX_MILESTONE_BYTES {
        bail!("engineering milestone exceeds the persistent store size bound");
    }
    let digest = digest_bytes(&bytes);
    let path = directory.join(format!(
        "{:020}-{}.json",
        milestone.timestamp_ms,
        &digest[..24]
    ));
    if path.exists() {
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
        .with_context(|| format!("cannot create engineering milestone {}", path.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("cannot write engineering milestone {}", path.display()))?;
    file.flush()
        .with_context(|| format!("cannot flush engineering milestone {}", path.display()))?;
    prune_directory(&directory)?;
    Ok(())
}

pub(crate) fn load_recent(
    workspace: &Workspace,
    limit: usize,
) -> Result<EngineeringJournalHistory> {
    let directory = journal_directory(workspace)?;
    if !directory.exists() {
        return Ok(EngineeringJournalHistory {
            records: Vec::new(),
            retained_records: 0,
            truncated: false,
        });
    }
    ensure_existing_directory(&directory)?;
    let paths = journal_paths(&directory)?;
    let retained_records = paths.len();
    let limit = limit.clamp(1, MAX_ENGINEERING_MILESTONES);
    let selected = paths.into_iter().rev().take(limit).collect::<Vec<_>>();
    let mut records = Vec::with_capacity(selected.len());
    for path in selected {
        let metadata = fs::symlink_metadata(&path)
            .with_context(|| format!("cannot inspect engineering milestone {}", path.display()))?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.len() > MAX_MILESTONE_BYTES
        {
            continue;
        }
        let bytes = fs::read(&path)
            .with_context(|| format!("cannot read engineering milestone {}", path.display()))?;
        let milestone: EngineeringMilestone = match serde_json::from_slice(&bytes) {
            Ok(milestone) => milestone,
            Err(_) => continue,
        };
        if milestone.validate().is_ok() {
            records.push(milestone);
        }
    }
    records.sort_by_key(|record| std::cmp::Reverse(record.timestamp_ms));
    Ok(EngineeringJournalHistory {
        records,
        retained_records,
        truncated: retained_records > limit,
    })
}

pub(crate) fn change_fingerprint(workspace: &Workspace) -> Result<String> {
    let directory = journal_directory(workspace)?;
    let metadata = match fs::symlink_metadata(&directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok("empty".into()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("engineering journal path is not a regular directory");
    }
    let paths = journal_paths(&directory)?;
    let mut hasher = Sha256::new();
    hasher.update(b"engineering-journal-metadata-v1");
    for path in paths {
        let metadata = fs::symlink_metadata(&path)?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            bail!("engineering journal fingerprint requires regular records");
        }
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default();
        hasher.update(name.as_bytes());
        hasher.update([0]);
        hasher.update(metadata.len().to_le_bytes());
        let modified = metadata.modified()?.duration_since(UNIX_EPOCH)?.as_nanos();
        hasher.update(modified.to_le_bytes());
    }
    Ok(format!("metadata:{:x}", hasher.finalize()))
}

fn journal_directory(workspace: &Workspace) -> Result<PathBuf> {
    Ok(workspace_state_directory(workspace)?.join("engineering-journal"))
}

fn ensure_directory(directory: &Path) -> Result<()> {
    if directory.exists() {
        return ensure_existing_directory(directory);
    }
    fs::create_dir_all(directory)
        .with_context(|| format!("cannot create engineering journal {}", directory.display()))?;
    ensure_existing_directory(directory)
}

fn ensure_existing_directory(directory: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(directory)
        .with_context(|| format!("cannot inspect engineering journal {}", directory.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("engineering journal path is not a regular directory");
    }
    Ok(())
}

fn journal_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = fs::read_dir(directory)
        .with_context(|| format!("cannot list engineering journal {}", directory.display()))?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(".json"))
                .then(|| entry.path())
        })
        .collect::<Vec<_>>();
    if paths.len() > MAX_ENGINEERING_MILESTONES.saturating_add(1) {
        bail!("engineering journal exceeds its record bound");
    }
    paths.sort();
    Ok(paths)
}

fn prune_directory(directory: &Path) -> Result<()> {
    let paths = journal_paths(directory)?;
    let excess = paths.len().saturating_sub(MAX_ENGINEERING_MILESTONES);
    for path in paths.into_iter().take(excess) {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

fn bounded_token(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

fn valid_repository_path(value: &str) -> bool {
    if value.is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
        return false;
    }
    let path = Path::new(value);
    !path.is_absolute()
        && !path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
#[path = "../../tests/unit/evidence/journal.rs"]
mod tests;
