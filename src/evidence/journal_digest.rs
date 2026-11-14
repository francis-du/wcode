use super::*;

/// Digest a record with its own `record_hash` cleared, so the stored digest
/// covers the observed content plus the previous link and nothing self-referential.
pub(super) fn record_digest(milestone: &EngineeringMilestone) -> String {
    let mut cleared = milestone.clone();
    cleared.record_hash = None;
    let Ok(bytes) = serde_json::to_vec(&cleared) else {
        return String::new();
    };
    digest_bytes(&bytes)
}

#[derive(Clone, Debug)]
pub(crate) struct EngineeringJournalHistory {
    pub records: Vec<EngineeringMilestone>,
    pub retained_records: usize,
    pub truncated: bool,
    /// Whether the retained window's chain links and content digests verify.
    /// Records written before the chain existed are skipped, not failed.
    pub chain_valid: bool,
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
    // Link the new record to the newest retained record before taking the lock.
    // The chain is best-effort: a missing predecessor leaves `chain_previous`
    // empty rather than blocking the write.
    let previous_hash = latest_record_hash(workspace)?;
    let mut linked = milestone.clone();
    linked.chain_previous = previous_hash;
    linked.actor = Some(
        linked
            .actor
            .clone()
            .filter(|actor| bounded_token(actor, 80))
            .unwrap_or_else(|| "unknown".to_owned()),
    );
    linked.record_hash = Some(record_digest(&linked));
    let milestone = &linked;
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
            chain_valid: true,
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
    let chain_valid = verify_chain(&records).is_ok();
    Ok(EngineeringJournalHistory {
        records,
        retained_records,
        truncated,
        chain_valid,
    })
}
