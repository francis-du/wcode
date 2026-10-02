use crate::evidence_store::workspace_state_directory;
use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "failure_memory.rs"]
pub(crate) mod failure_memory;

#[path = "workspace_access.rs"]
mod workspace_access;

const JOURNAL_VERSION: u8 = 1;
const MAX_ENGINEERING_MILESTONES: usize = 512;
const MAX_MILESTONE_BYTES: u64 = 16 * 1024;
const MAX_MILESTONE_PATHS: usize = 32;
const MAX_FAILURE_CODES: usize = 8;
const MAX_CLOCK_SKEW_MS: u64 = 5 * 60 * 1000;
// Recovery reads at most twice the retained capacity, then still keeps 512.
// Beyond this bound the journal fails closed rather than scanning without limit.
const MAX_RECOVERY_SCAN_ENTRIES: usize = 2 * MAX_ENGINEERING_MILESTONES;
// Serialize complete mutations and snapshots of the same canonical Workspace.
// Independent Workspace stores must not block each other's tool completion.
// This is not a cross-process lock, OS isolation or store authentication.
static JOURNAL_ACCESS: workspace_access::WorkspaceStoreAccess =
    workspace_access::WorkspaceStoreAccess::new();

pub(crate) fn journal_access(workspace: &Workspace) -> Result<Arc<Mutex<()>>> {
    JOURNAL_ACCESS.for_workspace(workspace)
}

/// Bounded historical observations, never Evidence, permission or new policy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EngineeringFailureCode {
    ShaMismatch,
    AuthorizationRequired,
    ProtectedPath,
    SourceLimit,
    VerificationFailure,
    Timeout,
    RevisionStale,
    DiscoveryIncomplete,
}

impl EngineeringFailureCode {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::ShaMismatch => "sha_mismatch",
            Self::AuthorizationRequired => "authorization_required",
            Self::ProtectedPath => "protected_path",
            Self::SourceLimit => "source_limit",
            Self::VerificationFailure => "verification_failure",
            Self::Timeout => "timeout",
            Self::RevisionStale => "revision_stale",
            Self::DiscoveryIncomplete => "discovery_incomplete",
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EngineeringMilestone {
    pub version: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    // Best-effort post-operation observation, never a verification revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_revision: Option<crate::evidence::Revision>,
    pub timestamp_ms: u64,
    pub tool: String,
    pub stage: String,
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failure_codes: Vec<EngineeringFailureCode>,
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
            event_id: Some(uuid::Uuid::new_v4().to_string()),
            observed_revision: None,
            timestamp_ms: now_ms(),
            tool: tool.into(),
            stage: stage.into(),
            outcome: outcome.into(),
            failure_codes: Vec::new(),
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
        if self
            .event_id
            .as_ref()
            .is_some_and(|id| uuid::Uuid::parse_str(id).is_err())
            || self.observed_revision.as_ref().is_some_and(|revision| {
                !valid_revision_digest(&revision.code)
                    || revision
                        .design
                        .as_ref()
                        .is_some_and(|value| !valid_revision_digest(value))
            })
        {
            bail!("invalid engineering journal observation identity");
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
        if self.failure_codes.len() > MAX_FAILURE_CODES
            || self
                .failure_codes
                .iter()
                .copied()
                .collect::<BTreeSet<_>>()
                .len()
                != self.failure_codes.len()
            || (!self.failure_codes.is_empty()
                && !matches!(self.outcome.as_str(), "failed" | "blocked"))
        {
            bail!("invalid engineering journal failure codes");
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
    persist_with_error(workspace, milestone, None)
}

/// Match only a bounded native error in memory; journal bytes never contain it.
pub(crate) fn persist_with_error(
    workspace: &Workspace,
    milestone: &EngineeringMilestone,
    transient_error: Option<&str>,
) -> Result<()> {
    milestone.validate()?;
    let access = journal_access(workspace)?;
    let _access = access
        .lock()
        .map_err(|_| anyhow::anyhow!("engineering journal access lock is poisoned"))?;
    let directory = journal_directory(workspace)?;
    ensure_directory(&directory)?;
    // Recover a bounded old overflow before adding another file.
    prune_directory(&directory)?;
    let bytes = serde_json::to_vec(milestone).context("cannot encode engineering milestone")?;
    if bytes.len() as u64 > MAX_MILESTONE_BYTES {
        bail!("engineering milestone exceeds the persistent store size bound");
    }
    let path = directory.join(milestone_filename(milestone.timestamp_ms, &bytes));
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
    // Close the new record before removal, including Windows file handles.
    drop(file);
    if let Err(error) = prune_directory(&directory) {
        fs::remove_file(&path).with_context(|| {
            format!(
                "engineering journal pruning failed ({error:#}); cannot remove new milestone {}",
                path.display()
            )
        })?;
        // This removes only our new record. Already pruned older records are
        // not restored, and this is not a cross-process transaction rollback.
        return Err(error.context("engineering journal pruning failed; new milestone removed"));
    }
    // Learning is advisory and must not turn a completed operation into a
    // false failure. Recall exposes its own store/rule availability separately.
    let _ = failure_memory::observe_tool_failure(workspace, milestone, transient_error);
    Ok(())
}

pub(crate) fn load_recent(
    workspace: &Workspace,
    limit: usize,
) -> Result<EngineeringJournalHistory> {
    let access = journal_access(workspace)?;
    let _access = access
        .lock()
        .map_err(|_| anyhow::anyhow!("engineering journal access lock is poisoned"))?;
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
    let loaded = crate::resource::parallel_io(&selected, |path| read_milestone(path))?;
    let mut records = Vec::with_capacity(selected.len());
    for milestone in loaded {
        if let Some(milestone) = milestone? {
            records.push(milestone);
        }
    }
    records.sort_by_key(|record| std::cmp::Reverse(record.timestamp_ms));
    let truncated = retained_records > limit || records.len() < selected.len();
    Ok(EngineeringJournalHistory {
        records,
        retained_records,
        truncated,
    })
}

fn read_milestone(path: &Path) -> Result<Option<EngineeringMilestone>> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("cannot inspect engineering milestone {}", path.display()))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_MILESTONE_BYTES
    {
        return Ok(None);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            return Ok(None);
        }
    }
    let mut bytes = Vec::new();
    File::open(path)
        .with_context(|| format!("cannot read engineering milestone {}", path.display()))?
        .take(MAX_MILESTONE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_MILESTONE_BYTES {
        return Ok(None);
    }
    let milestone: EngineeringMilestone = match serde_json::from_slice(&bytes) {
        Ok(milestone) => milestone,
        Err(_) => return Ok(None),
    };
    let canonical = milestone_filename(milestone.timestamp_ms, &bytes);
    if path.file_name().and_then(|name| name.to_str()) != Some(canonical.as_str()) {
        return Ok(None);
    }
    if milestone.validate().is_err()
        || milestone.timestamp_ms > now_ms().saturating_add(MAX_CLOCK_SKEW_MS)
    {
        // A clock that jumped forward and then recovered must not make this
        // observation a permanent latest anchor or manufacture current advice.
        return Ok(None);
    }
    Ok(Some(milestone))
}

pub(crate) fn change_fingerprint(workspace: &Workspace) -> Result<String> {
    let access = journal_access(workspace)?;
    let _access = access
        .lock()
        .map_err(|_| anyhow::anyhow!("engineering journal access lock is poisoned"))?;
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
    let metadata = crate::resource::parallel_io(&paths, |path| journal_record_metadata(path))?;
    let mut hasher = Sha256::new();
    hasher.update(b"engineering-journal-metadata-v1");
    for item in metadata {
        let (name, len, modified) = item?;
        hasher.update(name.as_bytes());
        hasher.update([0]);
        hasher.update(len.to_le_bytes());
        hasher.update(modified.to_le_bytes());
    }
    Ok(format!("metadata:{:x}", hasher.finalize()))
}

fn journal_record_metadata(path: &Path) -> Result<(String, u64, u128)> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!("engineering journal fingerprint requires regular records");
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_owned();
    let modified = metadata.modified()?.duration_since(UNIX_EPOCH)?.as_nanos();
    Ok((name, metadata.len(), modified))
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
    journal_paths_with_bound(directory, MAX_ENGINEERING_MILESTONES.saturating_add(1))
}

