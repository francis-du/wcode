//! Private, bounded delivery queue. Retained digests deduplicate signed bodies,
//! not unsigned delivery IDs. Filesystem state is not native Acceptance evidence.
//! No silent rebootstrap, deletion, exactly-once remote promise or expired lease takeover.
use super::*;
use anyhow::{ensure, Context};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[path = "inbox_archive.rs"]
mod archive;
#[path = "receiver.rs"]
mod receiver;
pub use archive::InboxArchiveCheckpoint;

const MAX_EVENTS: usize = 128;
const MAX_STATE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_ATTEMPTS: u32 = 5;
const HEADER_NAMES: [&str; 4] = [
    "x-hub-signature-256",
    "x-github-event",
    "x-github-delivery",
    "content-type",
];

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    repository: String,
    repository_id: u64,
    app_id: u64,
    installation_id: u64,
    check_name: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum DeliveryState {
    Queued,
    Publishing,
    Retry,
    Completed,
    Exhausted,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    body_digest: String,
    body_base64: String,
    headers: [String; 4],
    received_at_ms: u64,
    updated_at_ms: u64,
    state: DeliveryState,
    attempts: u32,
    retry_at_ms: u64,
}

impl Entry {
    fn body(&self) -> Result<Vec<u8>> {
        ensure!(
            self.body_base64.len() <= MAX_WEBHOOK_BYTES.div_ceil(3) * 4,
            "inbox encoded body exceeds its bound"
        );
        let bytes = STANDARD
            .decode(&self.body_base64)
            .map_err(|_| anyhow!("inbox body encoding is invalid"))?;
        ensure!(
            !bytes.is_empty()
                && bytes.len() <= MAX_WEBHOOK_BYTES
                && digest(&bytes) == self.body_digest,
            "inbox body identity is invalid"
        );
        Ok(bytes)
    }

    fn header_map(&self) -> Result<HeaderMap> {
        let mut map = HeaderMap::new();
        for (name, value) in HEADER_NAMES.iter().zip(&self.headers) {
            ensure!(
                !value.is_empty() && value.len() <= 256,
                "inbox header exceeds its bound"
            );
            map.insert(
                reqwest::header::HeaderName::from_static(name),
                HeaderValue::from_str(value).map_err(|_| anyhow!("inbox header is invalid"))?,
            );
        }
        Ok(map)
    }
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct State {
    schema_version: u32,
    root_digest: String,
    binding: Binding,
    generation: u64,
    updated_at_ms: u64,
    entries: Vec<Entry>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    archived: Vec<archive::ReplayTombstone>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    state: State,
    checksum: String,
}

#[derive(Debug, Serialize)]
pub struct InboxEnqueueResult {
    pub body_digest: String,
    pub duplicate: bool,
    pub reauthenticated: bool,
    pub archived: bool,
    pub generation: u64,
    pub retained: usize,
    pub current_acceptance: bool,
}

/// One trusted installation/repository/Check binding per private directory.
/// Original signed bodies are retained privately to reauthenticate on every retry.
/// Status never returns payloads, signatures, credentials or a cached green receipt.
#[derive(Clone)]
pub struct GitHubInbox {
    root: PathBuf,
    root_digest: String,
    binding: Binding,
    config: GitHubConfig,
}

// Explicit unlock matters: closing one File does not release an open-file-
// description lock while a duplicated/inherited descriptor remains open.
struct InboxLock(File);

impl std::ops::Deref for InboxLock {
    type Target = File;
    fn deref(&self) -> &File {
        &self.0
    }
}

impl Drop for InboxLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

impl GitHubInbox {
    pub fn initialize(root: &Path, config: GitHubConfig, installation_id: u64) -> Result<Self> {
        ensure!(
            installation_id > 0,
            "inbox installation identity must be positive"
        );
        validate_root(root, true)?;
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(root)
            .context("inbox initialization requires a new directory")?;
        let inbox = Self::configured(root, config, installation_id)?;
        let lock = create_file(&root.join("inbox.lock"))?;
        lock.sync_all()?;
        create_file(&root.join("worker.lock"))?.sync_all()?;
        lock.try_lock()
            .map_err(|_| anyhow!("inbox busy; no work started"))?;
        let _lock = InboxLock(lock);
        let state = State {
            schema_version: 1,
            root_digest: inbox.root_digest.clone(),
            binding: inbox.binding.clone(),
            generation: 1,
            updated_at_ms: now_ms()?,
            entries: Vec::new(),
            archived: Vec::new(),
        };
        inbox.write(&state, true)?;
        #[cfg(unix)]
        File::open(root.parent().context("inbox parent missing")?)?.sync_all()?;
        Ok(inbox)
    }

