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
