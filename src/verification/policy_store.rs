//! Native local-operator policy authority, separate from repository-editable drafts.
//! A fixed generation path is the cross-process compare-and-set slot. An unfinished
//! highest slot blocks every reader/writer; it is never deleted or bypassed here.
//! This is bounded local state, not Team identity or same-OS-user tamper protection.
use super::policy::PolicySnapshot;
use crate::workspace::Workspace;
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::fs::File;
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SCHEMA_VERSION: u32 = 1;
const MAX_RECORDS: usize = 256;
const MAX_RECORD_BYTES: u64 = 2 * 1024 * 1024;
const MAX_MARKER_BYTES: u64 = 512;
const MAX_LIFETIME_MS: u64 = 365 * 24 * 60 * 60 * 1_000;

/// Construct only after a native operator authorization has been consumed.
/// No Deserialize implementation: JSON cannot construct this native call input.
/// This receipt records a local decision, not an authenticated Team identity.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct OperatorReceipt {
    request_id: String,
    decided_at_ms: u64,
}

impl OperatorReceipt {
    pub(crate) fn new(request_id: &str, decided_at_ms: u64) -> Result<Self> {
        let value = Self {
            request_id: request_id.to_owned(),
            decided_at_ms,
        };
        validate_receipt(&value.request_id, value.decided_at_ms)?;
        Ok(value)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredOperatorReceipt {
    request_id: String,
    decided_at_ms: u64,
}

impl From<OperatorReceipt> for StoredOperatorReceipt {
    fn from(value: OperatorReceipt) -> Self {
        Self {
            request_id: value.request_id,
            decided_at_ms: value.decided_at_ms,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PolicyAction {
    Active,
    Revoke,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NativePolicyRecord {
    schema_version: u32,
    generation: u64,
    previous_digest: Option<String>,
    workspace: String,
    root_digest: String,
    action: PolicyAction,
    snapshot: Option<PolicySnapshot>,
    operator_receipt: StoredOperatorReceipt,
    created_at_ms: u64,
    expires_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    checksum: String,
}

impl NativePolicyRecord {
    pub(crate) fn generation(&self) -> u64 {
        self.generation
    }

    pub(crate) fn created_at_ms(&self) -> u64 {
        self.created_at_ms
    }

    pub(crate) fn expires_at_ms(&self) -> Option<u64> {
        self.expires_at_ms
    }

    pub(crate) fn snapshot(&self) -> Option<&PolicySnapshot> {
        self.snapshot.as_ref()
    }

    pub(crate) fn is_active_at(&self, now_ms: u64) -> bool {
        self.action == PolicyAction::Active
            && self.snapshot.is_some()
            && now_ms >= self.created_at_ms
            && now_ms >= self.operator_receipt.decided_at_ms
            && now_ms != 0
            && self.expires_at_ms.is_none_or(|expiry| now_ms < expiry)
    }

    pub(crate) fn digest(&self) -> Result<String> {
        let mut unsigned = self.clone();
        unsigned.checksum.clear();
        Ok(digest_bytes(&serde_json::to_vec(&unsigned)?))
    }

    fn validate(&self, binding: &Binding, now_ms: u64) -> Result<()> {
        if self.schema_version != SCHEMA_VERSION
            || self.generation == 0
            || self.workspace != binding.workspace
            || self.root_digest != binding.root_digest
            || self.created_at_ms == 0
            || self.created_at_ms > now_ms
            || self.operator_receipt.decided_at_ms > self.created_at_ms
            || (self.generation == 1) != self.previous_digest.is_none()
            || self
                .previous_digest
                .as_ref()
                .is_some_and(|value| !valid_digest(value))
            || !valid_digest(&self.checksum)
            || self.digest()? != self.checksum
        {
            bail!("policy authority record binding, clock, generation or checksum is invalid");
        }
        validate_receipt(
            &self.operator_receipt.request_id,
            self.operator_receipt.decided_at_ms,
        )?;
        validate_expiry(self.created_at_ms, self.expires_at_ms)?;
        match (self.action, &self.snapshot) {
            (PolicyAction::Active, Some(snapshot)) => {
                snapshot.validate()?;
                if snapshot.workspace != binding.workspace
                    || snapshot.root_digest != binding.root_digest
                {
                    bail!("policy snapshot belongs to a different workspace root");
                }
            }
            (PolicyAction::Revoke, None) if self.expires_at_ms.is_none() => {}
            _ => bail!("policy action has inconsistent snapshot or expiry"),
        }
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CommitMarker {
    generation: u64,
    checksum: String,
}

struct Binding {
    workspace: String,
    root_digest: String,
}

struct Intent {
    action: PolicyAction,
    snapshot: Option<PolicySnapshot>,
    operator_receipt: StoredOperatorReceipt,
    expires_at_ms: Option<u64>,
}

pub(crate) fn workspace_root_digest(workspace: &Workspace) -> Result<String> {
    let root = workspace.root().canonicalize()?;
    if root != workspace.root() {
        bail!("workspace root changed before policy authority access");
    }
    let root = root
        .to_str()
        .context("policy authority requires a lossless UTF-8 root")?;
    let mut hasher = Sha256::new();
    hasher.update(b"wcode-policy-root-v1\0");
    hasher.update(root.as_bytes());
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

fn binding(workspace: &Workspace, selected_workspace: &str) -> Result<Binding> {
    if selected_workspace.trim().is_empty()
        || selected_workspace.trim() != selected_workspace
        || selected_workspace.len() > 300
        || selected_workspace.chars().any(char::is_control)
    {
        bail!("policy authority workspace alias is invalid");
    }
    Ok(Binding {
        workspace: selected_workspace.to_owned(),
        root_digest: workspace_root_digest(workspace)?,
    })
}

fn directory(workspace: &Workspace) -> Result<PathBuf> {
    let path =
        crate::evidence_store::workspace_state_directory(workspace)?.join("acceptance-policy");
    if path.is_absolute() {
        Ok(path)
    } else {
        Ok(std::env::current_dir()?.join(path))
    }
}

pub(crate) fn load(
    workspace: &Workspace,
    selected_workspace: &str,
) -> Result<Option<NativePolicyRecord>> {
    load_at(
        &directory(workspace)?,
        &binding(workspace, selected_workspace)?,
        now_ms()?,
    )
}

pub(crate) fn activate(
    workspace: &Workspace,
    selected_workspace: &str,
    expected_generation: u64,
    snapshot: PolicySnapshot,
    receipt: OperatorReceipt,
    expires_at_ms: Option<u64>,
) -> Result<NativePolicyRecord> {
    let binding = binding(workspace, selected_workspace)?;
    snapshot.validate()?;
    if snapshot.workspace != binding.workspace || snapshot.root_digest != binding.root_digest {
        bail!("policy snapshot belongs to a different workspace root");
    }
    persist_at(
        &directory(workspace)?,
        &binding,
        expected_generation,
        Intent {
            action: PolicyAction::Active,
            snapshot: Some(snapshot),
            operator_receipt: receipt.into(),
            expires_at_ms,
        },
        now_ms()?,
    )
}

pub(crate) fn revoke(
    workspace: &Workspace,
    selected_workspace: &str,
    expected_generation: u64,
    receipt: OperatorReceipt,
) -> Result<NativePolicyRecord> {
    persist_at(
        &directory(workspace)?,
        &binding(workspace, selected_workspace)?,
        expected_generation,
        Intent {
            action: PolicyAction::Revoke,
            snapshot: None,
            operator_receipt: receipt.into(),
            expires_at_ms: None,
        },
        now_ms()?,
    )
}

fn persist_at(
    directory: &Path,
    binding: &Binding,
    expected_generation: u64,
    intent: Intent,
    now_ms: u64,
) -> Result<NativePolicyRecord> {
    validate_receipt(
        &intent.operator_receipt.request_id,
        intent.operator_receipt.decided_at_ms,
    )?;
    if now_ms == 0 || intent.operator_receipt.decided_at_ms > now_ms {
        bail!("policy operator decision is future-dated or the clock is unavailable");
    }
    let records = load_records_at(directory, binding, now_ms)?;
    let current = records.last();
    let generation = expected_generation
        .checked_add(1)
        .context("policy generation exhausted")?;
    if let Some(record) = current {
        if record.generation == generation && same_intent(record, &intent) {
            sync_committed(directory, record.generation)?;
            return Ok(record.clone());
        }
    }
    validate_expiry(now_ms, intent.expires_at_ms)?;
    if current.map_or(0, |record| record.generation) != expected_generation {
        bail!("policy expected generation conflicts with the current authority");
    }
    if records
        .iter()
        .any(|record| record.operator_receipt.request_id == intent.operator_receipt.request_id)
    {
        bail!("policy operator request has already been committed");
    }
    if records.len() >= MAX_RECORDS {
        bail!("policy authority history capacity exhausted; no records were removed");
    }
    let mut record = NativePolicyRecord {
        schema_version: SCHEMA_VERSION,
        generation,
        previous_digest: current.map(|record| record.checksum.clone()),
        workspace: binding.workspace.clone(),
        root_digest: binding.root_digest.clone(),
        action: intent.action,
        snapshot: intent.snapshot,
        operator_receipt: intent.operator_receipt,
        created_at_ms: now_ms,
        expires_at_ms: intent.expires_at_ms,
        checksum: String::new(),
    };
    record.checksum = record.digest()?;
    record.validate(binding, now_ms)?;
    let bytes = serde_json::to_vec(&record)?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        bail!("policy authority record exceeds its size bound");
    }
    ensure_directory(directory)?;
    let path = record_path(directory, generation);
    let options = create_options();
    let mut file = match options.open(&path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let existing = load_at(directory, binding, now_ms)?
                .context("policy CAS slot exists without an authority head")?;
            if existing.generation == generation
                && same_intent(
                    &existing,
                    &Intent {
                        action: record.action,
                        snapshot: record.snapshot.clone(),
                        operator_receipt: record.operator_receipt.clone(),
                        expires_at_ms: record.expires_at_ms,
                    },
                )
            {
                sync_committed(directory, generation)?;
                return Ok(existing);
            }
            bail!("policy expected generation was committed by another operator request");
        }
        Err(error) => return Err(error).context("cannot reserve policy generation"),
    };
    // Do not remove a failed reservation. Its presence prevents another writer
    // from winning this generation or restoring an older active authority.
    file.write_all(&bytes)
        .context("cannot write policy generation")?;
    file.sync_all().context("cannot sync policy generation")?;
    drop(file);
    sync_directory(directory)?;
    let marker = serde_json::to_vec(&CommitMarker {
        generation,
        checksum: record.checksum.clone(),
    })?;
    let mut marker_file = create_options()
        .open(marker_path(directory, generation))
        .context("cannot create policy commit marker")?;
    marker_file
        .write_all(&marker)
        .context("cannot write policy commit marker")?;
    marker_file
        .sync_all()
        .context("cannot sync policy commit marker")?;
    drop(marker_file);
    sync_directory(directory)?;
    Ok(record)
}

fn same_intent(record: &NativePolicyRecord, intent: &Intent) -> bool {
    record.action == intent.action
        && record.snapshot == intent.snapshot
        && record.operator_receipt == intent.operator_receipt
        && record.expires_at_ms == intent.expires_at_ms
}

fn load_at(directory: &Path, binding: &Binding, now_ms: u64) -> Result<Option<NativePolicyRecord>> {
    Ok(load_records_at(directory, binding, now_ms)?.pop())
}

fn load_records_at(
    directory: &Path,
    binding: &Binding,
    now_ms: u64,
) -> Result<Vec<NativePolicyRecord>> {
    match fs::symlink_metadata(directory) {
        Ok(metadata) if !metadata.file_type().is_symlink() && metadata.is_dir() => {}
        Ok(_) => bail!("policy authority directory is not a regular directory"),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error).context("cannot inspect policy authority directory"),
    }
    if now_ms == 0 {
        bail!("policy authority clock is unavailable");
    }
    let generations = collect_generations(fs::read_dir(directory)?)?;
    let mut records = Vec::new();
    for generation in generations {
        if generation != records.len() as u64 + 1 {
            bail!("policy authority history has a missing generation");
        }
        let record: NativePolicyRecord = serde_json::from_slice(&read_regular(
            &record_path(directory, generation),
            MAX_RECORD_BYTES,
        )?)
        .context("invalid policy authority JSON")?;
        record.validate(binding, now_ms)?;
        if record.generation != generation {
            bail!("policy filename generation differs from its body");
        }
        let previous = records
            .last()
            .map(|record: &NativePolicyRecord| record.checksum.clone());
        if record.previous_digest != previous {
            bail!("policy authority digest chain is incomplete or conflicting");
        }
        let marker: CommitMarker = serde_json::from_slice(&read_regular(
            &marker_path(directory, generation),
            MAX_MARKER_BYTES,
        )?)
        .context("policy generation has no complete commit marker")?;
        if marker.generation != generation || marker.checksum != record.checksum {
            bail!("policy commit marker differs from its authority record");
        }
        if records.iter().any(|previous: &NativePolicyRecord| {
            previous.operator_receipt.request_id == record.operator_receipt.request_id
        }) {
            bail!("policy authority contains a repeated operator request");
        }
        records.push(record);
    }
    Ok(records)
}

fn collect_generations(
    entries: impl Iterator<Item = std::io::Result<fs::DirEntry>>,
) -> Result<Vec<u64>> {
    let mut records = std::collections::BTreeSet::new();
    let mut markers = std::collections::BTreeSet::new();
    for (index, entry) in entries.enumerate() {
        if index >= MAX_RECORDS * 2 {
            bail!("policy authority enumeration exceeds its entry bound");
        }
        let entry = entry.context("cannot enumerate policy authority entry")?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .context("policy authority entry name is invalid")?;
        let (stem, extension) = name
            .rsplit_once('.')
            .context("unexpected policy authority entry")?;
        let number = stem
            .strip_prefix('g')
            .context("unexpected policy authority entry")?;
        if number.len() != 20 || !number.bytes().all(|byte| byte.is_ascii_digit()) {
            bail!("policy authority generation name is invalid");
        }
        let generation = number
            .parse::<u64>()
            .context("policy generation name overflow")?;
        if generation == 0 {
            bail!("policy authority generation zero cannot be persisted");
        }
        match extension {
            "json" => {
                records.insert(generation);
            }
            "commit" => {
                markers.insert(generation);
            }
            _ => bail!("unexpected policy authority entry"),
        }
        if records.len() > MAX_RECORDS || markers.len() > MAX_RECORDS {
            bail!("policy authority history capacity exceeded");
        }
    }
    if !markers.is_subset(&records) {
        bail!("policy authority has an orphan commit marker");
    }
    Ok(records.into_iter().collect())
}

fn read_regular(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path).context("cannot inspect policy authority record")?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > limit {
        bail!("policy authority record is not a bounded regular file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() != 1 {
            bail!("policy authority record has multiple hard links");
        }
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    let file = options
        .open(path)
        .context("cannot open policy authority record")?;
    if !file.metadata()?.is_file() {
        bail!("policy authority record changed type");
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        bail!("policy authority record exceeds its size bound");
    }
    Ok(bytes)
}

fn create_options() -> OpenOptions {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    options
}

fn ensure_directory(directory: &Path) -> Result<()> {
    let mut ancestor = directory.to_path_buf();
    let mut missing = Vec::new();
    loop {
        match fs::symlink_metadata(&ancestor) {
            Ok(metadata) => {
                if ancestor == directory && metadata.file_type().is_symlink() {
                    bail!("policy authority directory is a symlink");
                }
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(
                    ancestor
                        .file_name()
                        .context("policy directory has no parent")?
                        .to_owned(),
                );
                if !ancestor.pop() {
                    return Err(error.into());
                }
            }
            Err(error) => return Err(error).context("cannot inspect policy authority directory"),
        }
    }
    // Existing configured ancestors may have canonical aliases (for example
    // macOS /var). New components are created below their physical directory.
    let mut current = ancestor.canonicalize()?;
    if !current.is_dir() {
        bail!("policy authority ancestor is not a directory");
    }
    for leaf in missing.into_iter().rev() {
        let parent = current.clone();
        current.push(leaf);
        let mut builder = fs::DirBuilder::new();
        builder.recursive(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&current) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error).context("cannot create policy authority directory"),
        }
        let metadata = fs::symlink_metadata(&current)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            bail!("policy authority directory changed type");
        }
        sync_directory(&parent)?;
    }
    let metadata = fs::symlink_metadata(directory)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("policy authority directory changed type");
    }
    Ok(())
}

fn sync_committed(directory: &Path, generation: u64) -> Result<()> {
    for path in [
        record_path(directory, generation),
        marker_path(directory, generation),
    ] {
        let mut options = OpenOptions::new();
        options.read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(libc::O_NOFOLLOW);
        }
        options.open(path)?.sync_all()?;
    }
    sync_directory(directory)
}

#[cfg(unix)]
fn sync_directory(directory: &Path) -> Result<()> {
    File::open(directory)?
        .sync_all()
        .context("cannot sync policy authority directory")
}

#[cfg(not(unix))]
fn sync_directory(_directory: &Path) -> Result<()> {
    // File contents and markers are synced on Windows. std does not supply the
    // Unix directory-fsync guarantee; this store does not claim equivalent
    // power-loss persistence of directory entries on Windows.
    Ok(())
}

fn record_path(directory: &Path, generation: u64) -> PathBuf {
    directory.join(format!("g{generation:020}.json"))
}

fn marker_path(directory: &Path, generation: u64) -> PathBuf {
    directory.join(format!("g{generation:020}.commit"))
}

fn validate_receipt(request_id: &str, decided_at_ms: u64) -> Result<()> {
    if request_id.is_empty()
        || request_id.len() > 160
        || !request_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.:".contains(&byte))
        || decided_at_ms == 0
    {
        bail!("policy operator receipt is invalid");
    }
    Ok(())
}

fn validate_expiry(created_at_ms: u64, expires_at_ms: Option<u64>) -> Result<()> {
    if let Some(expiry) = expires_at_ms {
        if expiry <= created_at_ms || expiry - created_at_ms > MAX_LIFETIME_MS {
            bail!("policy expiry is not a bounded future timestamp");
        }
    }
    Ok(())
}

fn valid_digest(value: &str) -> bool {
    value.strip_prefix("sha256:").is_some_and(|digest| {
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    })
}

fn digest_bytes(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}

fn now_ms() -> Result<u64> {
    let value = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("policy authority clock precedes Unix epoch")?
        .as_millis();
    let value = u64::try_from(value).context("policy authority clock exceeds its bound")?;
    if value == 0 {
        bail!("policy authority clock is unavailable");
    }
    Ok(value)
}

#[cfg(test)]
#[path = "../../tests/unit/verification/policy_store.rs"]
mod tests;