fn journal_paths_with_bound(directory: &Path, max_entries: usize) -> Result<Vec<PathBuf>> {
    let entries = fs::read_dir(directory)
        .with_context(|| format!("cannot list engineering journal {}", directory.display()))?;
    collect_journal_paths(entries, max_entries)
}

fn collect_journal_paths(
    entries: impl IntoIterator<Item = std::io::Result<fs::DirEntry>>,
    max_entries: usize,
) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for (index, entry) in entries.into_iter().enumerate() {
        if index >= max_entries {
            bail!("engineering journal exceeds its directory scan bound");
        }
        let entry = entry.context("cannot read engineering journal directory entry")?;
        if entry
            .file_name()
            .to_str()
            .is_some_and(|name| name.ends_with(".json"))
        {
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

fn prune_directory(directory: &Path) -> Result<()> {
    let paths = journal_paths_with_bound(directory, MAX_RECOVERY_SCAN_ENTRIES)?;
    if paths.len() <= MAX_ENGINEERING_MILESTONES {
        return Ok(());
    }
    // Keep at most two distinct observations per fixed failure category. These
    // sixteen maximum anchors survive successful context traffic; they remain
    // bounded historical advice, never a lifetime count or authority.
    let mut seen: BTreeMap<EngineeringFailureCode, BTreeSet<String>> = BTreeMap::new();
    let mut retained = BTreeSet::new();
    for path in paths.iter().rev() {
        let Some(event) = read_milestone(path)? else {
            continue;
        };
        if !matches!(event.outcome.as_str(), "failed" | "blocked") {
            continue;
        }
        let Some(event_id) = event.event_id else {
            continue;
        };
        for code in event.failure_codes {
            let ids = seen.entry(code).or_default();
            if ids.len() < 2 && ids.insert(event_id.clone()) {
                retained.insert(path.clone());
            }
        }
    }
    // Invalid/copied records cannot become anchors. The ordinary ring retains
    // the newest paths as before; its reader reports invalid entries as partial.
    for path in paths.iter().rev() {
        if retained.len() >= MAX_ENGINEERING_MILESTONES {
            break;
        }
        retained.insert(path.clone());
    }
    for path in paths {
        if !retained.contains(&path) {
            fs::remove_file(&path).with_context(|| {
                format!("cannot prune engineering milestone {}", path.display())
            })?;
        }
    }
    Ok(())
}

fn valid_revision_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

fn bounded_token(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

pub(crate) fn valid_repository_path(value: &str) -> bool {
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

fn milestone_filename(timestamp_ms: u64, bytes: &[u8]) -> String {
    format!("{timestamp_ms:020}-{}.json", &digest_bytes(bytes)[..24])
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
#[path = "../../tests/unit/evidence/journal.rs"]
mod tests;
