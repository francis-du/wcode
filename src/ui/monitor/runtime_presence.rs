use super::*;

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub(super) struct RuntimeActivityCounts {
    pub active_verifications: u64,
    pub queued_verifications: u64,
    pub active_jobs: u64,
    pub queued_jobs: u64,
}

pub(super) fn runtime_activity_counts(state: &MonitorState) -> RuntimeActivityCounts {
    let mut counts = RuntimeActivityCounts::default();
    for task in &state.tasks {
        match (
            task.tool.as_str(),
            task.status,
            task.command_job_id.is_some(),
        ) {
            ("verify_project", TaskStatus::Running, _) => counts.active_verifications += 1,
            ("verify_project", TaskStatus::Queued, _) => counts.queued_verifications += 1,
            ("run_command", TaskStatus::Running, true) => counts.active_jobs += 1,
            ("run_command", TaskStatus::Queued, true) => counts.queued_jobs += 1,
            _ => {}
        }
    }
    counts
}

#[cfg(test)]
#[path = "../../../tests/unit/ui/monitor/runtime_presence.rs"]
mod tests;
