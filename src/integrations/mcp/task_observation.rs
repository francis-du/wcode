//! Reuse existing owner-checked native jobs; no command-launch capability.
use super::*;
use crate::monitor_jobs::{
    MonitorJobAccess, MonitorJobOrigin, MonitorJobSnapshot, MonitorJobStream,
};

pub(crate) fn list(
    state: &AppState,
    workspace_id: &str,
    workspace: &Workspace,
    verification: bool,
) -> anyhow::Result<Value> {
    let _guard = state
        .tasks
        .state_lock
        .lock()
        .map_err(|_| anyhow::anyhow!("task state unavailable"))?;
    let owner = monitor_ui_owner(state);
    let listing = task_store::list_recent(
        workspace,
        workspace_id,
        if verification {
            "verify_project"
        } else {
            CONDITIONAL_TASK_TOOL
        },
        verification.then_some(owner.as_str()),
    )?;
    let items = listing.records.into_iter().map(|record| {
        let ui = record.owner == monitor_ui_owner_for_instance(&record.runtime_instance_id);
        let known = record.status.terminal() || record.runtime_instance_id == state.auth.instance_id();
        json!({"task_id":record.task_id, "workspace":workspace_id,
            "tool":record.tool_name, "status":if known { serde_json::to_value(record.status).unwrap_or(Value::Null) } else { json!("unknown") },
            "origin":if ui {"ui"} else {"mcp"},
            "can_cancel":known && ui && record.owner == owner && record.status == TaskStatus::Working,
            "created_at_ms":record.created_at_ms, "updated_at_ms":record.updated_at_ms,
            "completion_is_not_success":true})
    }).collect::<Vec<_>>();
    Ok(json!({"schema_version":1,
        "kind":if verification {"verification_task_list"} else {"command_job_list"},
        "workspace":workspace_id, "items":items, "truncated":listing.truncated,
        "coverage":"retained_bounded", "discovery_is_not_execution":true}))
}

pub(crate) fn command(
    state: &Arc<AppState>,
    workspace_id: &str,
    task_id: &str,
    cancel: bool,
) -> anyhow::Result<Value> {
    let access = MonitorTaskAccess {
        state: Arc::downgrade(state),
    };
    // The same bridge validates exact selected Workspace and durable ID.
    let observed = access.observe(workspace_id, task_id)?;
    if cancel {
        if observed.origin != MonitorJobOrigin::Ui || !observed.can_cancel {
            anyhow::bail!("job cancellation denied");
        }
        access.cancel(workspace_id, task_id)?;
        return Ok(projection(access.observe(workspace_id, task_id)?));
    }
    Ok(projection(observed))
}

fn stream(value: MonitorJobStream) -> Value {
    json!({"text":value.text, "total_bytes":value.total_bytes,
        "truncated":value.truncated, "redacted":value.redacted})
}

fn projection(snapshot: MonitorJobSnapshot) -> Value {
    let error = snapshot
        .error
        .as_deref()
        .map(|error| error.chars().take(2000).collect::<String>());
    json!({"schema_version":1,"kind":"command_job", "task_id":snapshot.job_id,
        "workspace":snapshot.workspace,"tool":"run_command","status":snapshot.status,
        "origin":match snapshot.origin {MonitorJobOrigin::Ui=>"ui",MonitorJobOrigin::Mcp=>"mcp"},
        "can_cancel":snapshot.can_cancel,"success":snapshot.success,"exit_code":snapshot.exit_code,
        "error":error, "stdout":stream(snapshot.stdout), "stderr":stream(snapshot.stderr),
        "completion_is_not_success":true,"acceptance_ready":false})
}
