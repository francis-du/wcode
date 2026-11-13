use super::*;
use anyhow::{bail, Context};
use std::fs::{self, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub(super) fn verification_plan_satisfies_floor(
    plan: &crate::verification::VerificationPlan,
    floor: Option<&str>,
) -> bool {
    match floor {
        None => true,
        Some("full") => plan.deterministic_level == "full",
        Some("adversarial") => {
            plan.deterministic_level == "full"
                && plan
                    .reviewer_roles
                    .contains(&crate::verification::ReviewerRole::Adversarial)
        }
        Some(_) => false,
    }
}

pub(super) fn sync(
    workspace: &Workspace,
    worklist: &crate::worklist::Worklist,
    current: Option<Execution>,
    restart: bool,
    signals: ExecutionSignals,
) -> Result<Value> {
    let mut checkpoint = checkpoint(worklist, &signals);
    let new_generation = restart
        || current.is_none()
        || current.as_ref().is_some_and(|execution| {
            execution.pending_directive.is_none()
                && execution.checkpoint.open_items == 0
                && !crate::worklist::is_complete(worklist)
                && worklist.revision > execution.checkpoint.worklist_revision
        });
    let mut pending_directive = if new_generation {
        None
    } else {
        current
            .as_ref()
            .and_then(|execution| execution.pending_directive.clone())
    };
    if pending_directive.as_ref().is_some_and(|directive| {
        let worklist_advanced = worklist.revision > directive.worklist_revision;
        let replan_satisfied = !directive.requires_replan
            || (checkpoint.reconciliation_plan_id.is_some()
                && checkpoint.reconciliation_plan_id != directive.reconciliation_plan_id);
        worklist_advanced && replan_satisfied
    }) {
        pending_directive = None;
    }
    if let Some(directive) = pending_directive.as_ref() {
        append_execution_blocker(
            &mut checkpoint,
            if directive.requires_replan {
                "steering_replan_required"
            } else {
                "steering_pending"
            },
        );
    }
    let phase = phase(worklist, &checkpoint, pending_directive.is_some());

    if let Some(existing) = current.as_ref() {
        if !new_generation
            && existing.objective == worklist.goal
            && existing.phase == phase
            && existing.checkpoint == checkpoint
            && existing.pending_directive == pending_directive
        {
            return Ok(status_value(existing));
        }
    }

    let proposal = current.as_ref().and_then(|execution| {
        execution.proposal.clone().filter(|proposal| {
            proposal.worklist_revision == checkpoint.worklist_revision
                && proposal.repository_revision == checkpoint.repository_revision
        })
    });
    let now = now_ms();
    let revision = current
        .as_ref()
        .map_or(1, |execution| execution.revision.saturating_add(1));
    let execution = Execution {
        schema_version: EXECUTION_SCHEMA_VERSION,
        execution_id: if new_generation {
            format!("EX-{}", Uuid::new_v4().simple())
        } else {
            current
                .as_ref()
                .map(|execution| execution.execution_id.clone())
                .unwrap_or_else(|| format!("EX-{}", Uuid::new_v4().simple()))
        },
        revision,
        objective: worklist.goal.clone(),
        phase,
        created_at_ms: if new_generation {
            now
        } else {
            current
                .as_ref()
                .map_or(now, |execution| execution.created_at_ms)
        },
        updated_at_ms: now,
        checkpoint,
        proposal: if new_generation || pending_directive.is_some() {
            None
        } else {
            proposal
        },
        pending_directive,
        lineage: if new_generation {
            None
        } else {
            current
                .as_ref()
                .and_then(|execution| execution.lineage.clone())
        },
        verification_floor: if new_generation {
            None
        } else {
            current
                .as_ref()
                .and_then(|execution| execution.verification_floor.clone())
        },
    };
    validate(&execution)?;
    persist(workspace, &execution)?;
    Ok(status_value(&execution))
}

pub(super) fn checkpoint(
    worklist: &crate::worklist::Worklist,
    signals: &ExecutionSignals,
) -> ExecutionCheckpoint {
    let done_items = worklist
        .items
        .iter()
        .filter(|item| item.status == crate::worklist::WorkItemStatus::Done)
        .count();
    let blocked_ids = worklist
        .items
        .iter()
        .filter(|item| item.status == crate::worklist::WorkItemStatus::Blocked)
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    let mut blockers = blocked_ids
        .iter()
        .map(|id| format!("worklist:{id}"))
        .collect::<Vec<_>>();
    blockers.extend(signals.verification_blockers.iter().cloned());
    blockers.extend(signals.reconciliation_blockers.iter().cloned());
    if crate::worklist::is_complete(worklist)
        && signals.verification_plan_id.is_none()
        && signals.verification_ready != Some(true)
    {
        blockers.push("verification_pending".to_owned());
    }
    blockers.sort();
    blockers.dedup();
    blockers.truncate(MAX_EXECUTION_BLOCKERS);

    ExecutionCheckpoint {
        worklist_revision: worklist.revision,
        worklist_updated_at_ms: worklist.updated_at_ms,
        open_items: worklist.items.len().saturating_sub(done_items),
        done_items,
        blocked_items: blocked_ids.len(),
        runnable: crate::worklist::runnable_ids(worklist),
        repository_revision: Some(signals.repository_revision.clone()),
        reconciliation_plan_id: signals.reconciliation_plan_id.clone(),
        verification_plan_id: signals.verification_plan_id.clone(),
        verification_ready: signals.verification_ready,
        reconciliation_converged: signals.reconciliation_converged,
        blockers,
    }
}

pub(super) fn select_reconciliation_plan_id(
    current: Option<&Execution>,
    restart: bool,
    allow_steering_replan: bool,
    current_revision_plan_id: Option<String>,
) -> Option<String> {
    if restart {
        return current_revision_plan_id;
    }
    let bound = current.and_then(|execution| execution.checkpoint.reconciliation_plan_id.clone());
    if allow_steering_replan {
        if let Some(candidate) = current_revision_plan_id.as_ref() {
            if bound.as_ref() != Some(candidate) {
                return Some(candidate.clone());
            }
        }
    }
    bound.or(current_revision_plan_id)
}

pub(super) fn phase(
    worklist: &crate::worklist::Worklist,
    checkpoint: &ExecutionCheckpoint,
    steering_pending: bool,
) -> ExecutionPhase {
    if steering_pending {
        return ExecutionPhase::Blocked;
    }
    if crate::worklist::is_complete(worklist) {
        let reconciliation_ready = checkpoint.reconciliation_plan_id.is_none()
            || checkpoint.reconciliation_converged == Some(true);
        if reconciliation_ready && checkpoint.verification_ready == Some(true) {
            ExecutionPhase::Completed
        } else {
            ExecutionPhase::Verifying
        }
    } else if checkpoint.open_items > 0
        && checkpoint.runnable.is_empty()
        && checkpoint.blocked_items > 0
    {
        ExecutionPhase::Blocked
    } else {
        ExecutionPhase::Executing
    }
}

pub(crate) fn steer(workspace: &Workspace, input: ExecutionDirectiveInput) -> Result<Value> {
    let mut execution =
        load(workspace)?.ok_or_else(|| anyhow::anyhow!("active execution does not exist"))?;
    ensure_active_revision(&execution, input.expected_revision)?;
    if execution.pending_directive.is_some() {
        bail!("execution already has pending steering; apply it through Worklist/replan state before recording another directive");
    }
    let mut directive = build_directive(&execution, input)?;
    directive.reconciliation_plan_id = execution.checkpoint.reconciliation_plan_id.clone();
    directive.normalize_replan_requirement();
    if directive.kind == ExecutionDirectiveKind::StrengthenVerification {
        execution.verification_floor = directive.verification_strength.clone();
    }
    execution.revision = execution.revision.saturating_add(1);
    execution.updated_at_ms = now_ms();
    execution.phase = ExecutionPhase::Blocked;
    append_execution_blocker(
        &mut execution.checkpoint,
        if directive.requires_replan {
            "steering_replan_required"
        } else {
            "steering_pending"
        },
    );
    execution.proposal = None;
    execution.pending_directive = Some(directive);
    validate(&execution)?;
    persist(workspace, &execution)?;
    Ok(status_value(&execution))
}

pub(crate) fn handoff(workspace: &Workspace, input: ExecutionHandoffInput) -> Result<Value> {
    let mut execution =
        load(workspace)?.ok_or_else(|| anyhow::anyhow!("active execution does not exist"))?;
    ensure_active_revision(&execution, input.expected_revision)?;
    let requested_by = input.requested_by.trim();
    let summary = input.summary.trim();
    if requested_by.is_empty()
        || requested_by.len() > 256
        || summary.is_empty()
        || summary.chars().count() > MAX_STEERING_SUMMARY_CHARS
    {
        bail!("execution handoff requester or summary is invalid");
    }
    let parent_execution_id = execution.execution_id.clone();
    let handoff_count = execution
        .lineage
        .as_ref()
        .map_or(1, |lineage| lineage.handoff_count.saturating_add(1));
    execution.execution_id = format!("EX-{}", Uuid::new_v4().simple());
    execution.revision = execution.revision.saturating_add(1);
    execution.updated_at_ms = now_ms();
    execution.created_at_ms = execution.updated_at_ms;
    execution.proposal = None;
    execution.lineage = Some(ExecutionLineage {
        parent_execution_id,
        handoff_count,
        requested_by: requested_by.to_owned(),
        summary: summary.to_owned(),
        handed_off_at_ms: execution.updated_at_ms,
    });
    validate(&execution)?;
    persist(workspace, &execution)?;
    Ok(status_value(&execution))
}

pub(super) fn ensure_active_revision(execution: &Execution, expected_revision: u64) -> Result<()> {
    if execution.phase == ExecutionPhase::Completed {
        bail!("execution is already settled as completed");
    }
    if expected_revision != execution.revision {
        bail!(
            "execution revision changed: expected {expected_revision}, current {}; reread execution_status and retry",
            execution.revision
        );
    }
    Ok(())
}

pub(super) fn build_directive(
    execution: &Execution,
    input: ExecutionDirectiveInput,
) -> Result<ExecutionDirective> {
    let summary = input.summary.trim();
    let requested_by = input.requested_by.trim();
    if summary.is_empty()
        || summary.chars().count() > MAX_STEERING_SUMMARY_CHARS
        || requested_by.is_empty()
        || requested_by.len() > 256
        || input.scopes.len() > MAX_STEERING_SCOPES
        || input
            .scopes
            .iter()
            .any(|scope| scope.trim().is_empty() || scope.len() > 300)
    {
        bail!("execution steering directive is invalid");
    }
    let objective = input.objective.map(|value| value.trim().to_owned());
    let verification_strength = input
        .verification_strength
        .map(|value| value.trim().to_ascii_lowercase());
    match input.kind {
        ExecutionDirectiveKind::RefineObjective => {
            if objective.as_deref().is_none_or(str::is_empty)
                || objective
                    .as_ref()
                    .is_some_and(|value| value.chars().count() > 1_000)
                || !input.scopes.is_empty()
                || verification_strength.is_some()
            {
                bail!("refine_objective requires objective only");
            }
        }
        ExecutionDirectiveKind::ChangeScope => {
            if input.scopes.is_empty() || objective.is_some() || verification_strength.is_some() {
                bail!("change_scope requires one or more scopes only");
            }
        }
        ExecutionDirectiveKind::StrengthenVerification => {
            if objective.is_some() || !input.scopes.is_empty() {
                bail!("strengthen_verification accepts verification_strength only");
            }
            if !matches!(
                verification_strength.as_deref(),
                Some("full" | "adversarial")
            ) {
                bail!("verification_strength must be full or adversarial");
            }
            if verification_strength.as_deref() == Some("full")
                && execution.verification_floor.as_deref() == Some("adversarial")
            {
                bail!("verification steering cannot lower strength from adversarial to full");
            }
        }
    }
    let scopes = input
        .scopes
        .into_iter()
        .map(|scope| scope.trim().to_owned())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    Ok(ExecutionDirective {
        kind: input.kind,
        summary: summary.to_owned(),
        requested_by: requested_by.to_owned(),
        objective,
        scopes,
        verification_strength,
        worklist_revision: execution.checkpoint.worklist_revision,
        repository_revision: execution.checkpoint.repository_revision.clone(),
        reconciliation_plan_id: execution.checkpoint.reconciliation_plan_id.clone(),
        requires_replan: false,
        requested_at_ms: now_ms(),
    })
}

pub(crate) fn propose(workspace: &Workspace, input: ExecutionProposalInput) -> Result<Value> {
    let mut execution =
        load(workspace)?.ok_or_else(|| anyhow::anyhow!("active execution does not exist"))?;
    ensure_active_revision(&execution, input.expected_revision)?;
    let summary = input.summary.trim();
    let proposer = input.proposer.trim();
    if summary.is_empty()
        || summary.chars().count() > MAX_PROPOSAL_SUMMARY_CHARS
        || proposer.is_empty()
        || proposer.len() > 256
    {
        bail!("execution proposal summary or proposer is invalid");
    }
    execution.revision = execution.revision.saturating_add(1);
    execution.updated_at_ms = now_ms();
    execution.proposal = Some(ExecutionProposal {
        status: input.status,
        summary: summary.to_owned(),
        proposer: proposer.to_owned(),
        worklist_revision: execution.checkpoint.worklist_revision,
        repository_revision: execution.checkpoint.repository_revision.clone(),
        proposed_at_ms: execution.updated_at_ms,
    });
    validate(&execution)?;
    persist(workspace, &execution)?;
    Ok(status_value(&execution))
}

pub(super) fn load(workspace: &Workspace) -> Result<Option<Execution>> {
    let directory = execution_directory(workspace)?;
    if !directory.exists() {
        return Ok(None);
    }
    ensure_regular_directory(&directory)?;
    let Some(path) = snapshot_paths(&directory)?.pop() else {
        return Ok(None);
    };
    // A published but unreadable latest revision is not an absent execution.
    // Falling back could resurrect a superseded completion or drop steering.
    let metadata = fs::symlink_metadata(&path).context("execution snapshot unavailable")?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() > MAX_EXECUTION_BYTES
    {
        bail!("execution snapshot is not a bounded regular file");
    }
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .context("execution snapshot unreadable")?
        .take(MAX_EXECUTION_BYTES + 1)
        .read_to_end(&mut bytes)
        .context("execution snapshot unreadable")?;
    if bytes.len() as u64 > MAX_EXECUTION_BYTES {
        bail!("execution snapshot exceeds its read bound");
    }
    let mut execution: Execution =
        serde_json::from_slice(&bytes).context("execution snapshot is corrupt")?;
    validate(&execution)?;
    if path.file_name().and_then(|name| name.to_str())
        != Some(format!("{:020}.json", execution.revision).as_str())
    {
        bail!("execution snapshot revision does not match its identity");
    }
    if let Some(directive) = execution.pending_directive.as_mut() {
        directive.normalize_replan_requirement();
    }
    Ok(Some(execution))
}

pub(super) fn persist(workspace: &Workspace, execution: &Execution) -> Result<()> {
    let _guard = update_lock()
        .lock()
        .map_err(|_| anyhow::anyhow!("execution update lock poisoned"))?;
    let directory = execution_directory(workspace)?;
    fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create execution store {}", directory.display()))?;
    ensure_regular_directory(&directory)?;
    let bytes = serde_json::to_vec(execution).context("cannot encode execution checkpoint")?;
    if bytes.len() as u64 > MAX_EXECUTION_BYTES {
        bail!("execution checkpoint exceeds persistent store size bound");
    }
    let target = directory.join(format!("{:020}.json", execution.revision));
    let temp = directory.join(format!(".execution-{}.tmp", Uuid::new_v4().simple()));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(&temp)
        .with_context(|| format!("cannot create execution temp {}", temp.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("cannot write execution temp {}", temp.display()))?;
    file.sync_all()
        .with_context(|| format!("cannot sync execution temp {}", temp.display()))?;
    match fs::hard_link(&temp, &target) {
        Ok(()) => {
            let _ = fs::remove_file(&temp);
        }
        Err(error) => {
            let _ = fs::remove_file(&temp);
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                bail!(
                    "execution revision changed concurrently; refresh execution_status and retry"
                );
            }
            return Err(error).context("cannot atomically publish execution checkpoint");
        }
    }
    prune(&directory)?;
    Ok(())
}

pub(super) fn validate(execution: &Execution) -> Result<()> {
    let checkpoint = &execution.checkpoint;
    if execution.schema_version != EXECUTION_SCHEMA_VERSION
        || execution.execution_id.trim().is_empty()
        || execution.execution_id.len() > 96
        || execution.revision == 0
        || execution.objective.trim().is_empty()
        || execution.objective.len() > 1_000
        || checkpoint.worklist_revision == 0
        || checkpoint.blocked_items > checkpoint.open_items
        || checkpoint.runnable.len() > 64
        || checkpoint.blockers.len() > MAX_EXECUTION_BLOCKERS
        || execution.proposal.as_ref().is_some_and(|proposal| {
            proposal.summary.trim().is_empty()
                || proposal.summary.chars().count() > MAX_PROPOSAL_SUMMARY_CHARS
                || proposal.proposer.trim().is_empty()
                || proposal.proposer.len() > 256
                || proposal.worklist_revision == 0
                || proposal.proposed_at_ms == 0
        })
        || execution
            .pending_directive
            .as_ref()
            .is_some_and(|directive| {
                directive.summary.trim().is_empty()
                    || directive.summary.chars().count() > MAX_STEERING_SUMMARY_CHARS
                    || directive.requested_by.trim().is_empty()
                    || directive.requested_by.len() > 256
                    || directive.scopes.len() > MAX_STEERING_SCOPES
                    || directive.worklist_revision == 0
                    || directive.requested_at_ms == 0
                    || directive
                        .reconciliation_plan_id
                        .as_deref()
                        .is_some_and(|id| id.trim().is_empty() || id.len() > 160)
                    || directive.objective.as_ref().is_some_and(|value| {
                        value.trim().is_empty() || value.chars().count() > 1_000
                    })
                    || directive
                        .scopes
                        .iter()
                        .any(|scope| scope.trim().is_empty() || scope.len() > 300)
                    || directive
                        .verification_strength
                        .as_deref()
                        .is_some_and(|value| !matches!(value, "full" | "adversarial"))
            })
        || execution
            .verification_floor
            .as_deref()
            .is_some_and(|value| !matches!(value, "full" | "adversarial"))
        || execution.lineage.as_ref().is_some_and(|lineage| {
            lineage.parent_execution_id.trim().is_empty()
                || lineage.parent_execution_id.len() > 96
                || lineage.handoff_count == 0
                || lineage.requested_by.trim().is_empty()
                || lineage.requested_by.len() > 256
                || lineage.summary.trim().is_empty()
                || lineage.summary.chars().count() > MAX_STEERING_SUMMARY_CHARS
                || lineage.handed_off_at_ms == 0
        })
    {
        bail!("invalid execution checkpoint");
    }
    for id in &checkpoint.runnable {
        if id.is_empty() || id.len() > 64 {
            bail!("execution checkpoint contains invalid runnable item");
        }
    }
    for blocker in &checkpoint.blockers {
        if blocker.trim().is_empty() || blocker.len() > 1_000 {
            bail!("execution checkpoint contains invalid blocker");
        }
    }
    for id in [
        checkpoint.reconciliation_plan_id.as_deref(),
        checkpoint.verification_plan_id.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if id.trim().is_empty() || id.len() > 160 {
            bail!("execution checkpoint contains invalid plan id");
        }
    }
    Ok(())
}

pub(super) fn append_execution_blocker(checkpoint: &mut ExecutionCheckpoint, blocker: &str) {
    if checkpoint.blockers.iter().any(|value| value == blocker) {
        return;
    }
    if checkpoint.blockers.len() >= MAX_EXECUTION_BLOCKERS {
        checkpoint.blockers.pop();
    }
    checkpoint.blockers.push(blocker.to_owned());
    checkpoint.blockers.sort();
    checkpoint.blockers.dedup();
}

pub(super) fn execution_directory(workspace: &Workspace) -> Result<PathBuf> {
    Ok(crate::evidence_store::workspace_state_directory(workspace)?.join("execution"))
}

pub(super) fn snapshot_paths(directory: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for (index, entry) in fs::read_dir(directory)
        .context("cannot list execution store")?
        .enumerate()
    {
        if index >= MAX_EXECUTION_SNAPSHOTS.saturating_mul(2) {
            bail!("execution store scan exceeds recovery bound");
        }
        let entry = entry.context("execution store entry unavailable")?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.ends_with(".json") {
            if name.len() != 25 || !name[..20].bytes().all(|byte| byte.is_ascii_digit()) {
                bail!("execution snapshot has invalid identity");
            }
            paths.push(entry.path());
        }
    }
    paths.sort();
    Ok(paths)
}

pub(super) fn prune(directory: &Path) -> Result<()> {
    let paths = snapshot_paths(directory)?;
    let excess = paths.len().saturating_sub(MAX_EXECUTION_SNAPSHOTS);
    for path in paths.into_iter().take(excess) {
        let _ = fs::remove_file(path);
    }
    Ok(())
}

pub(super) fn ensure_regular_directory(directory: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(directory)
        .with_context(|| format!("cannot inspect execution store {}", directory.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("execution store path is not a regular directory");
    }
    Ok(())
}

pub(super) fn update_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}
