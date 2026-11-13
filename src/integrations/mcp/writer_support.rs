use super::*;

pub(super) fn registered_linked_worktree(workspaces: &Workspaces, workspace: &Workspace) -> bool {
    workspaces.roots().into_iter().any(|(id, _)| {
        workspaces
            .select(Some(&id))
            .ok()
            .is_some_and(|(_, candidate)| {
                workspace.is_linked_worktree_of(&candidate).unwrap_or(false)
            })
    })
}

pub(super) fn is_writer_run(run: &ReconciliationTaskRun) -> bool {
    matches!(
        run.task.kind,
        ReconciliationTaskKind::Design | ReconciliationTaskKind::Implementation
    ) && run
        .ownership
        .as_ref()
        .is_some_and(|ownership| ownership.mode == ReconciliationClaimMode::SharedWriter)
}

pub(super) fn validate_identity(value: &str, max: usize, label: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > max {
        return Err(format!("{label} is invalid"));
    }
    Ok(())
}

pub(super) fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub(super) fn mutation_domain_id(domain: &Path) -> String {
    let digest = digest(&domain.to_string_lossy());
    format!("md-{}", &digest[..16])
}

pub(super) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| u64::try_from(duration.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or(0)
}
