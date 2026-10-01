use super::*;
use ratatui::backend::TestBackend;
use std::sync::atomic::AtomicUsize;

fn fixture() -> (tempfile::TempDir, MonitorConfig, TaskMonitor) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("backend");
    std::fs::create_dir(&path).unwrap();
    let config = MonitorConfig {
        version: "test".into(),
        instance_id: "fixture".into(),
        local_health_url: "http://127.0.0.1:8765/healthz".into(),
        public_url: Arc::new(std::sync::RwLock::new("https://example.test".into())),
        intelligence_url: "http://127.0.0.1:8765/intelligence".into(),
        project_url: "https://example.test/project".into(),
        author_url: "https://example.test/author".into(),
        author_handle: "fixture".into(),
        pairing_code: "123456".into(),
        max_parallel: 4,
        input_token_price_per_million_usd: 0.0,
        semantic_auto: false,
        workspaces: Workspaces::new(&[path], true, true).unwrap(),
        harness: ToolHarness::new(2).unwrap(),
    };
    (root, config, TaskMonitor::new(["backend".into()]))
}

fn observation(origin: MonitorJobOrigin) -> MonitorJobSnapshot {
    MonitorJobSnapshot {
        job_id: "job-exact".into(),
        workspace: "backend".into(),
        status: "working".into(),
        origin,
        can_cancel: true,
        stdout: MonitorJobStream {
            text: "actual output\n".into(),
            total_bytes: 14,
            ..Default::default()
        },
        stderr: MonitorJobStream {
            text: "actual diagnostic\n".into(),
            total_bytes: 18,
            ..Default::default()
        },
        exit_code: None,
        success: None,
        error: None,
    }
}

