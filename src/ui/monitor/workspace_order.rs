use super::*;

pub(super) fn pending_authorizations(config: &MonitorConfig) -> Vec<AuthorizationRequest> {
    config
        .workspaces
        .authorization_requests(256)
        .into_iter()
        .filter(|request| request.status == AuthorizationStatus::Pending)
        .collect()
}

pub(super) fn configured_workspaces(config: &MonitorConfig) -> Vec<(String, String, bool)> {
    config
        .workspaces
        .roots()
        .into_iter()
        .map(|(id, root)| {
            let is_default = id == config.workspaces.default_id();
            (id, root.display().to_string(), is_default)
        })
        .collect()
}

pub(super) fn workspace_last_task<'a>(
    snapshot: &'a MonitorSnapshot,
    workspace_id: &str,
) -> Option<&'a TaskRecord> {
    snapshot
        .tasks
        .iter()
        .rev()
        .find(|task| task.workspace == workspace_id)
}

pub(super) fn workspace_last_activity(
    snapshot: &MonitorSnapshot,
    workspace_id: &str,
) -> Option<Instant> {
    workspace_last_task(snapshot, workspace_id).map(|task| {
        task.finished_at
            .or(task.started_at)
            .unwrap_or(task.queued_at)
    })
}

pub(super) fn workspace_recent_activity(
    snapshot: &MonitorSnapshot,
    workspace_id: &str,
) -> Option<Instant> {
    workspace_last_activity(snapshot, workspace_id)
        .filter(|activity| activity.elapsed() <= WORKSPACE_RECENT_ACTIVITY_WINDOW)
}

pub(super) fn workspace_recent_failure(snapshot: &MonitorSnapshot, workspace_id: &str) -> bool {
    workspace_recent_activity(snapshot, workspace_id).is_some()
        && workspace_last_task(snapshot, workspace_id)
            .is_some_and(|task| task.status == TaskStatus::Failed)
}

pub(super) fn approval_counts(pending: &[AuthorizationRequest]) -> BTreeMap<String, usize> {
    let mut approvals = BTreeMap::<String, usize>::new();
    for request in pending {
        *approvals.entry(request.workspace.clone()).or_default() += 1;
    }
    approvals
}

pub(super) fn ordered_workspaces(
    config: &MonitorConfig,
    snapshot: &MonitorSnapshot,
) -> Vec<(String, String, bool)> {
    ordered_workspaces_with_approvals(
        config,
        snapshot,
        &approval_counts(&pending_authorizations(config)),
    )
}

pub(super) fn ordered_workspaces_with_approvals(
    config: &MonitorConfig,
    snapshot: &MonitorSnapshot,
    approvals: &BTreeMap<String, usize>,
) -> Vec<(String, String, bool)> {
    let mut workspaces = configured_workspaces(config)
        .into_iter()
        .enumerate()
        .collect::<Vec<_>>();
    workspaces.sort_by(|(left_index, left), (right_index, right)| {
        // Borrow stats instead of cloning the full WorkspaceStats record on
        // every comparison during the sort.
        let empty = WorkspaceStats::default();
        let left_stats = snapshot.workspaces.get(&left.0).unwrap_or(&empty);
        let right_stats = snapshot.workspaces.get(&right.0).unwrap_or(&empty);
        let left_approvals = approvals.get(&left.0).copied().unwrap_or(0);
        let right_approvals = approvals.get(&right.0).copied().unwrap_or(0);
        right_stats
            .active
            .cmp(&left_stats.active)
            .then_with(|| right_stats.queued.cmp(&left_stats.queued))
            .then_with(|| right_approvals.cmp(&left_approvals))
            .then_with(|| {
                workspace_recent_failure(snapshot, &right.0)
                    .cmp(&workspace_recent_failure(snapshot, &left.0))
            })
            .then_with(|| {
                workspace_recent_activity(snapshot, &right.0)
                    .cmp(&workspace_recent_activity(snapshot, &left.0))
            })
            .then_with(|| left_index.cmp(right_index))
    });
    workspaces
        .into_iter()
        .map(|(_, workspace)| workspace)
        .collect()
}

pub(super) fn focused_workspace_id_from(
    workspaces: &[(String, String, bool)],
    focus: usize,
) -> Option<String> {
    workspaces
        .get(focus.min(workspaces.len().saturating_sub(1)))
        .map(|workspace| workspace.0.clone())
}

pub(super) fn focused_workspace_id(
    config: &MonitorConfig,
    snapshot: &MonitorSnapshot,
    focus: usize,
) -> Option<String> {
    focused_workspace_id_from(&ordered_workspaces(config, snapshot), focus)
}
