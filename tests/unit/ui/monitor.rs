use super::*;
use ratatui::backend::TestBackend;

#[path = "authorization_layout.rs"]
mod authorization_layout;
#[path = "monitor/console.rs"]
mod console;
#[path = "monitor_health.rs"]
mod monitor_health;
#[path = "process_metrics.rs"]
mod process_metrics;

#[test]
fn authorization_details_are_scrollable_without_losing_the_controls() {
    let (_root, workspaces) = monitor_test_workspaces(&["backend"]);
    let config = monitor_test_config(workspaces);
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    let mut request = monitor_test_request();
    request.summary = format!(
        "START_OF_OPERATION {}\nEND_OF_OPERATION",
        "详细参数 abcdef ".repeat(100)
    );
    let mut ui = DashboardState {
        pending_authorizations: vec![request],
        ..Default::default()
    };
    let first = monitor_test_text(&monitor, &config, 100, 24, &ui);
    assert!(first.contains("START_OF_OPERATION"));
    assert!(first.contains("PgUp/PgDn"));
    assert!(first.contains("approve selected"));
    ui.authorization_scroll = usize::MAX;
    let last = monitor_test_text(&monitor, &config, 100, 24, &ui);
    assert!(last.contains("END_OF_OPERATION"));
    assert!(last.contains("approve selected"));
    assert!(last.contains("deny selected"));
}

fn monitor_test_workspaces(names: &[&str]) -> (tempfile::TempDir, Workspaces) {
    let root = tempfile::tempdir().unwrap();
    let paths = names
        .iter()
        .map(|name| {
            let path = root.path().join(name);
            std::fs::create_dir(&path).unwrap();
            path
        })
        .collect::<Vec<_>>();
    let workspaces = Workspaces::new(&paths, true, true).unwrap();
    (root, workspaces)
}

fn monitor_test_config(workspaces: Workspaces) -> MonitorConfig {
    MonitorConfig {
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
        input_token_price_per_million_usd: 5.0,
        semantic_auto: true,
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
    }
}

fn monitor_test_request() -> AuthorizationRequest {
    AuthorizationRequest {
        id: "AUTH-00000001".to_owned(),
        workspace: "backend".to_owned(),
        kind: crate::authorization::AuthorizationKind::CommandAccess,
        summary: "authorize command: git".to_owned(),
        program: Some("git".to_owned()),
        fingerprint: "test-request".to_owned(),
        status: AuthorizationStatus::Pending,
        created_at_ms: 1,
        decided_at_ms: None,
    }
}

fn monitor_test_text(
    monitor: &TaskMonitor,
    config: &MonitorConfig,
    width: u16,
    height: u16,
    ui: &DashboardState,
) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| draw_dashboard(frame, &monitor.snapshot(), config, 0, ui))
        .unwrap();
    terminal
        .backend()
        .buffer()
        .content
        .chunks(usize::from(width))
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn dashboard_prioritizes_tasks_without_overview_in_both_languages() {
    let (_root, workspaces) = monitor_test_workspaces(&["backend"]);
    let config = monitor_test_config(workspaces);
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    for provider in ["cloudflare", "localhost.run", "pinggy", "tailscale"] {
        monitor.register_tunnel(provider, &format!("https://{provider}.test"));
    }
    let mut task = monitor.queue("backend", "read_file", "src/lib.rs", 1);
    task.start();
    for connected in [false, true] {
        if connected {
            monitor.mark_mcp_initialized();
        }
        for (width, height) in [(40, 24), (80, 24), (100, 32), (140, 40)] {
            for language in [UiLanguage::En, UiLanguage::ZhCn] {
                let ui = DashboardState {
                    language,
                    ..DashboardState::default()
                };
                let text = monitor_test_text(&monitor, &config, width, height, &ui);
                assert!(!text.contains("OVERVIEW") && !text.contains("总览"));
                assert!(
                    text.contains("read_file"),
                    "task missing at {width}x{height}"
                );
                assert!(text.contains("123456"));
                if !connected {
                    for provider in ["cloudflare", "localhost.run", "pinggy", "tailscale"] {
                        assert!(
                            text.contains(provider),
                            "missing {provider} at {width}x{height}"
                        );
                    }
                }
                if height < 28 {
                    assert!(!text.contains("THROUGHPUT") && !text.contains("吞吐量"));
                }
            }
        }
    }
}

