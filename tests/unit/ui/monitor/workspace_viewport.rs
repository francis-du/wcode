//! Workspace viewport tests: the card window the panel renders must be the one
//! the event loop measures, and the focused card must stay inside it.

use super::*;

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
fn workspace_viewport_matches_the_rendered_panel_across_widths() {
    let names = ["alpha", "beta", "gamma", "delta", "epsilon", "zeta", "eta"];
    let (_root, workspaces) = monitor_test_workspaces(&names);
    let config = monitor_test_config(workspaces);
    let monitor = TaskMonitor::new(names.into_iter().map(str::to_owned));
    for width in [104u16, 110, 120, 132, 144, 160, 200] {
        let snapshot = monitor.snapshot();
        let workspaces = ordered_workspaces(&config, &snapshot);
        let total = workspaces.len();
        let loop_visible = workspace_column_count(workspace_activity_width(width, 34), total);
        let mut ui = DashboardState {
            workspace_focus: total - 1,
            ..DashboardState::default()
        };
        ui.sync_workspace_order(&workspaces, loop_visible);
        let text = monitor_test_text(&monitor, &config, width, 34, &ui);
        let expected = if total <= loop_visible {
            format!("1–{} / {}", total, total)
        } else {
            format!("{}–{} / {}", total - loop_visible + 1, total, total)
        };
        assert!(
            text.contains(&format!("VIEW  {expected}")),
            "width {width} must advertise the page that holds the focus:\n{text}"
        );
    }
}
