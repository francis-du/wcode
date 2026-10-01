use super::*;
use ratatui::backend::TestBackend;

#[test]
fn intelligence_refresh_panic_releases_loading_state_and_allows_retry() {
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    let worker = spawn_intelligence_refresh(&monitor, "backend".to_owned(), || {
        panic!("fixture private panic payload");
    })
    .unwrap();
    let joined = worker.join();
    let snapshot = monitor.snapshot();
    let stats = &snapshot.intelligence["backend"];
    assert!(
        !stats.refreshing,
        "a stopped worker must not leave permanent Loading state"
    );
    assert!(
        stats.refresh_error.is_some(),
        "failed refresh must stay visibly stale"
    );
    assert!(!stats
        .refresh_error
        .as_ref()
        .unwrap()
        .contains("private panic payload"));
    assert!(
        joined.is_ok(),
        "refresh panics must be contained at the worker boundary"
    );
    spawn_intelligence_refresh(&monitor, "backend".to_owned(), || Ok(()))
        .expect("a stopped worker must not block retry")
        .join()
        .unwrap();
    let snapshot = monitor.snapshot();
    assert!(!snapshot.intelligence["backend"].refreshing);
    assert!(snapshot.intelligence["backend"].refresh_error.is_none());
}

#[test]
fn intelligence_refresh_error_keeps_cached_data_and_retry_clears_failure() {
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    monitor.record_intelligence_result(
        "backend",
        "worklist_status",
        &serde_json::json!({
            "exists": true, "revision": 7, "items": [], "counts": {}, "complete": false
        }),
    );
    let before = monitor.snapshot().intelligence["backend"]
        .project_worklist
        .clone();
    assert!(before.is_some(), "fixture must retain real cached data");
    spawn_intelligence_refresh(&monitor, "backend".to_owned(), || {
        Err(anyhow::anyhow!("fixture refresh failed"))
    })
    .unwrap()
    .join()
    .unwrap();
    let snapshot = monitor.snapshot();
    let stats = &snapshot.intelligence["backend"];
    assert!(!stats.refreshing);
    assert_eq!(
        stats.refresh_error.as_deref(),
        Some("fixture refresh failed")
    );
    assert_eq!(stats.project_worklist, before);
    spawn_intelligence_refresh(&monitor, "backend".to_owned(), || Ok(()))
        .unwrap()
        .join()
        .unwrap();
    assert!(monitor.snapshot().intelligence["backend"]
        .refresh_error
        .is_none());
}

#[test]
fn intelligence_refresh_single_flight_preserves_other_workspaces() {
    let monitor = TaskMonitor::new(["backend".to_owned(), "frontend".to_owned()]);
    let (release, wait) = std::sync::mpsc::channel();
    let first = spawn_intelligence_refresh(&monitor, "backend".to_owned(), move || {
        wait.recv_timeout(Duration::from_secs(5))?;
        Ok(())
    })
    .unwrap();
    let duplicate = spawn_intelligence_refresh(&monitor, "backend".to_owned(), || {
        panic!("duplicate refresh must never run");
    });
    let other = spawn_intelligence_refresh(&monitor, "frontend".to_owned(), || Ok(()));
    release.send(()).unwrap();
    first.join().unwrap();
    assert!(duplicate.is_none());
    other
        .expect("another Workspace must remain independent")
        .join()
        .unwrap();
    assert!(monitor
        .snapshot()
        .intelligence
        .values()
        .all(|stats| !stats.refreshing));
}

fn fixture() -> (tempfile::TempDir, MonitorConfig, TaskMonitor) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("backend");
    std::fs::create_dir(&path).unwrap();
    let workspaces = Workspaces::new(&[path], true, true).unwrap();
    let config = MonitorConfig {
        version: "test".to_owned(),
        instance_id: "test-instance".to_owned(),
        local_health_url: "http://127.0.0.1:8765/healthz".to_owned(),
        public_url: Arc::new(std::sync::RwLock::new("https://example.test".to_owned())),
        intelligence_url: "http://127.0.0.1:8765/intelligence".to_owned(),
        project_url: "https://github.com/francis-du/wcode".to_owned(),
        author_url: "https://github.com/francis-du".to_owned(),
        author_handle: "@francis-du".to_owned(),
        pairing_code: "123456".to_owned(),
        max_parallel: 8,
        input_token_price_per_million_usd: 2.0,
        semantic_auto: true,
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
    };
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    (root, config, monitor)
}

fn render(
    snapshot: &MonitorSnapshot,
    config: &MonitorConfig,
    ui: &DashboardState,
    width: u16,
    height: u16,
) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| render_intelligence_overlay(frame, frame.area(), snapshot, config, ui))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

#[test]
fn task_detail_uses_canonical_redaction_before_terminal_render() {
    let text = console_clean("Authorization: Bearer fixture_terminal_secret_12345\n\u{001b}[31m");
    assert!(!text.contains("fixture_terminal_secret_12345"));
    assert!(!text.contains('\u{001b}'));
}

