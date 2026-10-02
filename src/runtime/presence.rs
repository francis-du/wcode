use crate::auth::authority_state_root;
use crate::monitor::{MonitorConnectionStatus, TaskMonitor};
use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use url::Url;
use uuid::Uuid;

const SCHEMA_VERSION: u32 = 1;
const STORE_VERSION: &str = "v1";
const MAX_RECORD_BYTES: u64 = 64 * 1024;
const MAX_SCAN_ENTRIES: usize = 128;
const MAX_ACTIVE_RECORDS: usize = 64;
const MAX_WORKSPACES: usize = 64;
const ACTIVE_TTL_MS: u64 = 20_000;
const MAX_FUTURE_SKEW_MS: u64 = 5 * 60 * 1_000;
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RuntimeTransport {
    Http,
    Stdio,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct RuntimePresenceRecord {
    pub schema_version: u32,
    pub instance_id: String,
    pub version: String,
    pub transport: RuntimeTransport,
    pub started_at_ms: u64,
    pub updated_at_ms: u64,
    pub workspaces: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local_url: Option<String>,
    pub mcp_connected: bool,
    pub initialize_count: u64,
    pub last_mcp_seen_seconds_ago: Option<u64>,
    pub oauth_authorized: bool,
    pub public_endpoint: Option<String>,
    pub public_url_healthy: Option<bool>,
    pub tunnel_running: Option<bool>,
    pub active_tasks: u64,
    pub queued_tasks: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_verifications: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued_verifications: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_jobs: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queued_jobs: Option<u64>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub(crate) struct RuntimePresenceSnapshot {
    pub schema_version: u32,
    pub records: Vec<RuntimePresenceRecord>,
    pub partial: bool,
    pub invalid_records: u64,
    pub stale_records: u64,
    pub scan_truncated: bool,
}

#[derive(Clone)]
pub(crate) struct RuntimePresencePublisher {
    inner: Arc<PublisherInner>,
}

struct PublisherInner {
    publish_lock: std::sync::Mutex<bool>,
    directory: PathBuf,
    path: PathBuf,
    instance_id: String,
    version: String,
    transport: RuntimeTransport,
    started_at_ms: u64,
    workspaces: Vec<String>,
    local_url: Option<String>,
}

impl RuntimePresencePublisher {
    pub(crate) fn new(
        instance_id: &str,
        transport: RuntimeTransport,
        workspaces: impl IntoIterator<Item = String>,
        local_url: Option<&str>,
    ) -> Result<Self> {
        let root = authority_state_root()?;
        Self::new_at_root(
            &root,
            instance_id,
            transport,
            workspaces,
            local_url,
            now_ms(),
        )
    }

    fn new_at_root(
        root: &Path,
        instance_id: &str,
        transport: RuntimeTransport,
        workspaces: impl IntoIterator<Item = String>,
        local_url: Option<&str>,
        started_at_ms: u64,
    ) -> Result<Self> {
        validate_instance_id(instance_id)?;
        let directory = ensure_directory(root)?;
        let mut workspaces = workspaces.into_iter().collect::<Vec<_>>();
        workspaces.sort();
        workspaces.dedup();
        if workspaces.len() > MAX_WORKSPACES
            || workspaces
                .iter()
                .any(|workspace| !bounded_text(workspace, 200))
        {
            bail!("runtime presence workspace projection is invalid or exceeds its bound");
        }
        let local_url = local_url.map(validate_local_url).transpose()?;
        let path = directory.join(record_filename(instance_id));
        Ok(Self {
            inner: Arc::new(PublisherInner {
                publish_lock: std::sync::Mutex::new(false),
                directory,
                path,
                instance_id: instance_id.to_owned(),
                version: env!("CARGO_PKG_VERSION").to_owned(),
                transport,
                started_at_ms,
                workspaces,
                local_url,
            }),
        })
    }

    pub(crate) fn publish(&self, monitor: &TaskMonitor) -> Result<()> {
        self.publish_snapshot(|| monitor.connection_status())
    }

    fn publish_snapshot(&self, snapshot: impl FnOnce() -> MonitorConnectionStatus) -> Result<()> {
        // Capture and publish under the same lock. Locking only the rename would
        // let a delayed heartbeat replace a newer connected state with old data.
        let removed = self
            .inner
            .publish_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime presence writer lock is poisoned"))?;
        if *removed {
            return Ok(());
        }
        self.publish_status(&snapshot(), now_ms())
    }

    fn publish_status(&self, status: &MonitorConnectionStatus, updated_at_ms: u64) -> Result<()> {
        let record = RuntimePresenceRecord {
            schema_version: SCHEMA_VERSION,
            instance_id: self.inner.instance_id.clone(),
            version: self.inner.version.clone(),
            transport: self.inner.transport,
            started_at_ms: self.inner.started_at_ms,
            updated_at_ms,
            workspaces: self.inner.workspaces.clone(),
            local_url: self.inner.local_url.clone(),
            mcp_connected: status.chatgpt_initialized,
            initialize_count: status.initialize_count,
            last_mcp_seen_seconds_ago: status.last_mcp_seen_seconds_ago,
            oauth_authorized: status.oauth_authorized,
            public_endpoint: status
                .public_endpoint
                .as_deref()
                .filter(|value| bounded_text(value, 80))
                .map(str::to_owned),
            public_url_healthy: status.public_url_healthy,
            tunnel_running: status.tunnel_running,
            active_tasks: status.active_tasks,
            queued_tasks: status.queued_tasks,
            active_verifications: Some(status.active_verifications),
            queued_verifications: Some(status.queued_verifications),
            active_jobs: Some(status.active_jobs),
            queued_jobs: Some(status.queued_jobs),
        };
        validate_record(&record)?;
        publish_record(&self.inner.directory, &self.inner.path, &record)
    }

    pub(crate) fn spawn_heartbeat(&self, monitor: TaskMonitor) -> tokio::task::JoinSet<()> {
        let publisher = self.clone();
        // Dropping the session must abort, rather than detach, its heartbeat.
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
            let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let _ = publisher.publish(&monitor);
            }
        });
        tasks
    }

    pub(crate) fn remove(&self) -> Result<()> {
        let mut removed = self
            .inner
            .publish_lock
            .lock()
            .map_err(|_| anyhow::anyhow!("runtime presence writer lock is poisoned"))?;
        // Serialize cleanup with snapshots and reject all later publications.
        *removed = true;
        remove_owned_record(&self.inner.path, &self.inner.instance_id)
    }
}

