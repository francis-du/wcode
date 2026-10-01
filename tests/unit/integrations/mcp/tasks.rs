use super::*;

#[path = "resume.rs"]
mod resume;

#[test]
fn oversized_task_result_becomes_a_durable_failure() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let mut record = TaskRecord::working(
        "a".repeat(64),
        "demo".into(),
        "semantic_provider_refresh".into(),
        "runtime-a".into(),
    );
    task_store::persist(&workspace, &record).unwrap();
    record.complete(json!({"payload": "x".repeat(4 * 1024 * 1024)}));
    persist_task_result(&workspace, &mut record).unwrap();
    let loaded = task_store::load(&workspace, &record.task_id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.status, TaskStatus::Failed);
    assert!(loaded.result.is_none());
    assert!(loaded.error.unwrap()["message"]
        .as_str()
        .unwrap()
        .contains("could not be persisted"));
}

#[test]
fn successful_task_result_is_not_replaced_by_recovery() {
    let root = tempfile::tempdir().unwrap();
    let workspace = Workspace::new(root.path(), false, false).unwrap();
    let mut record = TaskRecord::working(
        "a".repeat(64),
        "demo".into(),
        "semantic_provider_refresh".into(),
        "runtime-a".into(),
    );
    let result = json!({"isError": false, "structuredContent": {"runs": []}});
    record.complete(result.clone());
    persist_task_result(&workspace, &mut record).unwrap();
    let loaded = task_store::load(&workspace, &record.task_id)
        .unwrap()
        .unwrap();
    assert_eq!(loaded.status, TaskStatus::Completed);
    assert_eq!(loaded.result, Some(result));
}

fn snapshot_live_command_stream(bytes: &[u8]) -> (String, usize, bool) {
    let mut stream = LiveCommandStream::default();
    stream.push(bytes, false);
    stream.snapshot()
}

#[test]
fn live_command_window_is_utf8_safe_bounded_and_redacts_before_tailing() {
    let input = format!(
        "{}\nmarker=尾部\n",
        "x".repeat(MAX_LIVE_COMMAND_STREAM_BYTES + 128)
    );
    let (tail, dropped, redacted) = snapshot_live_command_stream(input.as_bytes());
    assert!(tail.len() <= MAX_LIVE_COMMAND_STREAM_BYTES);
    assert!(dropped > 0);
    assert!(tail.trim_end().ends_with("marker=尾部"));
    assert!(!redacted);

    let secret = format!(
        "api_key={}\n",
        "s".repeat(MAX_LIVE_COMMAND_STREAM_BYTES + 128)
    );
    let (tail, dropped, redacted) = snapshot_live_command_stream(secret.as_bytes());
    let tail = tail.trim_end();
    assert_eq!(tail, "api_key= [REDACTED]");
    assert_eq!(dropped, 0);
    assert!(redacted);
    assert!(!tail.contains(&"s".repeat(128)));
}

#[tokio::test]
async fn finished_task_handle_is_not_a_live_worker() {
    let runtime = TaskRuntime::default();
    let task = tokio::spawn(async {});
    let handle = task.abort_handle();
    task.await.unwrap();
    runtime.register("finished".into(), "demo".into(), handle);
    assert!(!runtime.running("finished"));
    runtime.remove("finished");
    assert!(!runtime.running("finished"));
}
fn monitor_bridge_fixture(roots: &[&std::path::Path]) -> Arc<AppState> {
    let workspaces = crate::workspace::Workspaces::new(roots, true, true).unwrap();
    let ids = workspaces.roots().into_iter().map(|(id, _)| id);
    Arc::new(AppState {
        auth: Arc::new(crate::auth::AuthState::new("http://127.0.0.1:8765".into())),
        monitor: crate::monitor::TaskMonitor::new(ids),
        workspaces,
        harness: crate::harness::ToolHarness::new(1).unwrap(),
        tasks: TaskRuntime::default(),
    })
}