#[test]
fn minimum_connected_dashboard_contains_real_task_rows() {
    let (_root, workspaces) = monitor_test_workspaces(&["backend"]);
    let config = monitor_test_config(workspaces);
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    monitor.mark_mcp_initialized();
    let _first = monitor.queue("backend", "read_file", "first", 1);
    let _second = monitor.queue("backend", "search_code", "second", 1);
    let text = monitor_test_text(&monitor, &config, 80, 14, &DashboardState::default());
    assert!(text.contains("read_file"));
    assert!(text.contains("search_code"));
}

#[test]
fn hidden_or_tiny_authorization_overlays_are_not_actionable() {
    let area = Rect::new(0, 0, 100, 24);
    let mut ui = DashboardState {
        pending_authorizations: vec![monitor_test_request()],
        ..DashboardState::default()
    };
    assert!(ui.authorization_visible(area));
    assert!(!ui.authorization_visible(Rect::new(0, 0, 39, 24)));
    assert!(!ui.authorization_visible(Rect::new(0, 0, 100, 9)));
    ui.help_open = true;
    assert!(!ui.authorization_visible(area));
    ui.help_open = false;
    ui.commands_open = true;
    assert!(!ui.authorization_visible(area));
    ui.commands_open = false;
    ui.intelligence_open = true;
    assert!(!ui.authorization_visible(area));
    ui.intelligence_open = false;
    ui.workspace_input = Some(String::new());
    assert!(!ui.authorization_visible(area));
    ui.workspace_input = None;
    ui.full_access_confirm = true;
    assert!(!ui.authorization_visible(area));
    assert!(ui.full_access_visible(area));
    assert!(!ui.full_access_visible(Rect::new(0, 0, 47, 24)));
    assert!(!ui.full_access_visible(Rect::new(0, 0, 100, 11)));
}

#[test]
fn authorization_controls_stay_above_status_messages() {
    let (_root, workspaces) = monitor_test_workspaces(&["backend"]);
    let config = monitor_test_config(workspaces);
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    let ui = DashboardState {
        pending_authorizations: vec![monitor_test_request()],
        workspace_message: Some("previous operation finished".to_owned()),
        ..DashboardState::default()
    };
    let text = monitor_test_text(&monitor, &config, 100, 24, &ui);
    assert!(text.contains("AUTH-00000001"));
    assert!(text.contains("approve selected"));
    assert!(text.contains("deny selected"));
}

#[test]
fn help_clicks_match_rendered_rows_and_do_not_click_through() {
    let (_root, workspaces) = monitor_test_workspaces(&["backend"]);
    let config = monitor_test_config(workspaces);
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    let ui = DashboardState {
        help_open: true,
        ..DashboardState::default()
    };
    for (width, height) in [(70, 18), (140, 32)] {
        let text = monitor_test_text(&monitor, &config, width, height, &ui);
        for (label, target) in [
            ("Project:", &config.project_url),
            ("Health:", &config.local_health_url),
        ] {
            let row = text.lines().position(|line| line.contains(label)).unwrap() as u16;
            assert!((0..width).any(|column| {
                let click = MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column,
                    row,
                    modifiers: KeyModifiers::NONE,
                };
                dashboard_link_at(&click, width, height, &ui, &config).as_ref() == Some(target)
            }));
        }
        for row in [height - 2, height - 1] {
            for column in 0..width {
                let click = MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column,
                    row,
                    modifiers: KeyModifiers::NONE,
                };
                assert!(dashboard_link_at(&click, width, height, &ui, &config).is_none());
            }
        }
    }
    assert!(help_link_at((3, 13), Rect::new(0, 0, 70, 14), &config).is_none());
}

#[test]
fn chinese_footer_setup_hitbox_uses_display_columns() {
    let (_root, workspaces) = monitor_test_workspaces(&["backend"]);
    let config = monitor_test_config(workspaces);
    for language in [UiLanguage::En, UiLanguage::ZhCn] {
        let ui = DashboardState {
            language,
            ..DashboardState::default()
        };
        let mut terminal = Terminal::new(TestBackend::new(140, 32)).unwrap();
        terminal
            .draw(|frame| render_footer(frame, Rect::new(0, 30, 140, 2), &config, language))
            .unwrap();
        let row = &terminal.backend().buffer().content[140 * 31..140 * 32];
        let column = row.iter().position(|cell| cell.symbol() == "O").unwrap() as u16;
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row: 31,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            dashboard_link_at(&click, 140, 32, &ui, &config),
            Some(config.setup_url())
        );
    }
}