impl Drop for PublisherInner {
    fn drop(&mut self) {
        let _ = remove_owned_record(&self.path, &self.instance_id);
    }
}

pub(crate) fn snapshot() -> Result<RuntimePresenceSnapshot> {
    let root = authority_state_root()?;
    snapshot_at(&root, now_ms())
}

fn snapshot_at(root: &Path, now: u64) -> Result<RuntimePresenceSnapshot> {
    match fs::symlink_metadata(root) {
        Ok(_) => ensure_existing_directory(root)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RuntimePresenceSnapshot {
                schema_version: SCHEMA_VERSION,
                ..Default::default()
            });
        }
        Err(error) => return Err(error).context("cannot inspect runtime presence state root"),
    }

    let parent = root.join("runtime-presence");
    match fs::symlink_metadata(&parent) {
        Ok(_) => ensure_existing_directory(&parent)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RuntimePresenceSnapshot {
                schema_version: SCHEMA_VERSION,
                ..Default::default()
            });
        }
        Err(error) => return Err(error).context("cannot inspect runtime presence parent"),
    }

    let directory = parent.join(STORE_VERSION);
    match fs::symlink_metadata(&directory) {
        Ok(_) => ensure_existing_directory(&directory)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(RuntimePresenceSnapshot {
                schema_version: SCHEMA_VERSION,
                ..Default::default()
            });
        }
        Err(error) => return Err(error).context("cannot inspect runtime presence directory"),
    }
    let mut paths = Vec::new();
    let mut invalid_records = 0_u64;
    let mut scan_truncated = false;
    let mut scanned = 0_usize;
    for entry in fs::read_dir(&directory)
        .with_context(|| format!("cannot list runtime presence {}", directory.display()))?
    {
        scanned = scanned.saturating_add(1);
        if scanned > MAX_SCAN_ENTRIES {
            scan_truncated = true;
            break;
        }
        match entry {
            Ok(entry) if is_staging_entry(&entry) => {}
            Ok(entry) => paths.push(entry.path()),
            Err(_) => invalid_records = invalid_records.saturating_add(1),
        }
    }
    paths.sort();

    let mut records = Vec::new();
    let mut stale_records = 0_u64;
    for path in paths {
        match read_record(&path) {
            Ok(record)
                if record.updated_at_ms <= now.saturating_add(MAX_FUTURE_SKEW_MS)
                    && now.saturating_sub(record.updated_at_ms) <= ACTIVE_TTL_MS =>
            {
                records.push(record)
            }
            Ok(_) => stale_records = stale_records.saturating_add(1),
            Err(_) => invalid_records = invalid_records.saturating_add(1),
        }
    }
    records.sort_by(|left, right| {
        right
            .updated_at_ms
            .cmp(&left.updated_at_ms)
            .then_with(|| left.instance_id.cmp(&right.instance_id))
    });
    if records.len() > MAX_ACTIVE_RECORDS {
        records.truncate(MAX_ACTIVE_RECORDS);
        scan_truncated = true;
    }
    let partial = invalid_records > 0 || scan_truncated;
    Ok(RuntimePresenceSnapshot {
        schema_version: SCHEMA_VERSION,
        records,
        partial,
        invalid_records,
        stale_records,
        scan_truncated,
    })
}