fn key(code: KeyCode) -> event::KeyEvent {
    event::KeyEvent::new(code, KeyModifiers::NONE)
}

fn console(tab: ConsoleTab) -> DashboardState {
    DashboardState {
        intelligence_open: true,
        console_tab: tab,
        workspace_focus_id: Some("backend".to_owned()),
        ..Default::default()
    }
}

#[test]
fn console_tabs_keep_global_stop_and_authorization_separate() {
    let (_root, config, monitor) = fixture();
    let snapshot = monitor.snapshot();
    let area = Rect::new(0, 0, 100, 24);
    let mut ui = console(ConsoleTab::Acceptance);
    assert!(handle_console_key(
        key(KeyCode::Tab),
        &mut ui,
        area,
        &snapshot,
        &config
    ));
    assert_eq!(ui.console_tab, ConsoleTab::Attention);
    assert!(handle_console_key(
        event::KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT),
        &mut ui,
        area,
        &snapshot,
        &config
    ));
    assert_eq!(ui.console_tab, ConsoleTab::Acceptance);
    assert!(!handle_console_key(
        event::KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        &mut ui,
        area,
        &snapshot,
        &config
    ));
    assert!(!handle_console_key(
        key(KeyCode::Char('y')),
        &mut ui,
        area,
        &snapshot,
        &config
    ));
    assert!(!handle_console_key(
        key(KeyCode::Tab),
        &mut ui,
        Rect::new(0, 0, 39, 15),
        &snapshot,
        &config
    ));
    ui.workspace_input = Some(String::new());
    assert!(!handle_console_key(
        key(KeyCode::Tab),
        &mut ui,
        area,
        &snapshot,
        &config
    ));
}

#[test]
fn shared_attention_retains_revision_provenance_and_incomplete_observation() {
    let (_root, config, monitor) = fixture();
    monitor.record_intelligence_result("backend", "project_observatory", &serde_json::json!({
        "repository_revision": {"code": "sha256:current-code", "design": "sha256:design"},
        "attention": {
            "total": 9, "partial": true, "truncated": true,
            "items": [{
                "id": "proof:rust", "severity": "high", "subject": "rust proof",
                "message": "Failed proof from compiler", "provider": "rust-check", "precision": "deterministic",
                "section": "verification", "path": "src/lib.rs", "count": 1
            }]
        }
    }));
    let snapshot = monitor.snapshot();
    let ui = console(ConsoleTab::Attention);
    let text = render(&snapshot, &config, &ui, 120, 30);
    assert!(text.contains("1 Acceptance"));
    assert!(text.contains("2 Attention"));
    assert!(text.contains("sha256:current-code"));
    assert!(text.contains("partial"));
    assert!(text.contains("truncated"));
    assert!(text.contains("rust proof"));
    assert!(text.contains("rust-check"));
    assert!(text.contains("deterministic"));
    assert!(text.contains("cached project snapshot"));
    assert!(text.contains("src/lib.rs"));
    assert!(text.contains("verification"));
}

#[test]
fn attention_reports_unknown_project_state_without_claiming_clear() {
    let (_root, config, monitor) = fixture();
    let text = render(
        &monitor.snapshot(),
        &config,
        &console(ConsoleTab::Attention),
        80,
        20,
    );
    assert!(text.contains("Project snapshot unavailable"));
    assert!(text.contains("project state is unknown"));
    assert!(!text.contains("No issues in the observed snapshot"));
}

#[test]
fn attention_routes_failed_calls_to_retained_task_details() {
    let (_root, config, monitor) = fixture();
    let failed = monitor.queue(
        "backend",
        "read_file",
        "src/lib.rs: protected operation",
        32,
    );
    failed.start();
    failed.finish(false, 12);
    let snapshot = monitor.snapshot();
    let mut ui = console(ConsoleTab::Attention);
    assert!(handle_console_key(
        key(KeyCode::Enter),
        &mut ui,
        Rect::new(0, 0, 100, 24),
        &snapshot,
        &config
    ));
    assert_eq!(ui.console_tab, ConsoleTab::Tasks);
    assert_eq!(ui.console_task_id, Some(1));
    let text = render(&snapshot, &config, &ui, 100, 24);
    for expected in [
        "read_file",
        "failed",
        "workspace backend",
        "Wait",
        "Run",
        "32B",
        "12B",
        "src/lib.rs",
    ] {
        assert!(text.contains(expected), "missing {expected}: {text}");
    }
}

