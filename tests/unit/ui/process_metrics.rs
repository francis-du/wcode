use super::*;

#[tokio::test]
async fn admission_regression_tui_slots_follow_permits_not_started_task_counters() {
    let (_root, workspaces) = monitor_test_workspaces(&["backend"]);
    let config = monitor_test_config(workspaces);
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    let mut held = Vec::new();
    for _ in 0..3 {
        held.push(config.harness.acquire_tool(false).await.unwrap());
    }
    let text = monitor_test_text(&monitor, &config, 140, 40, &DashboardState::default());
    assert!(
        text.contains("SLOTS 3 / 4"),
        "actual permits must be visible without started tasks: {text}"
    );
    assert!(
        text.contains("RUN 0"),
        "started tasks must remain distinct from reserved slots: {text}"
    );
    drop(held);
}

#[tokio::test]
async fn admission_regression_loading_console_keeps_capacity_visible_in_both_languages() {
    let (_root, workspaces) = monitor_test_workspaces(&["backend"]);
    let config = monitor_test_config(workspaces);
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    assert!(monitor.begin_intelligence_refresh("backend"));
    let held = config.harness.acquire_tool(false).await.unwrap();
    for language in [UiLanguage::En, UiLanguage::ZhCn] {
        let ui = DashboardState {
            intelligence_open: true,
            language,
            ..Default::default()
        };
        for (width, height) in [(80, 24), (140, 40)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| {
                    render_intelligence_overlay(
                        frame,
                        frame.area(),
                        &monitor.snapshot(),
                        &config,
                        &ui,
                    )
                })
                .unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect();
            assert!(text.contains("SLOTS 1/4"), "{text}");
            assert!(text.contains("RUN 0"), "{text}");
            assert!(text.contains("RES 0 · EXEC 0 · SLOT 0"), "{text}");
        }
    }
    drop(held);
    monitor.finish_intelligence_refresh("backend", None);
}

#[test]
fn process_queue_labels_separate_occupancy_from_waiters() {
    let mut resources = crate::resource::snapshot();
    resources.child_queue = crate::resource::ProcessQueueSnapshot {
        limit: 2,
        active: 2,
        waiting: 3,
        max_wait_ms: 41,
        ..Default::default()
    };
    resources.probe_queue = crate::resource::ProcessQueueSnapshot {
        limit: 4,
        active: 1,
        waiting: 2,
        max_wait_ms: 17,
        ..Default::default()
    };
    assert_eq!(
        process_queue_text(&resources),
        "PROC 2/2 · GIT 1/4 · Q 5 · PEAK 41ms"
    );
}

#[test]
fn wide_dashboard_exposes_inner_process_queues_in_both_languages() {
    let (_root, workspaces) = monitor_test_workspaces(&["backend"]);
    let config = monitor_test_config(workspaces);
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    monitor.mark_mcp_initialized();
    for language in [UiLanguage::En, UiLanguage::ZhCn] {
        let ui = DashboardState {
            language,
            ..Default::default()
        };
        let text = monitor_test_text(&monitor, &config, 140, 40, &ui);
        assert!(text.contains("PROC "));
        assert!(text.contains("GIT "));
        assert!(text.contains("Q "));
        assert!(text.contains("123456"));
    }
}