fn ui_for(task: &TaskRecord, observation: MonitorJobSnapshot) -> DashboardState {
    DashboardState {
        intelligence_open: true,
        console_tab: ConsoleTab::Tasks,
        console_task_id: Some(task.id),
        command_job: CommandJobView {
            selection: Some((task.id, task.workspace.clone(), "job-exact".into())),
            snapshot: Some(observation),
            observed_at: Some(Instant::now()),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn render(task: &TaskRecord, ui: &DashboardState, scroll: usize) -> String {
    let mut terminal = Terminal::new(TestBackend::new(90, 16)).unwrap();
    terminal
        .draw(|frame| console_paragraph(frame, frame.area(), command_job_lines(task, ui), scroll))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

struct Access {
    monitor: TaskMonitor,
    cancelled: AtomicUsize,
    reject_cancel: bool,
    mismatch: bool,
}
impl MonitorJobAccess for Access {
    fn observe(&self, workspace: &str, job_id: &str) -> anyhow::Result<MonitorJobSnapshot> {
        assert!(
            self.monitor.state.try_lock().is_ok(),
            "observation held the monitor mutex"
        );
        assert_eq!((workspace, job_id), ("backend", "job-exact"));
        let mut snapshot = observation(MonitorJobOrigin::Ui);
        if self.mismatch {
            snapshot.job_id = "different-job".into();
        }
        Ok(snapshot)
    }
    fn cancel(&self, workspace: &str, job_id: &str) -> anyhow::Result<()> {
        assert!(
            self.monitor.state.try_lock().is_ok(),
            "cancellation held the monitor mutex"
        );
        assert_eq!((workspace, job_id), ("backend", "job-exact"));
        self.cancelled.fetch_add(1, Ordering::SeqCst);
        if self.reject_cancel {
            anyhow::bail!("runtime owner mismatch");
        }
        Ok(())
    }
}

#[test]
fn task_logs_render_actual_streams_exit_redaction_and_explicit_truncation() {
    let (_root, _config, monitor) = fixture();
    let ticket = monitor.queue("backend", "run_command", "native command", 0);
    ticket.bind_command_job("job-exact");
    let snapshot = monitor.snapshot();
    let task = &snapshot.tasks[0];
    let mut data = observation(MonitorJobOrigin::Mcp);
    data.status = "completed".into();
    data.exit_code = Some(7);
    data.success = Some(false);
    data.stdout = MonitorJobStream {
        text: format!("{}TAIL-OUTPUT", "界".repeat(12000)),
        total_bytes: 36011,
        truncated: true,
        redacted: false,
    };
    data.stderr = MonitorJobStream {
        text: "Authorization: Bearer fixture_log_secret\nACTUAL-STDERR\u{001b}[31m".into(),
        total_bytes: 80,
        truncated: false,
        redacted: true,
    };
    let ui = ui_for(task, data);
    let first = render(task, &ui, 0);
    assert!(first.contains("MCP-owned · observation-only"));
    assert!(first.contains("command failed · exit 7"));
    assert!(first.contains("tail truncated"));
    assert!(!first.contains("X request stop"));
    let tail = render(task, &ui, usize::MAX);
    assert!(tail.contains("TAIL-OUTPUT"));
    assert!(tail.contains("ACTUAL-STDERR"));
    assert!(!tail.contains("fixture_log_secret"));
    assert!(!tail.contains('\u{001b}'));
}

#[tokio::test]
async fn command_job_stop_requires_owned_exact_visible_press_and_rechecks_backend() {
    let (_root, config, monitor) = fixture();
    let ticket = monitor.queue("backend", "run_command", "native command", 0);
    ticket.bind_command_job("job-exact");
    let snapshot = monitor.snapshot();
    let access = Arc::new(Access {
        monitor: monitor.clone(),
        cancelled: AtomicUsize::new(0),
        reject_cancel: false,
        mismatch: false,
    });
    monitor.register_job_access(access.clone());
    let mut ui = ui_for(&snapshot.tasks[0], observation(MonitorJobOrigin::Mcp));
    let area = Rect::new(0, 0, 100, 30);
    let plain = event::KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE);
    assert!(handle_command_job_key(
        plain, &mut ui, area, &monitor, &snapshot, &config
    ));
    assert_eq!(access.cancelled.load(Ordering::SeqCst), 0);
    ui.command_job.snapshot.as_mut().unwrap().origin = MonitorJobOrigin::Ui;
    for kind in [KeyEventKind::Repeat, KeyEventKind::Release] {
        let mut key = plain;
        key.kind = kind;
        assert!(!handle_command_job_key(
            key, &mut ui, area, &monitor, &snapshot, &config
        ));
    }
    assert!(!handle_command_job_key(
        event::KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL),
        &mut ui,
        area,
        &monitor,
        &snapshot,
        &config
    ));
    ui.help_open = true;
    assert!(!handle_command_job_key(
        plain, &mut ui, area, &monitor, &snapshot, &config
    ));
    ui.help_open = false;
    ui.command_job.snapshot.as_mut().unwrap().workspace = "another-workspace".into();
    assert!(handle_command_job_key(
        plain, &mut ui, area, &monitor, &snapshot, &config
    ));
    assert_eq!(access.cancelled.load(Ordering::SeqCst), 0);
    ui.command_job.snapshot.as_mut().unwrap().workspace = "backend".into();
    assert!(handle_command_job_key(
        plain, &mut ui, area, &monitor, &snapshot, &config
    ));
    for _ in 0..50 {
        if access.cancelled.load(Ordering::SeqCst) == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(access.cancelled.load(Ordering::SeqCst), 1);
    assert!(ui
        .workspace_message
        .as_deref()
        .unwrap()
        .contains("awaiting runtime confirmation"));
}

#[tokio::test]
async fn command_job_observer_rejects_identity_mismatch_without_blank_success() {
    let (_root, config, monitor) = fixture();
    let ticket = monitor.queue("backend", "run_command", "native command", 0);
    ticket.bind_command_job("job-exact");
    ticket.bind_command_job("replacement-job");
    let snapshot = monitor.snapshot();
    assert_eq!(
        snapshot.tasks[0].command_job_id.as_deref(),
        Some("job-exact")
    );
    let access = Arc::new(Access {
        monitor: monitor.clone(),
        cancelled: AtomicUsize::new(0),
        reject_cancel: false,
        mismatch: true,
    });
    monitor.register_job_access(access);
    let mut ui = DashboardState {
        intelligence_open: true,
        console_tab: ConsoleTab::Tasks,
        ..Default::default()
    };
    refresh_command_job(&mut ui, &monitor, &snapshot, &config);
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        refresh_command_job(&mut ui, &monitor, &snapshot, &config);
        if ui.command_job.error.is_some() {
            break;
        }
    }
    assert!(ui.command_job.snapshot.is_none());
    assert!(render(&snapshot.tasks[0], &ui, 0).contains("identity mismatch"));
    assert!(!render(&snapshot.tasks[0], &ui, 0).contains("No output observed"));
}

#[tokio::test]
async fn command_job_cancel_rejection_preserves_unknown_state_and_cached_output() {
    let (_root, config, monitor) = fixture();
    let ticket = monitor.queue("backend", "run_command", "native command", 0);
    ticket.bind_command_job("job-exact");
    let snapshot = monitor.snapshot();
    let access = Arc::new(Access {
        monitor: monitor.clone(),
        cancelled: AtomicUsize::new(0),
        reject_cancel: true,
        mismatch: false,
    });
    monitor.register_job_access(access);
    let mut ui = ui_for(&snapshot.tasks[0], observation(MonitorJobOrigin::Ui));
    assert!(handle_command_job_key(
        event::KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE),
        &mut ui,
        Rect::new(0, 0, 100, 30),
        &monitor,
        &snapshot,
        &config
    ));
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(10)).await;
        refresh_command_job(&mut ui, &monitor, &snapshot, &config);
        if ui.command_job.error.is_some() {
            break;
        }
    }
    let text = render(&snapshot.tasks[0], &ui, 0);
    assert!(text.contains("runtime owner mismatch"));
    assert!(text.contains("current job state is unknown"));
    assert!(text.contains("actual output"));
    assert!(!text.contains("X request stop"));
}