#[test]
fn truncation_preserves_graphemes_and_respects_terminal_width() {
    for value in ["abcdef", "工作区路径", "e\u{301}abc", "👩‍💻文件.rs"] {
        for width in 0..16 {
            assert!(Span::raw(truncate_end(value, width)).width() <= width);
            assert!(Span::raw(truncate_middle(value, width)).width() <= width);
        }
    }
    assert_eq!(truncate_end("工作区", 5), "工作…");
    assert_eq!(truncate_end("e\u{301}abc", 2), "e\u{301}…");
    assert_eq!(truncate_end("👩‍💻abc", 3), "👩‍💻…");
    assert_eq!(truncate_middle("abcdef", 5), "ab…ef");
}

#[test]
fn sparkline_handles_saturated_counters_without_overflow() {
    assert_eq!(sparkline(&[0, u64::MAX]), "▁█");
    assert_eq!(sparkline(&[]), "");
    assert_eq!(sparkline(&[0, 0]), "▁▁");
}

#[test]
fn tracks_task_lifecycle_per_workspace_and_bytes() {
    let monitor = TaskMonitor::new(["api".to_owned(), "web".to_owned()]);
    let mut ticket = monitor.queue("web", "read_file", "src/main.rs · lines 1-80", 128);
    let snapshot = monitor.snapshot();
    assert_eq!(snapshot.workspaces["web"].queued, 1);
    assert_eq!(snapshot.workspaces["web"].calls, 1);
    assert_eq!(snapshot.workspaces["web"].request_bytes, 128);
    assert_eq!(snapshot.workspaces["api"].calls, 0);

    ticket.start();
    assert_eq!(monitor.snapshot().workspaces["web"].active, 1);
    ticket.finish_with_context_savings(true, 512, 4_096);

    let snapshot = monitor.snapshot();
    assert_eq!(snapshot.workspaces["web"].active, 0);
    assert_eq!(snapshot.workspaces["web"].completed, 1);
    assert_eq!(snapshot.workspaces["web"].response_bytes, 512);
    assert_eq!(snapshot.workspaces["web"].context_bytes_avoided, 4_096);
    assert_eq!(snapshot.workspaces["api"].completed, 0);
    monitor.record_agent_context_metrics(
        "web",
        AgentContextMetrics {
            model_bytes: 800,
            context_bytes_avoided: 3_200,
            repo_map_cache_hit: true,
            repo_map_candidates: 40,
            repo_map_delivered: 8,
            build_ms: 120,
        },
    );
    let snapshot = monitor.snapshot();
    assert_eq!(snapshot.workspaces["web"].agent_context_calls, 1);
    assert_eq!(snapshot.workspaces["web"].agent_context_model_bytes, 800);
    assert_eq!(
        snapshot.workspaces["web"].agent_context_bytes_avoided,
        3_200
    );
    assert_eq!(snapshot.workspaces["web"].agent_repo_map_cache_hits, 1);
    assert_eq!(snapshot.workspaces["web"].agent_repo_map_candidates, 40);
    assert_eq!(snapshot.workspaces["web"].agent_repo_map_delivered, 8);
    assert_eq!(snapshot.workspaces["web"].agent_context_build_ms, 120);
    let activity = monitor.observatory_activity("web");
    assert_eq!(activity["agent_context"]["repo_map_candidates"], 40);
    assert_eq!(activity["agent_context"]["repo_map_delivered"], 8);
    assert_eq!(activity["agent_context"]["build_ms"], 120);
    assert_eq!(totals(&snapshot).calls, 1);
    assert_eq!(estimated_tokens(totals(&snapshot).response_bytes), 128);
    assert_eq!(
        estimated_tokens(totals(&snapshot).context_bytes_avoided),
        1_024
    );
    assert!((estimated_cost_usd(512, 5.0) - 0.00064).abs() < f64::EPSILON);
    assert!((estimated_cost_usd(4_096, 5.0) - 0.00512).abs() < f64::EPSILON);
}

#[test]
fn scope_status_updates_operator_intelligence_state() {
    let monitor = TaskMonitor::new(["api".to_owned()]);
    monitor.record_intelligence_result(
        "api",
        "scope_status",
        &serde_json::json!({"source_files": 12, "mapped_files": 10, "unmapped_files": ["src/a.rs", "src/b.rs"]}),
    );
    let snapshot = monitor.snapshot();
    let stats = &snapshot.intelligence["api"];
    assert_eq!(stats.scope_source_files, 12);
    assert_eq!(stats.scope_mapped_files, 10);
    assert_eq!(stats.scope_unmapped_files, 2);
    assert!(stats.updated_at.is_some());
}