    pub fn open(root: &Path, config: GitHubConfig, installation_id: u64) -> Result<Self> {
        let inbox = Self::configured(root, config, installation_id)?;
        inbox.read(now_ms()?)?;
        Ok(inbox)
    }

    fn configured(root: &Path, config: GitHubConfig, installation_id: u64) -> Result<Self> {
        ensure!(
            installation_id > 0,
            "inbox installation identity must be positive"
        );
        validate_root(root, false)?;
        private_metadata(root, true)?;
        let root_digest = digest(
            root.to_str()
                .context("inbox root must be UTF-8")?
                .as_bytes(),
        );
        let binding = Binding {
            repository: config.repository.key(),
            repository_id: config.repository_id,
            app_id: config.app_id,
            installation_id,
            check_name: config.check_name.clone(),
        };
        Ok(Self {
            root: root.to_path_buf(),
            root_digest,
            binding,
            config,
        })
    }

    pub fn enqueue(
        &self,
        headers: &HeaderMap,
        body: &[u8],
        key: &[u8],
    ) -> Result<InboxEnqueueResult> {
        let delivery = VerifiedPullRequestDelivery::verify(
            &self.config,
            self.binding.installation_id,
            headers,
            body,
            key,
        )?;
        // Authentication precedes persistence and lock acquisition.
        let _lock = self.lock()?;
        let now = now_ms()?;
        let mut state = self.read(now)?;
        let existing = state
            .entries
            .iter()
            .position(|entry| entry.body_digest == delivery.body_digest());
        let archived = state
            .archived
            .iter()
            .any(|entry| entry.body_digest == delivery.body_digest());
        let duplicate = existing.is_some() || archived;
        let mut reauthenticated = false;
        let mut stored_headers: [String; 4] = Default::default();
        for (index, name) in HEADER_NAMES.iter().enumerate() {
            stored_headers[index] = headers[*name].to_str()?.to_owned();
        }
        if let Some(index) = existing {
            let entry = &mut state.entries[index];
            if matches!(entry.state, DeliveryState::Queued | DeliveryState::Retry)
                && VerifiedPullRequestDelivery::verify(
                    &self.config,
                    self.binding.installation_id,
                    &entry.header_map()?,
                    &entry.body()?,
                    key,
                )
                .is_err()
            {
                // The incoming raw bytes have already authenticated with the
                // current key. Replace only their obsolete authentication header
                // set: do not reset attempts, shorten backoff, replay a terminal
                // record, or rewrite an in-flight worker's delivery.
                ensure!(entry.body()? == body, "inbox duplicate body bytes conflict");
                entry.headers = stored_headers;
                entry.updated_at_ms = now;
                if entry.attempts > 0 {
                    entry.retry_at_ms = entry.retry_at_ms.max(now);
                }
                self.advance(&mut state, now)?;
                reauthenticated = true;
            }
        } else if !archived {
            ensure!(
                state.entries.len() < MAX_EVENTS,
                "inbox capacity reached; retained replay tombstones were not removed"
            );
            state.entries.push(Entry {
                body_digest: delivery.body_digest().into(),
                body_base64: STANDARD.encode(body),
                headers: stored_headers,
                received_at_ms: now,
                updated_at_ms: now,
                state: DeliveryState::Queued,
                attempts: 0,
                retry_at_ms: 0,
            });
            self.advance(&mut state, now)?;
        }
        if duplicate && !reauthenticated {
            // Also confirm an earlier rename whose directory sync may have failed.
            open_regular(&self.root.join("inbox.json"), true, MAX_STATE_BYTES)?.sync_all()?;
            #[cfg(unix)]
            File::open(&self.root)?.sync_all()?;
        }
        Ok(InboxEnqueueResult {
            body_digest: delivery.body_digest().into(),
            duplicate,
            reauthenticated,
            archived,
            generation: state.generation,
            retained: state.entries.len(),
            current_acceptance: false,
        })
    }