fn publish_record(directory: &Path, target: &Path, record: &RuntimePresenceRecord) -> Result<()> {
    ensure_existing_directory(directory)?;
    match fs::symlink_metadata(target) {
        Ok(_) => {
            ensure_safe_regular_file(target, MAX_RECORD_BYTES)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("cannot inspect runtime presence target"),
    }
    let bytes = serde_json::to_vec(record).context("cannot encode runtime presence")?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        bail!("runtime presence record exceeds size bound");
    }
    let temp = directory.join(format!(".presence-{}.tmp", Uuid::new_v4().simple()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let saved = (|| -> Result<()> {
        let mut file = options
            .open(&temp)
            .with_context(|| format!("cannot create runtime presence temp {}", temp.display()))?;
        file.write_all(&bytes)
            .context("cannot write runtime presence record")?;
        drop(file);
        // rename replaces an existing file on Windows too. Removing it first
        // would expose a missing-record window to readers.
        fs::rename(&temp, target).context("cannot publish runtime presence record")?;
        Ok(())
    })();
    if saved.is_err() {
        let _ = fs::remove_file(&temp);
    }
    saved
}

fn read_record(path: &Path) -> Result<RuntimePresenceRecord> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options
        .open(path)
        .with_context(|| format!("cannot open runtime presence {}", path.display()))?;
    let metadata = file
        .metadata()
        .context("cannot inspect open runtime presence record")?;
    validate_regular_file(&metadata, MAX_RECORD_BYTES)?;
    if metadata.len() == 0 {
        bail!("empty runtime presence record");
    }
    // Read the same handle that was validated. A concurrent rename may unlink
    // this inode, but its complete contents remain a valid snapshot.
    let mut bytes = Vec::new();
    file.take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)
        .context("cannot read runtime presence record")?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        bail!("runtime presence record exceeds size bound");
    }
    let record: RuntimePresenceRecord =
        serde_json::from_slice(&bytes).context("cannot decode runtime presence")?;
    validate_record(&record)?;
    let expected = record_filename(&record.instance_id);
    if path.file_name().and_then(|name| name.to_str()) != Some(expected.as_str()) {
        bail!("runtime presence filename does not match record identity");
    }
    Ok(record)
}

fn validate_record(record: &RuntimePresenceRecord) -> Result<()> {
    validate_instance_id(&record.instance_id)?;
    if record.schema_version != SCHEMA_VERSION
        || !bounded_text(&record.version, 64)
        || record.started_at_ms == 0
        || record.updated_at_ms < record.started_at_ms
        || record.workspaces.len() > MAX_WORKSPACES
        || record
            .workspaces
            .iter()
            .any(|workspace| !bounded_text(workspace, 200))
        || record
            .public_endpoint
            .as_deref()
            .is_some_and(|value| !bounded_text(value, 80))
    {
        bail!("invalid runtime presence record");
    }
    if let Some(url) = record.local_url.as_deref() {
        validate_local_url(url)?;
    }
    Ok(())
}