#[test]
fn selected_task_identity_survives_runtime_reordering_and_details_scroll() {
    let (_root, config, monitor) = fixture();
    let first = monitor.queue(
        "backend",
        "read_file",
        format!("START {} END_OF_DETAIL", "very long context ".repeat(300)),
        1,
    );
    first.start();
    let second = monitor.queue("backend", "search_code", "second", 1);
    let mut ui = console(ConsoleTab::Tasks);
    let area = Rect::new(0, 0, 100, 24);
    assert!(handle_console_key(
        key(KeyCode::Down),
        &mut ui,
        area,
        &monitor.snapshot(),
        &config
    ));
    assert_eq!(ui.console_task_id, Some(2));
    second.start();
    first.finish(true, 2);
    let snapshot = monitor.snapshot();
    let tasks = console_tasks(&snapshot, "backend");
    assert_eq!(tasks[task_selection(&ui, &tasks)].id, 2);

    ui.console_task_id = Some(1);
    let first_text = render(&snapshot, &config, &ui, 80, 20);
    assert!(first_text.contains("START"));
    assert!(!first_text.contains("END_OF_DETAIL"));
    ui.console_scroll = usize::MAX;
    let last_text = render(&snapshot, &config, &ui, 80, 20);
    assert!(last_text.contains("END_OF_DETAIL"));
    assert!(last_text.contains("PgUp/PgDn"));
    assert_eq!(ui.console_task_id, Some(1));
    // The 80-column overlay has a 72-column inner area, so all six routes
    // use compact labels while task details retain their independent scroll.
    for label in [
        "1 Accept",
        "2 Issues",
        "3 Tasks",
        "4 LSP/AI",
        "5 Agents",
        "6 Observe",
        "Tasks · Tab / Shift-Tab · 1-6",
    ] {
        assert!(last_text.contains(label), "missing {label}: {last_text}");
    }
    let wide_text = render(&snapshot, &config, &ui, 120, 30);
    assert!(wide_text.contains("1 Acceptance"));
    assert!(wide_text.contains("6 Observations"));
    assert!(wide_text.contains("END_OF_DETAIL"));

    assert!(handle_console_key(
        key(KeyCode::Char('6')),
        &mut ui,
        area,
        &snapshot,
        &config
    ));
    assert_eq!(ui.console_tab, ConsoleTab::Summary);
    assert_eq!(ui.console_scroll, 0);
    assert!(
        render(&snapshot, &config, &ui, 80, 20).contains("Observations · Tab / Shift-Tab · 1-6")
    );
    assert!(handle_console_key(
        key(KeyCode::Tab),
        &mut ui,
        area,
        &snapshot,
        &config
    ));
    assert_eq!(ui.console_tab, ConsoleTab::Acceptance);
}

#[test]
fn providers_show_unknown_jev_and_semantic_freshness_then_scroll_to_tunnels() {
    let (_root, config, monitor) = fixture();
    monitor.record_intelligence_result(
        "backend",
        "semantic_session_status",
        &serde_json::json!({
            "sessions": 2, "documents": 5, "starts": 3, "requests": 12
        }),
    );
    let mut snapshot = monitor.snapshot();
    snapshot.intelligence.get_mut("backend").unwrap().lsp_stale = 4;
    snapshot.tunnel_runtime.push(MonitorTunnelRuntimeStatus {
        provider: "test-provider".to_owned(),
        url: Some("https://example.test".to_owned()),
        role: "retrying".to_owned(),
        state: "circuit-open".to_owned(),
        lease_age_seconds: None,
        consecutive_failures: 3,
        death_count: 2,
        circuit_open: true,
        retry_in_seconds: Some(45),
        connected_seconds: None,
    });
    let mut ui = console(ConsoleTab::Providers);
    let text = render(&snapshot, &config, &ui, 120, 30);
    assert!(text.contains("stale 4"));
    assert!(text.contains("Jev observation unavailable"));
    ui.console_scroll = usize::MAX;
    let scrolled = render(&snapshot, &config, &ui, 80, 20);
    assert!(scrolled.contains("test-provider"));
    assert!(scrolled.contains("circuit-open"));
    // The retry label and value can occupy separate rows on narrow terminals.
    assert!(scrolled.contains("retry "));
    assert!(scrolled.contains("45s"));
}

#[test]
fn attention_enter_opens_selected_authorization_without_approving_it() {
    let (_root, config, monitor) = fixture();
    let mut ui = console(ConsoleTab::Attention);
    ui.pending_authorizations.push(AuthorizationRequest {
        id: "AUTH-1".to_owned(),
        workspace: "backend".to_owned(),
        kind: crate::authorization::AuthorizationKind::CommandAccess,
        summary: "cargo".to_owned(),
        program: Some("cargo".to_owned()),
        fingerprint: "test".to_owned(),
        status: AuthorizationStatus::Pending,
        created_at_ms: 1,
        decided_at_ms: None,
    });
    assert!(handle_console_key(
        key(KeyCode::Enter),
        &mut ui,
        Rect::new(0, 0, 100, 24),
        &monitor.snapshot(),
        &config
    ));
    assert!(!ui.intelligence_open);
    assert_eq!(
        ui.pending_authorizations[0].status,
        AuthorizationStatus::Pending
    );
}