    pub fn status(&self) -> Result<Value> {
        // Atomic snapshots permit diagnostics even while a worker owns the lock.
        let state = self.read(now_ms()?)?;
        let entries = state
            .entries
            .iter()
            .map(|entry| {
                json!({
                    "body_digest": entry.body_digest, "state": entry.state,
                    "attempts": entry.attempts, "retry_at_ms": entry.retry_at_ms,
                    "received_at_ms": entry.received_at_ms, "updated_at_ms": entry.updated_at_ms,
                })
            })
            .collect::<Vec<_>>();
        Ok(
            json!({"authority":"delivery_queue_observation", "current_acceptance":false,
            "generation":state.generation, "capacity":MAX_EVENTS,
            "retained":entries.len(), "entries":entries,
            "archived_replay_tombstones":state.archived.len(),
            "replay_index_capacity":archive::MAX_REPLAY_TOMBSTONES,
            "protected_signed_body_digests":state.entries.len() + state.archived.len(),
            "publishing_semantics":"in_flight_or_interrupted; never a success receipt",
            "replay_scope":"retained_signed_body_digests_in_this_intact_inbox"}),
        )
    }

    pub async fn publish_next(
        &self,
        provider: &GitHubProvider,
        root: &Path,
        workspace_id: &str,
        key: &[u8],
    ) -> Result<Option<GitHubGateReceipt>> {
        ensure!(
            self.binding.repository == provider.config.repository.key()
                && self.binding.repository_id == provider.config.repository_id
                && self.binding.app_id == provider.config.app_id
                && self.binding.check_name == provider.config.check_name,
            "inbox belongs to another publisher"
        );
        let candidate = root
            .canonicalize()
            .context("inbox candidate is unavailable")?;
        ensure!(
            !candidate.starts_with(&self.root) && !self.root.starts_with(&candidate),
            "inbox and candidate must not overlap"
        );
        self.process_admitted_at(
            key,
            now_ms()?,
            |delivery| PublicationGuard::acquire(provider, delivery.target()).map(Arc::new),
            |delivery, admission| async move {
                provider
                    .publish_delivery_with_guard(
                        &delivery,
                        &candidate,
                        workspace_id,
                        Some(admission),
                    )
                    .await
            },
        )
        .await
    }

    #[cfg(test)]
    async fn process_at<F, Fut>(
        &self,
        key: &[u8],
        now: u64,
        publish: F,
    ) -> Result<Option<GitHubGateReceipt>>
    where
        F: FnOnce(VerifiedPullRequestDelivery) -> Fut,
        Fut: Future<Output = Result<GitHubGateReceipt>>,
    {
        self.process_admitted_at(key, now, |_| Ok(()), |delivery, ()| publish(delivery))
            .await
    }