#[test]
fn semantic_provider_status_distinguishes_available_ready_and_live() {
    let monitor = TaskMonitor::new(["api".to_owned()]);
    monitor.record_intelligence_result(
        "api",
        "semantic_provider_status",
        &serde_json::json!([
            {"detected":true,"available":true,"launch_ready":true,"session_validated":true,"automatic":true,"runnable":true,"action":null},
            {"detected":true,"available":true,"launch_ready":false,"session_validated":false,"automatic":false,"runnable":false,"action":"authorize_lsp"},
            {"detected":true,"available":false,"launch_ready":false,"session_validated":false,"automatic":false,"runnable":false,"action":"install_lsp"}
        ]),
    );
    let snapshot = monitor.snapshot();
    let stats = &snapshot.intelligence["api"];
    assert_eq!(stats.lsp_available, 2);
    assert_eq!(stats.lsp_launch_ready, 1);
    assert_eq!(stats.lsp_validated, 1);
    assert_eq!(stats.lsp_automatic, 1);
    assert_eq!(stats.lsp_runnable, 1);
    assert_eq!(stats.lsp_authorization_required, 1);
    assert_eq!(stats.lsp_missing, 1);
}

#[test]
fn semantic_session_status_updates_operator_intelligence_state() {
    let monitor = TaskMonitor::new(["api".to_owned()]);
    monitor.record_intelligence_result(
        "api",
        "semantic_session_status",
        &serde_json::json!({"sessions": 1, "documents": 7, "starts": 2, "requests": 41}),
    );
    let snapshot = monitor.snapshot();
    let stats = &snapshot.intelligence["api"];
    assert_eq!(stats.lsp_sessions, 1);
    assert_eq!(stats.lsp_documents, 7);
    assert_eq!(stats.lsp_starts, 2);
    assert_eq!(stats.lsp_requests, 41);
}

#[test]
fn opening_intelligence_loads_the_focused_workspace_without_prior_mcp_calls() {
    let (workspace_root, workspaces) = monitor_test_workspaces(&["backend"]);
    let source = workspace_root.path().join("backend/src");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::write(source.join("lib.rs"), "pub fn answer() -> u32 { 42 }\n").unwrap();
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    let harness = ToolHarness::new(4).unwrap();

    refresh_intelligence_now(&monitor, &harness, &workspaces, "backend")
        .expect("focused workspace intelligence refresh");

    let snapshot = monitor.snapshot();
    let stats = &snapshot.intelligence["backend"];
    assert_eq!(stats.design_state.as_deref(), Some("uninitialized"));
    assert_eq!(stats.scope_source_files, 1);
    assert!(stats.graph_nodes > 0);
    assert!(stats.updated_at.is_some());
}

#[test]
fn orchestration_tasks_are_visible_without_consuming_execution_slots() {
    let monitor = TaskMonitor::new(["api".to_owned()]);
    let mut ticket =
        monitor.queue_orchestration("api", "verification_plan", "orchestrate child checks", 64);

    ticket.start();
    let running = monitor.snapshot();
    assert_eq!(running.tasks.len(), 1);
    assert_eq!(running.tasks[0].status, TaskStatus::Running);
    assert_eq!(running.workspaces["api"].active, 0);
    assert_eq!(running.peak_active, 0);

    ticket.finish(true, 128);
    let completed = monitor.snapshot();
    assert_eq!(completed.tasks[0].status, TaskStatus::Completed);
    assert_eq!(completed.workspaces["api"].completed, 1);
    assert_eq!(completed.workspaces["api"].active, 0);
}

#[test]
fn tracks_current_slots_and_peak_parallelism() {
    let monitor = TaskMonitor::new(["api".to_owned(), "web".to_owned()]);
    let mut first = monitor.queue("api", "read_file", "one", 1);
    let mut second = monitor.queue("web", "read_file", "two", 1);

    first.start();
    second.start();
    let running = monitor.connection_status();
    assert_eq!(running.active_tasks, 2);
    assert_eq!(running.queued_tasks, 0);
    assert_eq!(running.peak_active_tasks, 2);
    assert_eq!(monitor.snapshot().peak_active, 2);

    first.finish(true, 1);
    let draining = monitor.connection_status();
    assert_eq!(draining.active_tasks, 1);
    assert_eq!(draining.peak_active_tasks, 2);

    second.finish(true, 1);
    let idle = monitor.connection_status();
    assert_eq!(idle.active_tasks, 0);
    assert_eq!(idle.peak_active_tasks, 2);
}

