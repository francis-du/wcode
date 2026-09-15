use super::*;
use std::sync::{Arc, Weak};

static ACTIVATION_FLIGHTS: OnceLock<Mutex<BTreeMap<PathBuf, Weak<Mutex<()>>>>> = OnceLock::new();

pub(super) fn activation_flight(root: &Path) -> Arc<Mutex<()>> {
    let flights = ACTIVATION_FLIGHTS.get_or_init(|| Mutex::new(BTreeMap::new()));
    let mut flights = flights
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(flight) = flights.get(root).and_then(Weak::upgrade) {
        return flight;
    }
    flights.retain(|_, flight| flight.strong_count() > 0);
    let flight = Arc::new(Mutex::new(()));
    flights.insert(root.to_path_buf(), Arc::downgrade(&flight));
    flight
}

pub(super) struct EvaluationPaths {
    pub(super) unique_paths: usize,
    pub(super) live_paths: usize,
    pub(super) stale_path_references: usize,
    pub(super) prepared: Vec<Vec<String>>,
    pub(super) prepared_context: Vec<Vec<String>>,
}

pub(super) fn prepare_evaluation_paths(
    workspace: &Workspace,
    records: &[VerifiedChangeExperience],
) -> Result<EvaluationPaths> {
    let unique = records
        .iter()
        .flat_map(|record| record.paths.iter().chain(record.context_paths.iter()))
        .cloned()
        .collect::<BTreeSet<_>>();
    let paths = unique.iter().cloned().collect::<Vec<_>>();
    // This is membership, not proof of file contents. Reuse Workspace's guarded
    // path resolution without path_info's full-file SHA calculation.
    let live =
        crate::resource::parallel_io(&paths, |path| workspace.source_metadata_stamp(path).is_ok())?;
    let live_paths = paths
        .into_iter()
        .zip(live)
        .filter_map(|(path, live)| live.then_some(path))
        .collect::<BTreeSet<_>>();

    let mut stale_path_references = 0usize;
    let mut prepared = Vec::with_capacity(records.len());
    let mut prepared_context = Vec::with_capacity(records.len());
    for record in records {
        let record_paths = record
            .paths
            .iter()
            .filter_map(|path| {
                if live_paths.contains(path) {
                    Some(path.clone())
                } else {
                    stale_path_references = stale_path_references.saturating_add(1);
                    None
                }
            })
            .collect::<Vec<_>>();
        let context_paths = record
            .context_paths
            .iter()
            .filter_map(|path| {
                if live_paths.contains(path) {
                    Some(path.clone())
                } else {
                    stale_path_references = stale_path_references.saturating_add(1);
                    None
                }
            })
            .collect::<Vec<_>>();
        prepared.push(record_paths);
        prepared_context.push(context_paths);
    }

    Ok(EvaluationPaths {
        unique_paths: unique.len(),
        live_paths: live_paths.len(),
        stale_path_references,
        prepared,
        prepared_context,
    })
}

pub(super) fn activation_fingerprint(
    records: &[VerifiedChangeExperience],
    paths: &EvaluationPaths,
) -> Result<[u8; 32]> {
    let mut hasher = Sha256::new();
    hasher.update(b"experience-activation-input-v2");
    hasher.update(EXPERIENCE_EVALUATION_METHOD.as_bytes());
    hasher.update(EXPERIENCE_ACTIVATION_POLICY.as_bytes());
    hasher.update((records.len() as u64).to_le_bytes());
    // Frame each record separately instead of allocating one history-sized JSON.
    // Include trajectory/intent and ordered live paths, not just record count or
    // last revision; equal-size replacements must invalidate cached decisions.
    for (index, record) in records.iter().enumerate() {
        let bytes = serde_json::to_vec(&(
            record,
            &paths.prepared[index],
            &paths.prepared_context[index],
        ))?;
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    Ok(hasher.finalize().into())
}

pub(super) fn read_record(path: &Path) -> Result<Option<VerifiedChangeExperience>> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_EXPERIENCE_BYTES
    {
        return Ok(None);
    }
    let bytes = fs::read(path)?;
    let record = match serde_json::from_slice::<VerifiedChangeExperience>(&bytes) {
        Ok(record) => record,
        Err(_) => return Ok(None),
    };
    Ok(valid_record(&record).then_some(record))
}