    async fn process_admitted_at<F, Fut, A, G>(
        &self,
        key: &[u8],
        now: u64,
        mut admit: A,
        publish: F,
    ) -> Result<Option<GitHubGateReceipt>>
    where
        A: FnMut(&VerifiedPullRequestDelivery) -> Result<G>,
        G: Clone,
        F: FnOnce(VerifiedPullRequestDelivery, G) -> Fut,
        Fut: Future<Output = Result<GitHubGateReceipt>>,
    {
        ensure!(
            (16..=4096).contains(&key.len()),
            "inbox worker verification key is invalid"
        );
        // Keep the OS lock for the entire remote operation: no expiring lease can
        // let another worker overlap an in-flight request. Process death releases it.
        let _worker = self.lock_file("worker.lock")?;
        let state_lock = self.lock()?;
        let now = now.max(now_ms()?);
        let mut state = self.read(now)?;
        let mut recovered = false;
        for entry in &mut state.entries {
            if entry.state == DeliveryState::Publishing && entry.attempts == MAX_ATTEMPTS {
                entry.state = DeliveryState::Exhausted;
                entry.updated_at_ms = now;
                entry.retry_at_ms = now;
                recovered = true;
            }
        }
        if recovered {
            self.advance(&mut state, now)?;
        }
        let mut selected = None;
        let mut authentication_blocked = 0usize;
        let mut publication_blocked = false;
        // A retained delivery signed with a previous key must not starve later
        // deliveries authenticated by the current key. Inspect only the bounded
        // snapshot and leave unverifiable entries, attempts and tombstones intact.
        // Storage/encoding corruption still fails the whole read; only a failed
        // delivery authentication is skipped, never treated as completed or proof.
        for (index, entry) in state.entries.iter().enumerate() {
            if !matches!(
                entry.state,
                DeliveryState::Queued | DeliveryState::Retry | DeliveryState::Publishing
            ) || entry.attempts >= MAX_ATTEMPTS
                || now < entry.retry_at_ms
            {
                continue;
            }
            match VerifiedPullRequestDelivery::verify(
                &self.config,
                self.binding.installation_id,
                &entry.header_map()?,
                &entry.body()?,
                key,
            ) {
                Ok(delivery) => match admit(&delivery) {
                    Ok(admitted) => {
                        selected = Some((index, delivery, admitted));
                        break;
                    }
                    Err(error) if error.is::<PublicationBusy>() => publication_blocked = true,
                    Err(error) => return Err(error),
                },
                Err(_) => authentication_blocked += 1,
            }
        }
        let Some((index, delivery, admitted)) = selected else {
            if publication_blocked {
                return Err(PublicationBusy.into());
            }
            ensure!(
                authentication_blocked == 0,
                "inbox has {authentication_blocked} due deliveries that cannot authenticate with the current key; retained for operator reconciliation"
            );
            return Ok(None);
        };
        // Local contention is not an attempted publication. Acquire before
        // changing attempts/deadlines, and retain admission through completion
        // persistence as well as every remote request.
        let entry = &mut state.entries[index];
        entry.attempts += 1;
        entry.state = DeliveryState::Publishing;
        entry.updated_at_ms = now;
        entry.retry_at_ms = now
            .checked_add(backoff_ms(entry.attempts))
            .context("inbox clock overflow")?;
        let body_digest = entry.body_digest.clone();
        let attempt = entry.attempts;
        self.advance(&mut state, now)?;
        drop(state_lock);
        let result = publish(delivery, admitted.clone()).await;
        let _state_lock = self.lock()?;
        let finished = now_ms()?.max(now);
        let mut state = self.read(finished)?;
        let entry = state
            .entries
            .iter_mut()
            .find(|entry| entry.body_digest == body_digest)
            .context("inbox event disappeared during publication")?;
        ensure!(
            entry.state == DeliveryState::Publishing && entry.attempts == attempt,
            "inbox attempt changed during publication"
        );
        entry.updated_at_ms = finished;
        entry.state = if result.is_ok() {
            DeliveryState::Completed
        } else if entry.attempts == MAX_ATTEMPTS {
            DeliveryState::Exhausted
        } else {
            DeliveryState::Retry
        };
        entry.retry_at_ms = finished
            .checked_add(backoff_ms(entry.attempts))
            .context("inbox clock overflow")?;
        self.advance(&mut state, finished)?;
        drop(admitted);
        result
            .map(Some)
            .map_err(|_| anyhow!("inbox publication unavailable; inspect retry or exhausted state"))
    }

