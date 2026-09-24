use super::mcp_writer::ActiveWriterLease;
use crate::reconcile::ReconciliationTaskRun;
use crate::workspace::Workspaces;
use std::path::Path;

pub(super) fn writer_consistency_counts(
    workspaces: &Workspaces,
    domain: &Path,
    active: &[ActiveWriterLease],
) -> Result<(usize, usize), String> {
    let durable = durable_writer_records(workspaces, domain)?;
    let orphaned = durable
        .iter()
        .filter(|(workspace_id, plan_id, run)| {
            !active
                .iter()
                .any(|lease| lease_matches_run(lease, workspace_id, plan_id, run))
        })
        .count();
    let stale = active
        .iter()
        .filter(|lease| {
            lease.task_id.is_some()
                && lease.durable_owner_bound
                && !durable.iter().any(|(workspace_id, plan_id, run)| {
                    lease_matches_run(lease, workspace_id, plan_id, run)
                })
        })
        .count();
    Ok((orphaned, stale))
}

fn durable_writer_records(
    workspaces: &Workspaces,
    domain: &Path,
) -> Result<Vec<(String, String, ReconciliationTaskRun)>, String> {
    let mut durable = Vec::new();
    for (workspace_id, _) in workspaces.roots() {
        let (_, workspace) = workspaces
            .select(Some(&workspace_id))
            .map_err(|error| error.to_string())?;
        if workspace
            .mutation_domain_root()
            .map_err(|error| error.to_string())?
            != domain
        {
            continue;
        }
        for (plan_id, run) in
            crate::reconciliation_execution_store::claimed_writer_records(&workspace)
                .map_err(|error| error.to_string())?
        {
            durable.push((workspace_id.clone(), plan_id, run));
        }
    }
    Ok(durable)
}

fn lease_matches_run(
    lease: &ActiveWriterLease,
    workspace_id: &str,
    plan_id: &str,
    run: &ReconciliationTaskRun,
) -> bool {
    let durable_owner = run
        .ownership
        .as_ref()
        .and_then(|ownership| ownership.owner_binding.as_deref());
    lease.workspace == workspace_id
        && lease.plan_id == plan_id
        && lease.task_id.as_deref() == Some(run.task.id.as_str())
        && lease.executor == run.claimed_by.as_deref().unwrap_or_default()
        && durable_owner.is_none_or(|binding| lease.owner_binding == binding)
}