#[test]
fn snapshots_preserve_short_lived_activity_between_draws() {
    let monitor = TaskMonitor::new(["api".to_owned()]);
    let _ = monitor.snapshot();
    let mut ticket = monitor.queue("api", "read_file", "short task", 1);
    std::thread::sleep(Duration::from_millis(6));
    ticket.start();
    ticket.finish(true, 1);

    let observed = monitor.snapshot();
    assert_eq!(totals(&observed).active, 0);
    assert_eq!(totals(&observed).queued, 0);
    assert_eq!(observed.observed_active, 1);
    assert_eq!(observed.observed_queued, 1);

    let next = monitor.snapshot();
    assert_eq!(next.observed_active, 0);
    assert_eq!(next.observed_queued, 0);
}

#[test]
fn dropped_ticket_is_failed() {
    let monitor = TaskMonitor::new(["api".to_owned()]);
    let mut ticket = monitor.queue("api", "search_code", ". · query 8 chars", 64);
    ticket.start();
    drop(ticket);
    let snapshot = monitor.snapshot();
    assert_eq!(snapshot.workspaces["api"].active, 0);
    assert_eq!(snapshot.workspaces["api"].failed, 1);
}

#[test]
fn queued_ticket_can_fail_before_start() {
    let monitor = TaskMonitor::new(["api".to_owned()]);
    let ticket = monitor.queue("api", "run_command", "cargo test · cwd .", 16);
    ticket.finish(false, 32);
    let snapshot = monitor.snapshot();
    assert_eq!(snapshot.workspaces["api"].queued, 0);
    assert_eq!(snapshot.workspaces["api"].failed, 1);
    assert_eq!(snapshot.workspaces["api"].response_bytes, 32);
}

#[test]
fn spinner_advances_and_wraps() {
    assert_ne!(spinner_frame(0), spinner_frame(1));
    assert_eq!(spinner_frame(0), spinner_frame(SPINNER_FRAMES.len()));
}

#[test]
fn formatters_are_compact() {
    assert_eq!(short_duration(Duration::from_millis(382)), "382ms");
    assert_eq!(short_duration(Duration::from_millis(1200)), "1.2s");
    assert_eq!(short_bytes(8_806), "8.6K");
    assert_eq!(short_usd(0.0), "$0");
    assert_eq!(short_usd(0.000005), "$0.000005");
    assert_eq!(short_usd(0.00512), "$0.005120");
    assert_eq!(short_usd(2.1), "$2.10");
}

#[test]
fn dashboard_refreshes_less_often_while_idle() {
    let monitor = TaskMonitor::new(["api".to_owned()]);
    assert_eq!(
        dashboard_refresh_interval(&monitor.snapshot()),
        IDLE_REFRESH_INTERVAL
    );
    let ticket = monitor.queue("api", "read_file", "src/lib.rs", 32);
    assert_eq!(
        dashboard_refresh_interval(&monitor.snapshot()),
        ACTIVE_REFRESH_INTERVAL
    );
    drop(ticket);
    assert_eq!(
        dashboard_refresh_interval(&monitor.snapshot()),
        IDLE_REFRESH_INTERVAL
    );
}

#[test]
fn traffic_history_is_bounded_under_bursts() {
    let monitor = TaskMonitor::new(["api".to_owned()]);
    let now = Instant::now();
    let mut state = monitor.state.lock().unwrap();
    for _ in 0..MAX_TRAFFIC_EVENTS + 17 {
        state.traffic.push_back(TrafficEvent {
            at: now,
            requests: 1,
            request_bytes: 1,
            response_bytes: 1,
            context_bytes_avoided: 1,
        });
    }
    trim_history(&mut state, now);
    assert_eq!(state.traffic.len(), MAX_TRAFFIC_EVENTS);
}

#[test]
fn connection_tracks_public_endpoint_and_tunnel_failure() {
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    monitor.mark_public_endpoint("quick-tunnel", Some(true));
    let ready = monitor.connection_status();
    assert_eq!(ready.public_endpoint.as_deref(), Some("quick-tunnel"));
    assert_eq!(ready.tunnel_running, Some(true));
    assert!(ready.tunnel_error.is_none());

    monitor.mark_tunnel_stopped("cloudflared exited");
    let stopped = monitor.connection_status();
    assert_eq!(stopped.tunnel_running, Some(false));
    assert_eq!(stopped.tunnel_error.as_deref(), Some("cloudflared exited"));
}