#[tokio::test]
async fn monitor_task_bridge_enforces_exact_workspace_and_ui_only_cancel() {
    use crate::monitor_jobs::{MonitorJobAccess, MonitorJobOrigin};
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    let state = monitor_bridge_fixture(&[first.path(), second.path()]);
    let ids = state.workspaces.roots();
    let workspace_id = &ids[0].0;
    let (_, workspace) = state.workspaces.select(Some(workspace_id)).unwrap();
    let owner = "a".repeat(64);
    let mut mcp = TaskRecord::working(
        owner.clone(),
        workspace_id.clone(),
        "run_command".into(),
        state.auth.instance_id().into(),
    );
    mcp.update_command_output(json!({"stdout":"running\n","stderr":""}));
    task_store::persist(&workspace, &mcp).unwrap();
    let mcp_worker = tokio::spawn(std::future::pending::<()>());
    state.tasks.register(
        mcp.task_id.clone(),
        workspace_id.clone(),
        mcp_worker.abort_handle(),
    );
    let bridge = MonitorTaskAccess {
        state: Arc::downgrade(&state),
    };

    let observed = bridge.observe(workspace_id, &mcp.task_id).unwrap();
    assert_eq!(observed.origin, MonitorJobOrigin::Mcp);
    assert!(!observed.can_cancel);
    assert_eq!(observed.stdout.text, "running\n");
    assert_eq!(observed.success, None);
    assert_eq!(observed.exit_code, None);
    assert!(bridge.cancel(workspace_id, &mcp.task_id).is_err());
    assert!(get_task(&state, &mcp.task_id, &monitor_ui_owner(&state)).is_err());
    assert!(bridge.observe(&ids[1].0, &mcp.task_id).is_err());
    assert!(bridge.cancel(&ids[1].0, &mcp.task_id).is_err());
    assert!(state.tasks.running(&mcp.task_id));
    let unknown = TaskRecord::working(
        owner.clone(),
        workspace_id.clone(),
        "run_command".into(),
        state.auth.instance_id().into(),
    );
    assert!(bridge.observe(workspace_id, &unknown.task_id).is_err());

    let mut ui = TaskRecord::working(
        monitor_ui_owner(&state),
        workspace_id.clone(),
        "run_command".into(),
        state.auth.instance_id().into(),
    );
    ui.update_command_output(json!({"stdout":"","stderr":""}));
    task_store::persist(&workspace, &ui).unwrap();
    let ui_worker = tokio::spawn(std::future::pending::<()>());
    state.tasks.register(
        ui.task_id.clone(),
        workspace_id.clone(),
        ui_worker.abort_handle(),
    );
    assert!(
        bridge
            .observe(workspace_id, &ui.task_id)
            .unwrap()
            .can_cancel
    );
    bridge.cancel(workspace_id, &ui.task_id).unwrap();
    assert_eq!(
        bridge.observe(workspace_id, &ui.task_id).unwrap().status,
        "cancelled"
    );
    assert!(!state.tasks.running(&ui.task_id));
    assert!(ui_worker.await.unwrap_err().is_cancelled());
    cancel_task(&state, &mcp.task_id, &owner).unwrap();
    assert!(mcp_worker.await.unwrap_err().is_cancelled());
    drop(state);
    assert!(bridge
        .observe(workspace_id, &mcp.task_id)
        .unwrap_err()
        .to_string()
        .contains("disconnected"));
}