    fn advance(&self, state: &mut State, now: u64) -> Result<()> {
        state.generation = state
            .generation
            .checked_add(1)
            .context("inbox generation exhausted")?;
        state.updated_at_ms = now;
        self.write(state, false)
    }

    fn lock(&self) -> Result<InboxLock> {
        self.lock_file("inbox.lock")
    }

    fn lock_file(&self, name: &str) -> Result<InboxLock> {
        self.check_directory()?;
        let file = open_regular(&self.root.join(name), true, 0)?;
        file.try_lock()
            .map_err(|_| anyhow!("inbox busy; no work started"))?;
        Ok(InboxLock(file))
    }

    fn check_directory(&self) -> Result<()> {
        validate_root(&self.root, false)?;
        private_metadata(&self.root, true)?;
        for (index, entry) in fs::read_dir(&self.root)?.enumerate() {
            ensure!(index < 16, "inbox directory exceeds its entry bound");
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_str().context("inbox filename is invalid")?;
            ensure!(
                matches!(name, "inbox.lock" | "worker.lock" | "inbox.json")
                    || name
                        .strip_prefix(".pending-")
                        .and_then(|s| s.strip_suffix(".json"))
                        .is_some_and(|s| uuid::Uuid::parse_str(s).is_ok()),
                "unexpected inbox entry; no files were removed"
            );
            private_metadata(&entry.path(), false)?;
        }
        Ok(())
    }

    fn read(&self, now: u64) -> Result<State> {
        self.check_directory()?;
        let file = open_regular(&self.root.join("inbox.json"), false, MAX_STATE_BYTES)?;
        let mut bytes = Vec::new();
        file.take(MAX_STATE_BYTES + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_STATE_BYTES,
            "inbox snapshot exceeds its bound"
        );
        let envelope: Envelope = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow!("inbox snapshot is invalid; trusted recovery required"))?;
        ensure!(
            digest(&serde_json::to_vec(&envelope.state)?) == envelope.checksum,
            "inbox checksum mismatch; trusted recovery required"
        );
        // A lock-free reader may open a snapshot committed after its start time.
        self.validate(&envelope.state, now.max(now_ms()?))?;
        Ok(envelope.state)
    }

    fn validate(&self, state: &State, now: u64) -> Result<()> {
        ensure!(
            (state.schema_version == 1 && state.archived.is_empty() || state.schema_version == 2)
                && state.archived.len() <= archive::MAX_REPLAY_TOMBSTONES
                && state.root_digest == self.root_digest
                && state.binding == self.binding
                && state.generation > 0
                && state.updated_at_ms > 0
                && state.updated_at_ms <= now
                && state.entries.len() <= MAX_EVENTS,
            "inbox binding, clock or capacity invalid"
        );
        let mut seen = BTreeSet::new();
        for entry in &state.archived {
            entry.validate(state.updated_at_ms)?;
            ensure!(
                seen.insert(&entry.body_digest),
                "duplicate archived replay digest"
            );
        }
        for entry in &state.entries {
            ensure!(
                seen.insert(&entry.body_digest)
                    && entry.received_at_ms > 0
                    && entry.received_at_ms <= entry.updated_at_ms
                    && entry.updated_at_ms <= state.updated_at_ms
                    && entry.attempts <= MAX_ATTEMPTS,
                "inbox entry identity or clock invalid"
            );
            let valid = match entry.state {
                DeliveryState::Queued => entry.attempts == 0 && entry.retry_at_ms == 0,
                DeliveryState::Publishing | DeliveryState::Completed => entry.attempts > 0,
                DeliveryState::Retry => entry.attempts > 0 && entry.attempts < MAX_ATTEMPTS,
                DeliveryState::Exhausted => entry.attempts == MAX_ATTEMPTS,
            };
            ensure!(
                valid && (entry.attempts == 0 || entry.retry_at_ms >= entry.updated_at_ms),
                "inbox attempt state invalid"
            );
            entry.header_map()?;
            entry.body()?;
        }
        Ok(())
    }