#[test]
fn dashboard_terminal_claim_suppresses_background_operator_output() {
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    monitor.claim_terminal();
    assert!(monitor.terminal_claimed());
    monitor.operator_message(
        OperatorMessageKind::Warning,
        "tunnel",
        "cloudflare respawns in 15s",
    );
    let snapshot = monitor.snapshot();
    assert_eq!(
        snapshot.tunnel_activity.as_deref(),
        Some("cloudflare respawns in 15s")
    );
    monitor.release_terminal();
    assert!(!monitor.terminal_claimed());
}

#[test]
fn mouse_hit_testing_opens_footer_and_help_links() {
    let (_workspace_root, workspaces) = monitor_test_workspaces(&["backend"]);
    let config = MonitorConfig {
        version: "0.1.0".to_owned(),
        instance_id: "instance-one".to_owned(),
        local_health_url: "http://127.0.0.1:8765/healthz".to_owned(),
        public_url: Arc::new(std::sync::RwLock::new(
            "https://example.trycloudflare.com".to_owned(),
        )),
        intelligence_url: "http://127.0.0.1:8765/intelligence#token=test".to_owned(),
        project_url: "https://github.com/francis-du/wcode".to_owned(),
        author_url: "https://github.com/francis-du".to_owned(),
        author_handle: "@francis-du".to_owned(),
        pairing_code: "123456".to_owned(),
        max_parallel: 16,
        input_token_price_per_million_usd: 5.0,
        semantic_auto: true,
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
    };
    let project_hit = (0..140).any(|column| {
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row: 30,
            modifiers: KeyModifiers::NONE,
        };
        dashboard_link_at(&click, 140, 32, &DashboardState::default(), &config)
            == Some(config.project_url.clone())
    });
    assert!(project_hit, "wide footer must expose the project link");
    let setup_hit = (0..140).any(|column| {
        let click = MouseEvent {
            kind: MouseEventKind::Down(MouseButton::Left),
            column,
            row: 31,
            modifiers: KeyModifiers::NONE,
        };
        dashboard_link_at(&click, 140, 32, &DashboardState::default(), &config)
            == Some(config.setup_url())
    });
    assert!(
        setup_hit,
        "wide footer must expose a clickable setup region"
    );

    let help = DashboardState {
        help_open: true,
        ..DashboardState::default()
    };
    let help_has_link = |target: &str| {
        (0..18).any(|row| {
            (0..70).any(|column| {
                let click = MouseEvent {
                    kind: MouseEventKind::Down(MouseButton::Left),
                    column,
                    row,
                    modifiers: KeyModifiers::NONE,
                };
                dashboard_link_at(&click, 70, 18, &help, &config).as_deref() == Some(target)
            })
        })
    };
    assert!(help_has_link(&config.project_url));
    assert!(help_has_link(&config.local_health_url));
}

#[test]
fn narrow_and_tiny_layouts_do_not_panic() {
    let monitor = TaskMonitor::new(["backend".to_owned(), "frontend".to_owned()]);
    let (_workspace_root, workspaces) = monitor_test_workspaces(&["backend", "frontend"]);
    let config = MonitorConfig {
        version: "0.1.0".to_owned(),
        instance_id: "instance-one".to_owned(),
        local_health_url: "http://127.0.0.1:8765/healthz".to_owned(),
        public_url: Arc::new(std::sync::RwLock::new(
            "https://example.trycloudflare.com".to_owned(),
        )),
        intelligence_url: "http://127.0.0.1:8765/intelligence#token=test".to_owned(),
        project_url: "https://github.com/francis-du/wcode".to_owned(),
        author_url: "https://github.com/francis-du".to_owned(),
        author_handle: "@francis-du".to_owned(),
        pairing_code: "123456".to_owned(),
        max_parallel: 16,
        input_token_price_per_million_usd: 5.0,
        semantic_auto: true,
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
    };

    for (width, height) in [(20, 5), (40, 10), (60, 18), (100, 32)] {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        let snapshot = monitor.snapshot();
        let ui = DashboardState::default();
        terminal
            .draw(|frame| draw_dashboard(frame, &snapshot, &config, 0, &ui))
            .expect("layout renders");
    }
}

