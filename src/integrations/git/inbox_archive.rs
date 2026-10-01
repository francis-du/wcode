//! Explicit private terminal-delivery archival. Replay tombstones stay online;
//! an archive never becomes a publication receipt or a restorable green Check.
use super::*;

pub(super) const MAX_REPLAY_TOMBSTONES: usize = 16_384;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReplayTombstone {
    pub(super) body_digest: String,
    archive_digest: String,
    state: DeliveryState,
    attempts: u32,
    received_at_ms: u64,
    updated_at_ms: u64,
}
impl ReplayTombstone {
    pub(super) fn validate(&self, now: u64) -> Result<()> {
        ensure!(
            valid_digest(&self.body_digest)
                && valid_digest(&self.archive_digest)
                && self.received_at_ms > 0
                && self.received_at_ms <= self.updated_at_ms
                && self.updated_at_ms <= now
                && self.attempts > 0
                && self.attempts <= MAX_ATTEMPTS
                && (self.state == DeliveryState::Completed
                    || (self.state == DeliveryState::Exhausted && self.attempts == MAX_ATTEMPTS)),
            "inbox archived replay identity or terminal state invalid"
        );
        Ok(())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DeliveryArchive {
    schema_version: u32,
    source_root_digest: String,
    binding: Binding,
    source_generation: u64,
    created_at_ms: u64,
    entries: Vec<Entry>,
}

/// Integrity metadata only. Keep independently; not an authenticated native
/// execution receipt, encryption key, freshness assertion or rollback oracle.
#[derive(Debug, Serialize)]
pub struct InboxArchiveCheckpoint {
    pub archive_digest: String,
    pub archive_bytes: u64,
    pub source_root_digest: String,
    pub source_generation: u64,
    pub compacted_generation: u64,
    pub archived_entries: usize,
    pub retained_replay_tombstones: usize,
    pub current_acceptance: bool,
}

impl GitHubInbox {
    /// Export terminal signed payloads to a create-only private file, confirm its
    /// exact bytes, then replace those payloads with compact online tombstones.
    /// A stale generation, active worker or full replay index never prunes data.
    pub fn archive_terminal(
        &self,
        expected_generation: u64,
        output: &Path,
    ) -> Result<InboxArchiveCheckpoint> {
        let _worker = self.lock_file("worker.lock")?;
        let _state = self.lock()?;
        let now = now_ms()?;
        let mut state = self.read(now)?;
        ensure!(
            expected_generation == state.generation,
            "inbox archive generation changed"
        );
        let entries = state
            .entries
            .iter()
            .filter(|entry| terminal(entry.state))
            .cloned()
            .collect::<Vec<_>>();
        ensure!(
            !entries.is_empty(),
            "inbox has no terminal deliveries to archive"
        );
        ensure!(
            state.archived.len().saturating_add(entries.len()) <= MAX_REPLAY_TOMBSTONES,
            "inbox replay index capacity reached; no tombstones were removed"
        );
        let archive = DeliveryArchive {
            schema_version: 1,
            source_root_digest: self.root_digest.clone(),
            binding: self.binding.clone(),
            source_generation: state.generation,
            created_at_ms: now,
            entries,
        };
        let bytes = serde_json::to_vec(&archive)?;
        ensure!(
            bytes.len() as u64 <= MAX_STATE_BYTES,
            "inbox archive exceeds its byte bound"
        );
        let archive_digest = digest(&bytes);
        let next_generation = state
            .generation
            .checked_add(1)
            .context("inbox generation exhausted")?;
        let source_generation = state.generation;
        for entry in &archive.entries {
            state.archived.push(ReplayTombstone {
                body_digest: entry.body_digest.clone(),
                archive_digest: archive_digest.clone(),
                state: entry.state,
                attempts: entry.attempts,
                received_at_ms: entry.received_at_ms,
                updated_at_ms: entry.updated_at_ms,
            });
        }
        state.entries.retain(|entry| !terminal(entry.state));
        // Version 1 bytes remain unchanged on ordinary legacy reads/writes. This
        // explicit transition prevents old binaries from ignoring replay indices.
        state.schema_version = 2;
        state.generation = next_generation;
        state.updated_at_ms = now;
        self.validate(&state, now)?;
        let envelope = Envelope {
            checksum: digest(&serde_json::to_vec(&state)?),
            state: state.clone(),
        };
        ensure!(
            serde_json::to_vec(&envelope)?.len() as u64 <= MAX_STATE_BYTES,
            "inbox compacted snapshot exceeds capacity; archive not started"
        );
        validate_output(output, &self.root)?;
        let persist = (|| -> Result<()> {
            let mut file = create_file(output)?;
            file.write_all(&bytes)?;
            file.sync_all()?;
            drop(file);
            #[cfg(unix)]
            File::open(output.parent().context("archive parent missing")?)?.sync_all()?;
            // Do not discard any original payload until the exact archive is
            // readable through the private no-follow reader and digest matches.
            let (_, observed) = self.read_archive(output, &archive_digest)?;
            ensure!(observed == bytes.len() as u64, "archive size changed");
            Ok(())
        })();
        persist.map_err(|_| anyhow!("inbox archive persistence unconfirmed; source was not compacted; inspect the create-only destination"))?;
        self.write(&state, false).map_err(|_| anyhow!(
            "inbox archive is durable but compaction acknowledgement is unconfirmed; inspect generation before retry"
        ))?;
        Ok(InboxArchiveCheckpoint {
            archive_digest,
            archive_bytes: bytes.len() as u64,
            source_root_digest: self.root_digest.clone(),
            source_generation,
            compacted_generation: state.generation,
            archived_entries: archive.entries.len(),
            retained_replay_tombstones: state.archived.len(),
            current_acceptance: false,
        })
    }

    /// Inspect a confidential archive against an independently supplied digest
    /// without importing its entries, changing queue state or issuing a receipt.
    pub fn inspect_archive(&self, path: &Path, expected_digest: &str) -> Result<Value> {
        let (archive, bytes) = self.read_archive(path, expected_digest)?;
        Ok(
            json!({"authority":"delivery_archive_observation", "current_acceptance":false,
            "archive_digest":expected_digest, "archive_bytes":bytes,
            "source_generation":archive.source_generation, "archived_entries":archive.entries.len(),
            "source_root_digest":archive.source_root_digest, "created_at_ms":archive.created_at_ms,
            "restored":false, "freshness_proven":false}),
        )
    }

    fn read_archive(&self, path: &Path, expected_digest: &str) -> Result<(DeliveryArchive, u64)> {
        ensure!(
            valid_digest(expected_digest),
            "independent archive digest is invalid"
        );
        let parent = path.parent().context("archive parent missing")?;
        validate_root(parent, false)?;
        private_metadata(parent, true)?;
        let file = open_regular(path, false, MAX_STATE_BYTES)?;
        let mut bytes = Vec::new();
        file.take(MAX_STATE_BYTES + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= MAX_STATE_BYTES && digest(&bytes) == expected_digest,
            "inbox archive digest or length mismatch"
        );
        let archive: DeliveryArchive = serde_json::from_slice(&bytes)
            .map_err(|_| anyhow!("inbox archive metadata invalid"))?;
        ensure!(
            archive.schema_version == 1
                && archive.source_root_digest == self.root_digest
                && archive.binding == self.binding
                && !archive.entries.is_empty()
                && archive.entries.len() <= MAX_EVENTS
                && archive.created_at_ms <= now_ms()?
                && archive.entries.iter().all(|entry| terminal(entry.state)),
            "inbox archive identity or terminal records invalid"
        );
        self.validate(
            &State {
                schema_version: 1,
                root_digest: archive.source_root_digest.clone(),
                binding: archive.binding.clone(),
                generation: archive.source_generation,
                updated_at_ms: archive.created_at_ms,
                entries: archive.entries.clone(),
                archived: Vec::new(),
            },
            now_ms()?,
        )?;
        Ok((archive, bytes.len() as u64))
    }
}

fn terminal(state: DeliveryState) -> bool {
    matches!(state, DeliveryState::Completed | DeliveryState::Exhausted)
}
fn valid_digest(value: &str) -> bool {
    value
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()))
}
fn validate_output(output: &Path, root: &Path) -> Result<()> {
    ensure!(
        output.is_absolute()
            && !output
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
            && !output.starts_with(root),
        "archive requires a normalized absolute path outside the live inbox"
    );
    let parent = output.parent().context("archive parent missing")?;
    validate_root(parent, false)?;
    private_metadata(parent, true)?;
    match fs::symlink_metadata(output) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => bail!("archive destination already exists or is unavailable; nothing overwritten"),
    }
}

#[cfg(test)]
#[path = "../../../tests/unit/integrations/git/inbox_archive.rs"]
mod tests;