    fn write(&self, state: &State, initial: bool) -> Result<()> {
        self.validate(state, state.updated_at_ms)?;
        let bytes = serde_json::to_vec(&Envelope {
            checksum: digest(&serde_json::to_vec(state)?),
            state: state.clone(),
        })?;
        ensure!(
            bytes.len() as u64 <= MAX_STATE_BYTES,
            "inbox snapshot capacity reached"
        );
        self.check_directory()?;
        let destination = self.root.join("inbox.json");
        if !initial {
            private_metadata(&destination, false)?;
        }
        let pending = self
            .root
            .join(format!(".pending-{}.json", uuid::Uuid::new_v4()));
        let result = (|| -> Result<()> {
            let mut file = create_file(&pending)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            fs::rename(&pending, &destination)?;
            #[cfg(unix)]
            File::open(&self.root)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&pending);
        }
        // A directory-sync error may follow rename: never report rollback or success.
        result.context("inbox persistence unconfirmed; inspect before retry")
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}
fn backoff_ms(attempts: u32) -> u64 {
    5_000 * (1u64 << attempts.min(MAX_ATTEMPTS))
}
fn now_ms() -> Result<u64> {
    Ok(u64::try_from(
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis(),
    )?)
}

pub(super) fn validate_root(root: &Path, allow_missing: bool) -> Result<()> {
    ensure!(
        root.is_absolute()
            && !root
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir)),
        "inbox requires an explicit normalized absolute directory"
    );
    for path in root.ancestors() {
        match fs::symlink_metadata(path) {
            Ok(metadata) => ensure!(
                metadata.is_dir() && !is_alias(&metadata),
                "inbox path must not contain aliases"
            ),
            Err(error)
                if path == root
                    && allow_missing
                    && error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("inbox path unavailable"),
        }
    }
    Ok(())
}

fn is_alias(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

pub(super) fn private_metadata(path: &Path, directory: bool) -> Result<Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        !is_alias(&metadata)
            && if directory {
                metadata.is_dir()
            } else {
                metadata.is_file()
            },
        "inbox requires private regular paths"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            metadata.mode() & 0o077 == 0
                && metadata.uid() == unsafe { libc::geteuid() }
                && (directory || metadata.nlink() == 1),
            "inbox permissions, owner or hard links invalid"
        );
    }
    Ok(metadata)
}

fn options() -> OpenOptions {
    let mut options = OpenOptions::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.custom_flags(0x00200000);
    }
    options
}
pub(super) fn create_file(path: &Path) -> Result<File> {
    Ok(options()
        .read(true)
        .write(true)
        .create_new(true)
        .open(path)?)
}
pub(super) fn open_regular(path: &Path, write: bool, limit: u64) -> Result<File> {
    let before = private_metadata(path, false)?;
    ensure!(before.len() <= limit, "inbox file exceeds bound");
    let file = options().read(true).write(write).open(path)?;
    let after = file.metadata()?;
    ensure!(
        after.is_file() && after.len() <= limit && !is_alias(&after),
        "inbox file changed while opening"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            before.dev() == after.dev()
                && before.ino() == after.ino()
                && after.nlink() == 1
                && after.mode() & 0o077 == 0
                && after.uid() == unsafe { libc::geteuid() },
            "inbox file identity changed while opening"
        );
    }
    Ok(file)
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/git/inbox.rs"]
mod tests;
