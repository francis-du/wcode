use super::*;

#[test]
fn narrow_footer_keeps_pairing_approval_help_and_exit_visible() {
    let (_root, workspaces) = monitor_test_workspaces(&["backend"]);
    let config = monitor_test_config(workspaces);
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    monitor.mark_mcp_initialized();

    for width in [40, 60, 77] {
        let text = monitor_test_text(&monitor, &config, width, 20, &DashboardState::default());
        assert!(
            text.contains(&config.pairing_code),
            "pairing code at {width} columns"
        );
        for shortcut in ["Y/N", "?", "^C", " O ", " W "] {
            assert!(
                text.contains(shortcut),
                "missing {shortcut} at {width} columns"
            );
        }
    }
}

fn seed_engineering_state(monitor: &TaskMonitor, workspace: &str) {
    monitor.record_intelligence_result(
        workspace,
        "design_status",
        &serde_json::json!({
            "valid": true,
            "initialized": true,
            "requirements": 12,
            "components": 8
        }),
    );
    monitor.record_intelligence_result(
        workspace,
        "convention_status",
        &serde_json::json!({"errors": 1, "warnings": 2}),
    );
    monitor.record_intelligence_result(
        workspace,
        "traceability_status",
        &serde_json::json!({
            "design_to_implementation": {"percent": 96},
            "acceptance_to_verification": {"percent": 91}
        }),
    );
    monitor.record_intelligence_result(
        workspace,
        "software_graph",
        &serde_json::json!({"node_count": 144, "edge_count": 221, "precision": "syntax"}),
    );
    monitor.record_intelligence_result(
        workspace,
        "risk_status",
        &serde_json::json!({"level": "medium", "drift": {"findings": [{"id": "drift-1"}]}}),
    );
    monitor.record_intelligence_result(
        workspace,
        "evidence_status",
        &serde_json::json!({"total": 18, "failed": 1, "disagreed": 0}),
    );
    monitor.record_intelligence_result(
        workspace,
        "verification_status",
        &serde_json::json!({"ready": false, "blockers": ["rust-test"]}),
    );
    monitor.record_intelligence_result(
        workspace,
        "reconciliation_execution_status",
        &serde_json::json!({"converged": false, "pending": 2}),
    );
}

#[test]
fn engineering_pulse_is_default_on_roomy_terminals_without_crowding_small_ones() {
    let (_root, workspaces) = monitor_test_workspaces(&["backend"]);
    let config = monitor_test_config(workspaces);
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    monitor.mark_mcp_initialized();
    seed_engineering_state(&monitor, "backend");
    let mut task = monitor.queue("backend", "read_file", "src/lib.rs", 1);
    task.start();

    let roomy = monitor_test_text(&monitor, &config, 120, 34, &DashboardState::default());
    assert!(roomy.contains("ENGINEERING PULSE"));
    assert!(roomy.contains('╭'));
    assert!(roomy.contains('╰'));
    assert!(roomy.contains("ARCH"));
    assert!(roomy.contains("PROOF"));
    assert!(roomy.contains("POLICY"));
    assert!(roomy.contains("1 errors · 2 warnings"));
    assert!(roomy.contains("read_file"));

    let compact = monitor_test_text(&monitor, &config, 80, 20, &DashboardState::default());
    assert!(!compact.contains("ENGINEERING PULSE"));
    assert!(compact.contains("read_file"));
}

#[test]
fn compact_help_and_tiny_dashboard_keep_recovery_controls_visible() {
    let (_root, workspaces) = monitor_test_workspaces(&["backend"]);
    let config = monitor_test_config(workspaces);
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    let help = DashboardState {
        help_open: true,
        ..DashboardState::default()
    };
    let help_text = monitor_test_text(&monitor, &config, 70, 18, &help);
    assert!(help_text.contains("L / + / P"));
    assert!(help_text.contains("G Project:"));
    assert!(help_text.contains("Health:"));

    let tiny = monitor_test_text(&monitor, &config, 40, 10, &DashboardState::default());
    assert!(tiny.contains(&config.pairing_code));
    assert!(tiny.contains(" ? "));
    assert!(tiny.contains(" ^C "));
}

