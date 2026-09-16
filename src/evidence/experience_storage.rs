use super::*;
use crate::evidence_store::workspace_state_directory;
use anyhow::{bail, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_EXPERIENCE_CANDIDATES: usize = MAX_STORED_EXPERIENCES * 8;

fn experience_identity_bytes(
    schema_version: u8,
    revision: &Revision,
    level: &str,
    paths: &[String],
) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&(
        schema_version,
        revision,
        level,
        paths,
    ))?)
}

pub(super) fn canonical_experience_name(record: &VerifiedChangeExperience) -> Result<String> {
    let stable = experience_identity_bytes(
        record.schema_version,
        &record.revision,
        &record.level,
        &record.paths,
    )?;
    let digest = digest_bytes(&stable);
    Ok(format!("{}.json", &digest[..32]))
}

pub(super) fn experience_payload_digest(record: &VerifiedChangeExperience) -> Result<String> {
    let payload = serde_json::to_vec(&(
        record.schema_version,
        &record.revision,
        &record.level,
        &record.paths,
        &record.context_paths,
        &record.retrieval_intent,
        record.timestamp_ms,
    ))?;
    Ok(digest_bytes(&payload))
}

fn record_integrity_valid(record: &VerifiedChangeExperience) -> bool {
    match record.schema_version {
        1 | 2 => record.payload_digest.is_none(),
        EXPERIENCE_SCHEMA_VERSION => record.payload_digest.as_ref().is_some_and(|expected| {
            expected.len() == 64
                && expected.bytes().all(|byte| byte.is_ascii_hexdigit())
                && experience_payload_digest(record).is_ok_and(|actual| actual == *expected)
        }),
        _ => false,
    }
}