#[test]
fn help_and_footer_render_project_and_author_links() {
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    let (_workspace_root, workspaces) = monitor_test_workspaces(&["backend"]);
    monitor.mark_mcp_initialized();
    let mut saved = monitor.queue("backend", "symbol_context", "saved context", 1);
    saved.start();
    saved.finish_with_context_savings(true, 400, 4_000);
    monitor.record_agent_context_metrics(
        "backend",
        AgentContextMetrics {
            model_bytes: 800,
            context_bytes_avoided: 3_200,
            repo_map_cache_hit: true,
            repo_map_candidates: 40,
            repo_map_delivered: 8,
            build_ms: 120,
        },
    );
    let mut first = monitor.queue("backend", "read_file", "one", 1);
    let mut second = monitor.queue("backend", "search_code", "two", 1);
    first.start();
    second.start();
    let config = MonitorConfig {
        version: "0.1.0".to_owned(),
        instance_id: "instance-one".to_owned(),
        local_health_url: "http://127.0.0.1:8765/healthz".to_owned(),
        public_url: Arc::new(std::sync::RwLock::new(
            "https://example.trycloudflare.com".to_owned(),
        )),
        intelligence_url: "https://example.trycloudflare.com/intelligence#token=fixture".to_owned(),
        project_url: "https://github.com/francis-du/wcode".to_owned(),
        author_url: "https://github.com/francis-du".to_owned(),
        author_handle: "@francis-du".to_owned(),
        pairing_code: "123456".to_owned(),
        max_parallel: 8,
        input_token_price_per_million_usd: 5.0,
        semantic_auto: true,
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
    };

    let backend = TestBackend::new(140, 32);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            draw_dashboard(
                frame,
                &monitor.snapshot(),
                &config,
                0,
                &DashboardState::default(),
            )
        })
        .expect("wide footer renders");
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("github.com/francis-du/wcode"));
    assert!(text.contains("@francis-du"));
    assert!(text.contains("SLOTS 2 / 8"));
    assert!(text.contains("PEAK 2"));
    assert!(text.contains("VERIFY CODE 123456"));
    assert!(text.contains("INSTANCE"));
    assert!(text.contains("127.0.0.1:8765"));
    assert!(!text.contains("OVERVIEW"));
    assert!(text.contains("WORKSPACE ACTIVITY"));
    assert!(text.contains("THROUGHPUT"));
    assert!(text.contains("CTX 8/40"));
    assert!(text.contains("HIT 1/1"));
    assert!(text.contains("AVG 120ms"));
    assert!(text.contains("SAVED ~1.0K"));
    assert!(text.contains('╭'));
    assert!(text.contains('╰'));

    let backend = TestBackend::new(70, 18);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let ui = DashboardState {
        help_open: true,
        ..DashboardState::default()
    };
    terminal
        .draw(|frame| draw_dashboard(frame, &monitor.snapshot(), &config, 0, &ui))
        .expect("compact help renders");
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("Project:"));
    assert!(text.contains("Author:"));
    assert!(text.contains("Setup:"));
    assert!(text.contains("Health:"));
    assert!(text.contains("127.0.0.1:8765/healthz"));
}

#[test]
fn authorization_overlay_shows_selectable_requests_and_actions() {
    let requests = vec![
        AuthorizationRequest {
            id: "AUTH-00000002".to_owned(),
            workspace: "backend".to_owned(),
            kind: crate::authorization::AuthorizationKind::CommandAccess,
            summary: "authorize command: git".to_owned(),
            program: Some("git".to_owned()),
            fingerprint: "sha256:git".to_owned(),
            status: AuthorizationStatus::Pending,
            created_at_ms: 2,
            decided_at_ms: None,
        },
        AuthorizationRequest {
            id: "AUTH-00000001".to_owned(),
            workspace: "backend".to_owned(),
            kind: crate::authorization::AuthorizationKind::DestructiveDelete,
            summary: "delete file: src/obsolete.rs".to_owned(),
            program: None,
            fingerprint: "sha256:delete".to_owned(),
            status: AuthorizationStatus::Pending,
            created_at_ms: 1,
            decided_at_ms: None,
        },
    ];
    let backend = TestBackend::new(100, 24);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            render_authorization_overlay(frame, frame.area(), &requests, 1, 0, UiLanguage::En)
        })
        .expect("authorization overlay renders");
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("AUTHORIZATION REQUIRED"));
    assert!(text.contains("AUTH-00000002"));
    assert!(text.contains("AUTH-00000001"));
    assert!(text.contains("Y"));
    assert!(text.contains("approve selected"));
    assert!(text.contains("N"));
    assert!(text.contains("deny selected"));
}

