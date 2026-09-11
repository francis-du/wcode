use super::*;

#[test]
fn process_queue_labels_separate_occupancy_from_waiters() {
    let mut resources = crate::resource::snapshot();
    resources.child_queue = crate::resource::ProcessQueueSnapshot {
        limit: 2,
        active: 2,
        waiting: 3,
        ..Default::default()
    };
    resources.probe_queue = crate::resource::ProcessQueueSnapshot {
        limit: 4,
        active: 1,
        waiting: 2,
        ..Default::default()
    };
    assert_eq!(process_queue_text(&resources), "PROC 2/2 · GIT 1/4 · Q 5");
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