#[test]
fn modal_visibility_matches_the_minimum_renderable_terminal() {
    let input = DashboardState {
        workspace_input: Some(String::new()),
        ..DashboardState::default()
    };
    assert!(input.workspace_input_visible(Rect::new(0, 0, 36, 5)));
    assert!(!input.workspace_input_visible(Rect::new(0, 0, 35, 5)));
    assert!(!input.workspace_input_visible(Rect::new(0, 0, 36, 4)));

    let access = DashboardState {
        full_access_confirm: true,
        ..DashboardState::default()
    };
    assert!(access.full_access_visible(Rect::new(0, 0, 48, 12)));
    assert!(!access.full_access_visible(Rect::new(0, 0, 47, 12)));
    assert!(!access.full_access_visible(Rect::new(0, 0, 48, 11)));

    assert!(authorization_overlay_visible(Rect::new(0, 0, 40, 10)));
    assert!(!authorization_overlay_visible(Rect::new(0, 0, 39, 10)));
    assert!(!authorization_overlay_visible(Rect::new(0, 0, 40, 9)));
}

#[test]
fn connected_dashboard_keeps_pairing_code_in_one_persistent_place() {
    let (_root, workspaces) = monitor_test_workspaces(&["backend"]);
    let config = monitor_test_config(workspaces);
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    monitor.mark_mcp_initialized();
    for (width, height) in [(80, 20), (140, 32)] {
        let text = monitor_test_text(&monitor, &config, width, height, &DashboardState::default());
        assert_eq!(
            text.matches(&config.pairing_code).count(),
            1,
            "pairing code duplicated at {width}x{height}"
        );
        assert!(!text.contains("VERIFY CODE"));
    }
}

#[test]
fn long_project_footer_reserves_pairing_code_and_keeps_project_clickable() {
    let (_root, workspaces) = monitor_test_workspaces(&["backend"]);
    let mut config = monitor_test_config(workspaces);
    config.project_url = format!("https://example.test/{}", "very-long-project/".repeat(12));
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    monitor.mark_mcp_initialized();
    let width = 124;
    let height = 24;
    let text = monitor_test_text(&monitor, &config, width, height, &DashboardState::default());
    let project = wide_footer_project_text(&config, UiLanguage::En, width);
    assert!(text.contains(&config.pairing_code));
    assert!(text.contains(&project));
    assert!(Span::raw(&project).width() < Span::raw(&config.project_url).width());
    let click = MouseEvent {
        kind: MouseEventKind::Down(MouseButton::Left),
        column: Span::raw("  wcode  ").width() as u16 + 1,
        row: height - 2,
        modifiers: KeyModifiers::NONE,
    };
    assert_eq!(
        dashboard_link_at(&click, width, height, &DashboardState::default(), &config),
        Some(config.project_url.clone())
    );
}

#[test]
fn command_overlay_keeps_action_feedback_inside_the_surface() {
    let (_root, workspaces) = monitor_test_workspaces(&["backend"]);
    let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
    terminal
        .draw(|frame| {
            render_commands_overlay(
                frame,
                frame.area(),
                &workspaces,
                "backend",
                0,
                Some("authorization updated"),
                UiLanguage::En,
            )
        })
        .unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("STATUS"));
    assert!(text.contains("authorization updated"));
}

#[test]
fn engineering_console_groups_architecture_and_proof_instead_of_flat_intelligence_rows() {
    let (_root, workspaces) = monitor_test_workspaces(&["backend"]);
    let config = monitor_test_config(workspaces);
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    seed_engineering_state(&monitor, "backend");
    let ui = DashboardState {
        intelligence_open: true,
        ..DashboardState::default()
    };

    let text = monitor_test_text(&monitor, &config, 120, 30, &ui);
    assert!(text.contains("ENGINEERING CONSOLE"));
    assert!(text.contains('╭'));
    assert!(text.contains('╰'));
    assert!(text.contains("ARCHITECTURE"));
    assert!(text.contains("PROOF & RUNTIME"));
    assert!(text.contains("DESIGN"));
    assert!(text.contains("EVIDENCE"));
    assert!(text.contains("POLICY"));
    assert!(text.contains("1 errors · 2 warnings"));
    assert!(!text.contains("REPOSITORY INTELLIGENCE"));
}