#[test]
fn command_overlay_shows_the_complete_catalog_and_two_authorization_layers() {
    let (_workspace_root, workspaces) = monitor_test_workspaces(&["backend"]);
    workspaces
        .revoke_command(Some("backend"), "cargo")
        .expect("revoke cargo for the locked-state fixture");
    let area = Rect::new(0, 0, 120, 30);
    let page_size = command_page_size(area).max(1);
    let command_count = command_count(&workspaces, "backend");
    let mut text = String::new();
    for offset in (0..command_count.max(1)).step_by(page_size) {
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).expect("test terminal");
        terminal
            .draw(|frame| {
                render_commands_overlay(
                    frame,
                    frame.area(),
                    &workspaces,
                    "backend",
                    offset,
                    UiLanguage::En,
                )
            })
            .expect("command overlay renders");
        text.extend(
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol()),
        );
    }

    for program in crate::workspace::COMMAND_CATALOG {
        assert!(text.contains(program), "missing command {program}");
    }
    assert!(text.contains("SUPPORTED COMMANDS"));
    assert!(text.contains("Executable access"));
    assert!(text.contains("Exact repository operation"));
    assert!(text.contains("approval required"));
}

#[test]
fn intelligence_url_keeps_ui_token_and_targets_focused_workspace() {
    let url = intelligence_url_for_workspace(
        "http://127.0.0.1:8765/intelligence#token=secret",
        "frontend app",
    );
    assert_eq!(
        url,
        "http://127.0.0.1:8765/intelligence#token=secret&workspace=frontend+app"
    );
}

#[test]
fn workspace_columns_scale_from_one_to_four() {
    assert_eq!(workspace_column_count(45, 6), 1);
    assert_eq!(workspace_column_count(70, 6), 2);
    assert_eq!(workspace_column_count(130, 6), 4);
    assert_eq!(workspace_column_count(200, 1), 1);
}

#[test]
fn workspace_viewport_follows_focus_and_pages() {
    let mut ui = DashboardState {
        workspace_focus: 4,
        ..DashboardState::default()
    };
    ui.clamp(7, 3);
    assert_eq!(ui.workspace_offset, 2);
    ui.workspace_focus = 6;
    ui.clamp(7, 3);
    assert_eq!(ui.workspace_offset, 4);
    ui.workspace_focus = 1;
    ui.clamp(7, 3);
    assert_eq!(ui.workspace_offset, 1);
}

#[test]
fn connection_stages_and_setup_collapse_render() {
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    let (_workspace_root, workspaces) = monitor_test_workspaces(&["backend"]);
    let config = MonitorConfig {
        version: "0.1.0".to_owned(),
        instance_id: "instance-one".to_owned(),
        local_health_url: "http://127.0.0.1:8765/healthz".to_owned(),
        public_url: Arc::new(std::sync::RwLock::new(
            "https://example.trycloudflare.com".to_owned(),
        )),
        intelligence_url: "https://example.trycloudflare.com/intelligence#token=fixture".to_owned(),
        project_url: "https://github.com/francis-du/wcode".to_owned(),
        author_url: "https://github.com/francis-du".to_owned(),
        author_handle: "@francis-du".to_owned(),
        pairing_code: "123456".to_owned(),
        max_parallel: 8,
        input_token_price_per_million_usd: 5.0,
        semantic_auto: true,
        workspaces,
        harness: ToolHarness::new(4).unwrap(),
    };
    let backend = TestBackend::new(100, 32);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    let ui = DashboardState::default();
    terminal
        .draw(|frame| draw_dashboard(frame, &monitor.snapshot(), &config, 0, &ui))
        .expect("expanded setup renders");

    monitor.mark_oauth_client_registered();
    monitor.mark_oauth_authorized();
    monitor.mark_mcp_seen();
    monitor.mark_mcp_initialized();
    monitor.mark_mcp_initialized();
    let status = monitor.connection_status();
    assert!(status.oauth_client_registered);
    assert!(status.oauth_authorized);
    assert!(status.chatgpt_initialized);
    assert_eq!(status.initialize_count, 2);
    assert!(status.last_initialize_seconds_ago.is_some());
    assert!(status.last_mcp_seen_seconds_ago.is_some());
    terminal
        .draw(|frame| draw_dashboard(frame, &monitor.snapshot(), &config, 1, &ui))
        .expect("collapsed setup renders");
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(
        text.contains("VERIFY CODE 123456"),
        "the pairing code must remain visible after OAuth and MCP connect"
    );
}