fn validate_instance_id(value: &str) -> Result<()> {
    if !bounded_text(value, 128)
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        bail!("invalid runtime presence instance id");
    }
    Ok(())
}

fn validate_local_url(value: &str) -> Result<String> {
    if value.len() > 256 || value.chars().any(char::is_control) {
        bail!("invalid local runtime URL");
    }
    let url = Url::parse(value).context("invalid local runtime URL")?;
    let loopback = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "::1"));
    if url.scheme() != "http"
        || !loopback
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || !matches!(url.path(), "" | "/")
    {
        bail!("runtime presence only accepts a loopback HTTP root URL");
    }
    Ok(value.to_owned())
}

fn bounded_text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

fn record_filename(instance_id: &str) -> String {
    format!("{:x}.json", Sha256::digest(instance_id.as_bytes()))
}

fn is_staging_entry(entry: &fs::DirEntry) -> bool {
    entry
        .file_name()
        .to_str()
        .is_some_and(|name| name.starts_with(".presence-") && name.ends_with(".tmp"))
}

fn ensure_directory(root: &Path) -> Result<PathBuf> {
    match fs::symlink_metadata(root) {
        Ok(_) => ensure_existing_directory(root)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            reject_symlink_ancestors(root)?;
            fs::create_dir_all(root)
                .with_context(|| format!("cannot create runtime state root {}", root.display()))?;
            ensure_existing_directory(root)?;
            protect_directory(root)?;
        }
        Err(error) => return Err(error).context("cannot inspect runtime state root"),
    }

    let parent = root.join("runtime-presence");
    ensure_child_directory(&parent)?;
    let directory = parent.join(STORE_VERSION);
    ensure_child_directory(&directory)?;
    Ok(directory)
}

fn ensure_child_directory(path: &Path) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => ensure_existing_directory(path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            match fs::create_dir(path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!("cannot create runtime presence {}", path.display())
                    })
                }
            }
            // Another runtime may have created it; still reject links/files.
            ensure_existing_directory(path)?;
        }
        Err(error) => return Err(error).context("cannot inspect runtime presence directory"),
    }
    protect_directory(path)
}

fn reject_symlink_ancestors(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    bail!("runtime state root ancestors must not be symlinks");
                }
                if ancestor != path && !metadata.is_dir() {
                    bail!("runtime state root ancestor is not a directory");
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("cannot inspect runtime state root ancestor"),
        }
    }
    Ok(())
}

#[cfg(unix)]
fn protect_directory(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .context("cannot protect runtime presence directory")
}

#[cfg(not(unix))]
fn protect_directory(_path: &Path) -> Result<()> {
    Ok(())
}

fn ensure_existing_directory(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("cannot inspect runtime presence {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("runtime presence path is not a regular directory");
    }
    Ok(())
}

fn ensure_safe_regular_file(path: &Path, max_bytes: u64) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("cannot inspect runtime presence {}", path.display()))?;
    validate_regular_file(&metadata, max_bytes)?;
    Ok(metadata)
}

fn validate_regular_file(metadata: &fs::Metadata, max_bytes: u64) -> Result<()> {
    if metadata.file_type().is_symlink() || !metadata.is_file() || metadata.len() > max_bytes {
        bail!("runtime presence record is not a bounded regular file");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if metadata.nlink() > 1 {
            bail!("runtime presence record has multiple hard links");
        }
    }
    Ok(())
}

fn remove_owned_record(path: &Path, instance_id: &str) -> Result<()> {
    match fs::symlink_metadata(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error).context("cannot inspect runtime presence cleanup target"),
    }
    let record = read_record(path)?;
    if record.instance_id != instance_id {
        bail!("runtime presence ownership changed before cleanup");
    }
    fs::remove_file(path).context("cannot remove runtime presence record")
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}

#[cfg(test)]
#[path = "../../tests/unit/runtime/presence.rs"]
mod tests;
