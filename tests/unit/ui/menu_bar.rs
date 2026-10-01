use super::*;
use crate::runtime_presence::{RuntimePresenceRecord, RuntimePresenceSnapshot, RuntimeTransport};

fn record(
    transport: RuntimeTransport,
    connected: bool,
    active: u64,
    queued: u64,
) -> RuntimePresenceRecord {
    RuntimePresenceRecord {
        schema_version: 1,
        instance_id: match transport {
            RuntimeTransport::Http => "http".into(),
            RuntimeTransport::Stdio => "stdio".into(),
        },
        version: "0.9.0".into(),
        transport,
        started_at_ms: 1,
        updated_at_ms: 2,
        workspaces: vec!["project".into()],
        local_url: (transport == RuntimeTransport::Http).then(|| "http://127.0.0.1:8765".into()),
        mcp_connected: connected,
        initialize_count: u64::from(connected),
        last_mcp_seen_seconds_ago: connected.then_some(0),
        oauth_authorized: false,
        public_endpoint: None,
        public_url_healthy: None,
        tunnel_running: None,
        active_tasks: active,
        queued_tasks: queued,
        active_verifications: Some(0),
        queued_verifications: Some(0),
        active_jobs: Some(0),
        queued_jobs: Some(0),
    }
}

#[test]
fn menu_bar_summary_combines_http_stdio_mcp_and_tasks_without_inventing_success() {
    let mut http = record(RuntimeTransport::Http, true, 2, 1);
    http.active_verifications = Some(1);
    http.queued_verifications = Some(1);
    http.active_jobs = Some(1);
    let mut stdio = record(RuntimeTransport::Stdio, false, 3, 4);
    stdio.active_verifications = Some(2);
    stdio.queued_jobs = Some(3);
    let snapshot = RuntimePresenceSnapshot {
        schema_version: 1,
        records: vec![http, stdio],
        partial: false,
        invalid_records: 0,
        stale_records: 0,
        scan_truncated: false,
    };
    let summary = MenuBarSummary::from_snapshot(&snapshot);
    assert_eq!(summary.runtime_count, 2);
    assert_eq!(summary.http_runtimes, 1);
    assert_eq!(summary.stdio_runtimes, 1);
    assert_eq!(summary.mcp_connected, 1);
    assert_eq!(summary.active_tasks, 5);
    assert_eq!(summary.queued_tasks, 5);
    assert_eq!(summary.active_verifications, Some(3));
    assert_eq!(summary.queued_verifications, Some(1));
    assert_eq!(summary.active_jobs, Some(1));
    assert_eq!(summary.queued_jobs, Some(3));
    assert_eq!(summary.state, "working");
}

#[test]
fn menu_bar_summary_keeps_missing_or_partial_subactivity_unknown() {
    let mut legacy = record(RuntimeTransport::Http, true, 0, 0);
    legacy.active_verifications = None;
    legacy.queued_verifications = None;
    legacy.active_jobs = None;
    legacy.queued_jobs = None;
    let legacy = MenuBarSummary::from_snapshot(&RuntimePresenceSnapshot {
        schema_version: 1,
        records: vec![legacy],
        ..Default::default()
    });
    assert_eq!(legacy.active_verifications, None);
    assert_eq!(legacy.queued_verifications, None);
    assert_eq!(legacy.active_jobs, None);
    assert_eq!(legacy.queued_jobs, None);

    let partial = MenuBarSummary::from_snapshot(&RuntimePresenceSnapshot {
        schema_version: 1,
        records: vec![record(RuntimeTransport::Stdio, false, 0, 0)],
        partial: true,
        invalid_records: 1,
        ..Default::default()
    });
    assert_eq!(partial.active_verifications, None);
    assert_eq!(partial.queued_verifications, None);
    assert_eq!(partial.active_jobs, None);
    assert_eq!(partial.queued_jobs, None);
}

#[test]
fn menu_bar_summary_keeps_offline_partial_and_unknown_distinct() {
    let offline = MenuBarSummary::from_snapshot(&RuntimePresenceSnapshot {
        schema_version: 1,
        ..Default::default()
    });
    assert_eq!(offline.state, "offline");

    let unknown = MenuBarSummary::from_snapshot(&RuntimePresenceSnapshot {
        schema_version: 1,
        partial: true,
        invalid_records: 1,
        ..Default::default()
    });
    assert_eq!(unknown.state, "unknown");

    let partial = MenuBarSummary::from_snapshot(&RuntimePresenceSnapshot {
        schema_version: 1,
        records: vec![record(RuntimeTransport::Http, true, 0, 0)],
        partial: true,
        invalid_records: 1,
        ..Default::default()
    });
    assert_eq!(partial.state, "partial");
}