#[test]
fn monitor_task_bridge_redacts_bounded_utf8_and_preserves_failed_command_outcome() {
    use crate::monitor_jobs::MonitorJobAccess;
    let root = tempfile::tempdir().unwrap();
    let state = monitor_bridge_fixture(&[root.path()]);
    let (workspace_id, workspace) = state.workspaces.select(None).unwrap();
    let stdout = format!(
        "{}\napi_key=synthetic-monitor-secret\nmarker=尾部\n",
        "尾".repeat(MAX_LIVE_COMMAND_STREAM_BYTES)
    );
    let output = json!({
        "stdout": stdout, "stderr": "failure diagnostics", "success": false,
        "exit_code": 7, "truncated": false, "redacted": false,
    });
    let mut record = TaskRecord::working(
        "a".repeat(64),
        workspace_id.clone(),
        "run_command".into(),
        state.auth.instance_id().into(),
    );
    record.complete(json!({"isError":true,"structuredContent":output}));
    task_store::persist(&workspace, &record).unwrap();
    let bridge = MonitorTaskAccess {
        state: Arc::downgrade(&state),
    };
    let observed = bridge.observe(&workspace_id, &record.task_id).unwrap();
    assert_eq!(observed.status, "completed");
    assert_eq!(observed.success, Some(false));
    assert_eq!(observed.exit_code, Some(7));
    assert!(observed.stdout.text.len() <= MAX_LIVE_COMMAND_STREAM_BYTES);
    assert!(observed.stdout.truncated && observed.stdout.redacted);
    assert!(observed.stdout.text.ends_with("marker=尾部\n"));
    assert!(!observed.stdout.text.contains("synthetic-monitor-secret"));
    assert_eq!(observed.stderr.text, "failure diagnostics");
    assert!(monitor_job_stream(Some(&json!({"stdout":null})), "stdout").is_err());
    assert!(monitor_job_stream(Some(&json!({"stderr":1})), "stderr").is_err());
}

#[test]
fn monitor_stream_preserves_unredacted_line_endings_and_redacted_blank_tail() {
    for text in [
        "",
        "running\n",
        "first\r\nlast\r\n",
        "progress\rnext\r",
        "你好\n\n",
    ] {
        let output = json!({"stdout": text});
        let observed = monitor_job_stream(Some(&output), "stdout").unwrap();
        assert_eq!(observed.text, text);
        assert_eq!(observed.total_bytes, text.len() as u64);
        assert!(!observed.redacted);
        assert!(!observed.truncated);
    }
    let text = ["api_key", "=synthetic-log-value\r\n\r\n"].concat();
    let observed = monitor_job_stream(Some(&json!({"stdout": text})), "stdout").unwrap();
    assert!(observed.redacted);
    assert!(observed.text.ends_with("\r\n\r\n"));
    assert!(!observed.text.contains("synthetic-log-value"));
}

#[test]
fn monitor_task_bridge_restart_or_orphan_is_visible_and_never_cancelable() {
    use crate::monitor_jobs::{MonitorJobAccess, MonitorJobOrigin};
    let root = tempfile::tempdir().unwrap();
    let state = monitor_bridge_fixture(&[root.path()]);
    let (workspace_id, workspace) = state.workspaces.select(None).unwrap();
    let bridge = MonitorTaskAccess {
        state: Arc::downgrade(&state),
    };
    for previous_instance in ["previous-runtime", state.auth.instance_id()] {
        let owner = monitor_ui_owner_for_instance(previous_instance);
        let record = TaskRecord::working(
            owner,
            workspace_id.clone(),
            "run_command".into(),
            previous_instance.into(),
        );
        task_store::persist(&workspace, &record).unwrap();
        let observed = bridge.observe(&workspace_id, &record.task_id).unwrap();
        assert_eq!(observed.origin, MonitorJobOrigin::Ui);
        assert_eq!(observed.status, "failed");
        assert_eq!(observed.success, None);
        assert!(!observed.can_cancel);
        assert!(observed.error.as_deref().is_some_and(
            |error| error.contains("restart") || error.contains("without a durable result")
        ));
        if previous_instance != state.auth.instance_id() {
            assert!(bridge.cancel(&workspace_id, &record.task_id).is_err());
        }
    }
    let mut record = TaskRecord::working(
        "a".repeat(64),
        workspace_id.clone(),
        "run_command".into(),
        state.auth.instance_id().into(),
    );
    record.complete(json!({"structuredContent": {"stdout": null, "stderr": ""}}));
    task_store::persist(&workspace, &record).unwrap();
    let error = bridge.observe(&workspace_id, &record.task_id).unwrap_err();
    assert!(error.to_string().contains("stdout"));
}