#[test]
fn unbound_command_logs_and_unobserved_exit_stay_explicitly_unknown() {
    let (_root, _config, monitor) = fixture();
    let ticket = monitor.queue("backend", "run_command", "native command", 0);
    let snapshot = monitor.snapshot();
    assert!(render(&snapshot.tasks[0], &DashboardState::default(), 0)
        .contains("no durable job was bound"));
    ticket.bind_command_job("job-exact");
    let snapshot = monitor.snapshot();
    let ui = ui_for(&snapshot.tasks[0], observation(MonitorJobOrigin::Ui));
    let text = render(&snapshot.tasks[0], &ui, 0);
    assert!(text.contains("command outcome unknown · exit unknown"));
    assert!(!text.contains("command passed"));
}

#[tokio::test]
async fn monitor_task_bridge_real_run_command_binds_only_durable_id_and_ignores_claimed_ui_owner() {
    let root = tempfile::tempdir().unwrap();
    let workspaces = Workspaces::new([root.path()], true, true).unwrap();
    let workspace_id = workspaces.default_id().to_owned();
    let monitor = TaskMonitor::new([workspace_id.clone()]);
    let state = Arc::new(crate::mcp::AppState {
        auth: Arc::new(crate::auth::AuthState::new("http://127.0.0.1:8765".into())),
        workspaces,
        harness: ToolHarness::new(1).unwrap(),
        monitor: monitor.clone(),
        tasks: crate::mcp_tasks::TaskRuntime::default(),
    });
    let _router = crate::mcp::router(state.clone());
    let access = monitor
        .job_access()
        .expect("router registers the real task bridge");
    state.auth.insert_test_access_token(
        "monitor-task-fixture",
        "monitor-client",
        "http://127.0.0.1:8765/mcp",
    );
    let mut headers = axum::http::HeaderMap::new();
    headers.insert(
        axum::http::header::AUTHORIZATION,
        axum::http::HeaderValue::from_static("Bearer monitor-task-fixture"),
    );
    let owner = state
        .auth
        .authorized_client_fingerprint_for(&headers, "http://127.0.0.1:8765")
        .unwrap();
    let claimed_ui_owner = format!("ui:{}", state.auth.instance_id());
    assert_ne!(owner, claimed_ui_owner);
    let created = crate::mcp_tasks::create_tool_task(
        state.clone(),
        serde_json::json!({
            "name": "run_command", "owner": claimed_ui_owner, "origin": "ui",
            "arguments": {
                "program": "git", "args": ["--version"],
                "task_mode": true, "timeout_seconds": 30, "workspace": workspace_id,
            },
        }),
        owner.clone(),
    )
    .await
    .unwrap();
    let job_id = created["taskId"].as_str().unwrap();
    let terminal = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let result = crate::mcp_tasks::get_task(&state, job_id, &owner).unwrap();
            if result["status"] != "working" {
                break result;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("real command reaches a durable terminal state");
    assert_eq!(terminal["status"], "completed");
    let snapshot = monitor.snapshot();
    let task = snapshot
        .tasks
        .iter()
        .find(|task| task.tool == "run_command")
        .unwrap();
    assert_eq!(task.workspace, workspace_id);
    assert_eq!(task.command_job_id.as_deref(), Some(job_id));
    let observed = access.observe(&workspace_id, job_id).unwrap();
    assert_eq!(observed.status, "completed");
    assert_eq!(observed.origin, MonitorJobOrigin::Mcp);
    assert!(!observed.can_cancel);
    assert_eq!(observed.success, Some(true));
    assert_eq!(observed.exit_code, Some(0));
    assert!(observed.stdout.text.contains("git version"));
    assert!(observed.error.is_none());
    assert!(access.cancel(&workspace_id, job_id).is_err());
    assert!(crate::mcp_tasks::get_task(&state, job_id, &claimed_ui_owner).is_err());
    let mut ui = ui_for(task, observed);
    ui.command_job.selection = Some((task.id, workspace_id.clone(), job_id.to_owned()));
    let text = render(task, &ui, 0);
    assert!(text.contains("MCP-owned · observation-only"));
    assert!(text.contains("command passed · exit 0"));
    assert!(text.contains("git version"));
    assert!(!text.contains("X request stop"));

    crate::mcp::call_tool_owned(
        &state,
        serde_json::json!({"name": "run_command", "arguments": {
            "program": "git", "args": ["--version"], "workspace": workspace_id,
        }}),
        &owner,
    )
    .await
    .unwrap();
    crate::mcp::call_tool_owned(
        &state,
        serde_json::json!({"name": "path_info", "arguments": {
            "path": ".", "workspace": workspace_id,
        }}),
        &owner,
    )
    .await
    .unwrap();
    let snapshot = monitor.snapshot();
    assert!(snapshot
        .tasks
        .iter()
        .any(|record| record.tool == "run_command" && record.command_job_id.is_none()));
    assert!(snapshot
        .tasks
        .iter()
        .any(|record| record.tool == "path_info"));
    assert!(snapshot
        .tasks
        .iter()
        .filter(|record| record.tool == "path_info")
        .all(|record| record.command_job_id.is_none()));
}
