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
fn wide_dashboard_puts_activity_and_engineering_control_rail_side_by_side() {
    let (_root, workspaces) = monitor_test_workspaces(&["backend"]);
    let config = monitor_test_config(workspaces);
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    monitor.mark_mcp_initialized();
    seed_engineering_state(&monitor, "backend");
    let task = monitor.queue("backend", "read_file", "src/lib.rs", 1);
    task.start();

    let text = monitor_test_text(&monitor, &config, 120, 34, &DashboardState::default());
    let row = text
        .lines()
        .find(|line| line.contains("WORKSPACE ACTIVITY"))
        .expect("wide dashboard must render the activity title");
    assert!(
        row.contains("ENGINEERING PULSE"),
        "engineering pulse should live in the right control rail, not above activity: {row}"
    );
    assert!(text.contains("read_file"));
}

#[test]
fn engineering_pulse_is_default_on_roomy_terminals_without_crowding_small_ones() {
    let (_root, workspaces) = monitor_test_workspaces(&["backend"]);
    let config = monitor_test_config(workspaces);
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    monitor.mark_mcp_initialized();
    seed_engineering_state(&monitor, "backend");
    let task = monitor.queue("backend", "read_file", "src/lib.rs", 1);
    task.start();

    let roomy = monitor_test_text(&monitor, &config, 120, 34, &DashboardState::default());
    assert!(roomy.contains("ENGINEERING PULSE"));
    assert!(roomy.contains('╭'));
    assert!(roomy.contains('╰'));
    assert!(roomy.contains("ARCH"));
    assert!(roomy.contains("DRIFT"));
    assert!(roomy.contains("PROOF"));
    assert!(roomy.contains("MODEL"));
    assert!(roomy.contains("policy 1/2"));
    assert!(roomy.contains("read_file"));

    let medium = monitor_test_text(&monitor, &config, 100, 32, &DashboardState::default());
    assert!(!medium.contains("ENGINEERING PULSE"));
    assert!(medium.contains("read_file"));

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
fn agent_context_metrics_preserve_latest_jev_runtime_telemetry() {
    let monitor = TaskMonitor::new(["web".to_owned()]);
    monitor.record_agent_context_decision(
        "web",
        &serde_json::json!({
            "provider": "jev",
            "status": "active",
            "model": "jev-latest",
            "authority": "increase_only_assist",
            "question_set": {"id": "wcode.agent_context", "version": 4},
            "baseline_next_action": "edit_then_verify",
            "candidate_next_action": "semantic_navigation",
            "guidance": ["jev:prefer_semantic_navigation"],
            "comparison": {
                "shared_signals": 7,
                "choice_disagreements": 1,
                "safety_policy_violations": 0,
                "shape_mismatches": 0
            },
            "call": {
                "request_bytes": 4096,
                "response_bytes": 1024,
                "elapsed_ms": 240,
                "tokens": {
                    "input": 900,
                    "output": 180,
                    "total": 1080,
                    "source": "provider_reported"
                }
            },
            "api_key": "PRIVATE-JEV-KEY",
            "raw_response": "PRIVATE-JEV-RESPONSE"
        }),
    );

    let snapshot = monitor.snapshot();
    let stats = &snapshot.workspaces["web"];
    assert_eq!(stats.jev_observed, 1);
    assert_eq!(stats.jev_successful, 1);
    assert_eq!(stats.jev_degraded, 0);
    assert_eq!(stats.jev_checkpoints["agent_context"], 1);
    let latest = stats.jev_latest.as_ref().unwrap();
    assert_eq!(latest.checkpoint, "agent_context");
    assert_eq!(latest.status, "active");
    assert_eq!(latest.model.as_deref(), Some("jev-latest"));
    assert_eq!(latest.choice_disagreements, 1);

    let activity = monitor.observatory_activity("web");
    let jev = &activity["agent_context"]["decision_runtime"]["jev"];
    assert_eq!(jev["status"], "active");
    assert_eq!(jev["model"], "jev-latest");
    assert_eq!(jev["question_set"]["id"], "wcode.agent_context");
    assert_eq!(jev["question_set"]["version"], 4);
    assert_eq!(jev["checkpoint"], "agent_context");
    assert_eq!(jev["calls"]["by_checkpoint"]["agent_context"], 1);
    assert_eq!(jev["baseline_next_action"], "edit_then_verify");
    assert_eq!(jev["candidate_next_action"], "semantic_navigation");
    assert_eq!(jev["comparison"]["shared_signals"], 7);
    assert_eq!(jev["calls"]["observed"], 1);
    assert_eq!(jev["calls"]["successful"], 1);
    assert_eq!(jev["call"]["request_bytes"], 4096);
    assert_eq!(jev["call"]["response_bytes"], 1024);
    assert_eq!(jev["call"]["elapsed_ms"], 240);
    assert_eq!(jev["call"]["tokens"]["total"], 1080);
    assert_eq!(jev["call"]["tokens"]["source"], "provider_reported");
    assert_eq!(jev["calls"]["metered"], 1);
    assert_eq!(jev["calls"]["request_bytes"], 4096);
    assert_eq!(jev["calls"]["response_bytes"], 1024);
    assert_eq!(jev["calls"]["avg_elapsed_ms"], 240);
    assert_eq!(jev["calls"]["tokens"]["input"], 900);
    assert_eq!(jev["calls"]["tokens"]["output"], 180);
    assert_eq!(jev["calls"]["tokens"]["total"], 1080);
    assert_eq!(jev["calls"]["tokens"]["source"], "provider_reported");
    assert!(jev["observed_ago_ms"].as_u64().is_some());
    let serialized = activity.to_string();
    assert!(!serialized.contains("PRIVATE-JEV-KEY"));
    assert!(!serialized.contains("PRIVATE-JEV-RESPONSE"));

    monitor.record_jev_decision(
        "web",
        "post_edit_review",
        &serde_json::json!({
            "provider": "jev",
            "status": "active",
            "model": "jev-latest",
            "authority": "increase_only_assist",
            "question_set": {"id": "wcode.runtime_checkpoint", "version": 1},
            "baseline_next_action": "verify_project",
            "candidate_next_action": "verify_project",
            "guidance": [],
            "comparison": {
                "shared_signals": 4,
                "choice_disagreements": 0,
                "safety_policy_violations": 0,
                "shape_mismatches": 0
            },
            "call": {
                "request_bytes": 800,
                "response_bytes": 200,
                "elapsed_ms": 160,
                "tokens": {
                    "input": null,
                    "output": null,
                    "total": null,
                    "source": "unavailable"
                }
            }
        }),
    );
    let mixed = monitor.observatory_activity("web");
    let mixed_jev = &mixed["agent_context"]["decision_runtime"]["jev"];
    assert_eq!(mixed_jev["checkpoint"], "post_edit_review");
    assert_eq!(mixed_jev["calls"]["by_checkpoint"]["agent_context"], 1);
    assert_eq!(mixed_jev["calls"]["by_checkpoint"]["post_edit_review"], 1);
    assert_eq!(mixed_jev["call"]["tokens"]["source"], "byte_estimate");
    assert_eq!(mixed_jev["call"]["tokens"]["input"], 200);
    assert_eq!(mixed_jev["call"]["tokens"]["output"], 50);
    assert_eq!(
        mixed_jev["calls"]["tokens"]["source"],
        "mixed_provider_and_byte_estimate"
    );
    assert_eq!(mixed_jev["calls"]["tokens"]["input"], 1224);
    assert_eq!(mixed_jev["calls"]["tokens"]["output"], 306);

    monitor.record_agent_context_decision(
        "web",
        &serde_json::json!({
            "provider": "jev",
            "status": "unavailable",
            "model": "jev-latest",
            "authority": "increase_only_assist",
            "question_set": {"id": "wcode.agent_context", "version": 4},
            "baseline_next_action": "retrieve"
        }),
    );
    let latest = monitor.observatory_activity("web");
    let jev = &latest["agent_context"]["decision_runtime"]["jev"];
    assert_eq!(jev["status"], "unavailable");
    assert_eq!(jev["calls"]["observed"], 3);
    assert_eq!(jev["calls"]["successful"], 2);
    assert_eq!(jev["calls"]["degraded"], 1);
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

#[test]
fn workspace_focus_identity_survives_activity_reordering() {
    let initial = vec![
        ("alpha".to_owned(), "alpha".to_owned(), true),
        ("beta".to_owned(), "beta".to_owned(), true),
        ("gamma".to_owned(), "gamma".to_owned(), true),
    ];
    let mut ui = DashboardState::default();
    ui.sync_workspace_order(&initial, 2);
    ui.set_workspace_focus(&initial, 1, 2);
    assert_eq!(ui.workspace_focus_id.as_deref(), Some("beta"));

    let reordered = vec![
        ("beta".to_owned(), "beta".to_owned(), true),
        ("alpha".to_owned(), "alpha".to_owned(), true),
        ("gamma".to_owned(), "gamma".to_owned(), true),
    ];
    ui.sync_workspace_order(&reordered, 2);

    assert_eq!(ui.workspace_focus, 0);
    assert_eq!(ui.workspace_focus_id.as_deref(), Some("beta"));
}

#[tokio::test]
async fn workspace_activity_order_promotes_running_queued_approval_then_recent() {
    let (_root, workspaces) =
        monitor_test_workspaces(&["idle", "recent", "failed", "approval", "queued", "active"]);
    let config = monitor_test_config(workspaces);
    let monitor = TaskMonitor::new([
        "idle".to_owned(),
        "recent".to_owned(),
        "failed".to_owned(),
        "approval".to_owned(),
        "queued".to_owned(),
        "active".to_owned(),
    ]);

    config
        .workspaces
        .revoke_command(Some("approval"), "cargo")
        .unwrap();
    let (_, approval_workspace) = config.workspaces.select(Some("approval")).unwrap();
    let authorization_error = approval_workspace
        .run_command("cargo", &["test".to_owned()], ".", 30)
        .await
        .unwrap_err();
    assert!(authorization_error
        .to_string()
        .contains("authorization required"));

    let recent = monitor.queue("recent", "read_file", "recent", 1);
    recent.start();
    recent.finish(true, 1);
    let failed = monitor.queue("failed", "read_file", "failed", 1);
    failed.start();
    failed.finish(false, 0);
    let _queued = monitor.queue("queued", "search_code", "queued", 1);
    let active = monitor.queue("active", "agent_context", "active", 1);
    active.start();

    let snapshot = monitor.snapshot();
    let ordered = ordered_workspaces(&config, &snapshot);
    let ids = ordered
        .iter()
        .map(|workspace| workspace.0.as_str())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec!["active", "queued", "approval", "failed", "recent", "idle"]
    );
}