fn canonical_experience_filename(name: &str) -> bool {
    name.strip_suffix(".json")
        .is_some_and(|stem| stem.len() == 32 && stem.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

pub(crate) fn persist_verified_change(
    workspace: &Workspace,
    revision: &Revision,
    level: &str,
    paths: &[String],
) -> Result<bool> {
    if !matches!(level, "quick" | "full") {
        bail!("verified experience requires quick or full verification");
    }
    let paths = normalize_paths(paths);
    if paths.len() < 2 || paths.len() > MAX_EXPERIENCE_PATHS {
        return Ok(false);
    }
    if revision.code.trim().is_empty() || revision.code.ends_with(":partial") {
        return Ok(false);
    }

    let directory = experience_directory(workspace)?;
    ensure_regular_directory(&directory)?;
    let stable = experience_identity_bytes(EXPERIENCE_SCHEMA_VERSION, revision, level, &paths)?;
    let digest = digest_bytes(&stable);
    let path = directory.join(format!("{}.json", &digest[..32]));
    match fs::symlink_metadata(&path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                bail!("verified experience record path is not a regular file");
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                if metadata.nlink() > 1 {
                    bail!("verified experience record path is hard-linked");
                }
            }
            return Ok(false);
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }

    let (context_paths, retrieval_intent) = recent_context_trajectory(workspace, &paths);
    let mut record = VerifiedChangeExperience {
        schema_version: EXPERIENCE_SCHEMA_VERSION,
        revision: revision.clone(),
        level: level.to_owned(),
        paths,
        context_paths,
        retrieval_intent,
        payload_digest: None,
        timestamp_ms: now_ms(),
    };
    record.payload_digest = Some(experience_payload_digest(&record)?);
    let bytes = serde_json::to_vec(&record)?;
    if bytes.len() as u64 > MAX_EXPERIENCE_BYTES {
        bail!("verified experience record exceeds the persistent store size bound");
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
        .with_context(|| format!("cannot create verified experience {}", path.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("cannot write verified experience {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("cannot sync verified experience {}", path.display()))?;
    prune_directory(&directory)?;
    Ok(true)
}

pub(super) fn load(workspace: &Workspace) -> Result<Vec<VerifiedChangeExperience>> {
    let directory = experience_directory(workspace)?;
    let metadata = match fs::symlink_metadata(&directory) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("verified experience store path is not a regular directory");
    }
    let paths = experience_paths(&directory)?;
    let loaded = crate::resource::parallel_io(&paths, |path| experience_io::read_record(path))?;
    let mut identities = BTreeMap::<Vec<u8>, (Vec<u8>, VerifiedChangeExperience, bool)>::new();
    for record in loaded {
        let Some(record) = record? else {
            continue;
        };
        let identity = experience_identity_bytes(
            record.schema_version,
            &record.revision,
            &record.level,
            &record.paths,
        )?;
        let encoded = serde_json::to_vec(&record)?;
        match identities.entry(identity) {
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert((encoded, record, false));
            }
            std::collections::btree_map::Entry::Occupied(mut entry) => {
                let (existing, _, conflicted) = entry.get_mut();
                if *existing != encoded {
                    *conflicted = true;
                }
            }
        }
    }
    let mut records = identities
        .into_values()
        .filter_map(|(_, record, conflicted)| (!conflicted).then_some(record))
        .collect::<Vec<_>>();
    records.sort_by(|left, right| {
        left.timestamp_ms
            .cmp(&right.timestamp_ms)
            .then_with(|| left.revision.code.cmp(&right.revision.code))
            .then_with(|| left.level.cmp(&right.level))
            .then_with(|| left.paths.cmp(&right.paths))
    });
    let excess = records.len().saturating_sub(MAX_STORED_EXPERIENCES);
    if excess > 0 {
        records.drain(..excess);
    }
    Ok(records)
}

pub(super) fn valid_record(record: &VerifiedChangeExperience) -> bool {
    matches!(record.schema_version, 1 | 2 | EXPERIENCE_SCHEMA_VERSION)
        && record_integrity_valid(record)
        && matches!(record.level.as_str(), "quick" | "full")
        && !record.revision.code.trim().is_empty()
        && !record.revision.code.ends_with(":partial")
        && (2..=MAX_EXPERIENCE_PATHS).contains(&record.paths.len())
        && record
            .paths
            .iter()
            .all(|path| normalize_path(path).is_some())
        && normalize_paths(&record.paths) == record.paths
        && record.context_paths.len() <= MAX_CONTEXT_PATHS
        && record
            .context_paths
            .iter()
            .all(|path| normalize_path(path).is_some())
        && normalize_paths(&record.context_paths) == record.context_paths
        && record.retrieval_intent.as_ref().is_none_or(|intent| {
            matches!(
                intent.as_str(),
                "balanced_context"
                    | "trace_to_code"
                    | "code_to_test"
                    | "comment_to_context"
                    | "failure_trace_to_code"
                    | "edit_to_ripple"
            )
        })
}

pub(super) fn normalize_paths(paths: &[String]) -> Vec<String> {
    paths
        .iter()
        .filter_map(|path| normalize_path(path))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn normalize_path(raw: &str) -> Option<String> {
    let replaced = raw.trim().replace('\\', "/");
    if replaced.is_empty() || replaced.len() > MAX_PATH_LENGTH {
        return None;
    }
    let path = Path::new(&replaced);
    if path.is_absolute() {
        return None;
    }
    let mut parts = Vec::new();
    for component in path.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?.to_owned()),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

pub(super) fn experience_directory(workspace: &Workspace) -> Result<PathBuf> {
    Ok(workspace_state_directory(workspace)?.join("experience"))
}

pub(super) fn ensure_regular_directory(directory: &Path) -> Result<()> {
    match fs::symlink_metadata(directory) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_dir() {
                bail!("verified experience store path is not a regular directory");
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(directory)?;
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn experience_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        if entry
            .file_name()
            .to_str()
            .is_some_and(canonical_experience_filename)
        {
            paths.push(entry.path());
            if paths.len() > MAX_EXPERIENCE_CANDIDATES {
                bail!("verified experience store exceeds its bounded candidate scan");
            }
        }
    }
    paths.sort();
    Ok(paths)
}

fn prune_directory(directory: &Path) -> Result<()> {
    let paths = experience_paths(directory)?;
    let loaded = crate::resource::parallel_io(&paths, |path| experience_io::read_record(path))?;
    let mut valid = paths
        .into_iter()
        .zip(loaded)
        .filter_map(|(path, record)| record.ok().flatten().map(|record| (path, record)))
        .collect::<Vec<_>>();
    valid.sort_by(|(left_path, left), (right_path, right)| {
        left.timestamp_ms
            .cmp(&right.timestamp_ms)
            .then_with(|| left_path.cmp(right_path))
    });
    let excess = valid.len().saturating_sub(MAX_STORED_EXPERIENCES);
    for (path, _) in valid.into_iter().take(excess) {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

fn digest_bytes(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
