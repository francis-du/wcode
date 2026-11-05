use super::*;

impl TaskMonitor {
    /// Read-only projection for the protected WebUI. Never expose raw arguments
    /// or reset the TUI's short-lived peak/traffic observation window.
    pub(crate) fn observatory_activity(&self, workspace: &str) -> Value {
        let now = Instant::now();
        let state = self.state.lock().expect("task monitor lock poisoned");
        let stats = state.workspaces.get(workspace).cloned().unwrap_or_default();
        let mut tasks = state
            .tasks
            .iter()
            .filter(|task| task.workspace == workspace)
            .collect::<Vec<_>>();
        tasks.sort_by_key(|task| {
            (
                match task.status {
                    TaskStatus::Running => 0,
                    TaskStatus::Queued => 1,
                    TaskStatus::Failed => 2,
                    TaskStatus::Completed => 3,
                },
                std::cmp::Reverse(task.id),
            )
        });
        let retained = tasks.len();
        let orchestration = tasks
            .iter()
            .filter(|task| !task.slot_counted && task.status == TaskStatus::Running)
            .count();
        let recent = tasks.into_iter().take(12).map(|task| {
            let end = task.finished_at.unwrap_or(now);
            serde_json::json!({
                "id": task.id,
                "command_job_id": task.command_job_id,
                "tool": task.tool.chars().take(80).collect::<String>(),
                // `detail` is a free-form summary of the tool's arguments. The
                // TUI renders it locally; the HTTP projection deliberately does
                // not, so an argument-bearing summary cannot leave the machine.
                // The single structured `target` path is the only extra identity.
                "target": task.target,
                "status": match task.status {
                    TaskStatus::Queued => "queued", TaskStatus::Running => "running",
                    TaskStatus::Completed => "completed", TaskStatus::Failed => "failed",
                },
                "slot_counted": task.slot_counted,
                "wait_ms": task.started_at.unwrap_or(end).saturating_duration_since(task.queued_at).as_millis(),
                "run_ms": task.started_at.map(|start| end.saturating_duration_since(start).as_millis()),
                "finished_ago_ms": task.finished_at.map(|finished| now.saturating_duration_since(finished).as_millis()),
            })
        }).collect::<Vec<_>>();
        let jev_runtime = super::monitor_state_jev::jev_runtime_json(&stats, now);
        serde_json::json!({
            "workspace": workspace, "available": true,
            "active": stats.active, "queued": stats.queued,
            "orchestration": orchestration, "calls": stats.calls,
            "completed": stats.completed, "failed": stats.failed,
            "counter_scope": "workspace_since_process_start",
            "agent_context": {
                "calls": stats.agent_context_calls,
                "model_bytes": stats.agent_context_model_bytes,
                "model_tokens": stats.agent_context_model_tokens,
                "budget_tokens": stats.agent_context_budget_tokens,
                "bytes_avoided": stats.agent_context_bytes_avoided,
                "repo_map_cache_hits": stats.agent_repo_map_cache_hits,
                "repo_map_candidates": stats.agent_repo_map_candidates,
                "repo_map_delivered": stats.agent_repo_map_delivered,
                "build_ms": stats.agent_context_build_ms,
                "decision_runtime": {
                    "jev": jev_runtime,
                },
            },
            "recent": recent, "recent_truncated": retained > 12,
            "history_scope": "bounded_process_memory",
        })
    }
}
