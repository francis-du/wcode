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

#[test]
fn menu_bar_summary_large_counts_do_not_overflow_or_appear_idle() {
    let summary = MenuBarSummary::from_snapshot(&RuntimePresenceSnapshot {
        schema_version: 1,
        records: vec![
            record(RuntimeTransport::Http, false, u64::MAX, u64::MAX),
            record(RuntimeTransport::Stdio, false, 1, 1),
        ],
        ..Default::default()
    });
    assert_eq!(summary.active_tasks, u64::MAX);
    assert_eq!(summary.queued_tasks, u64::MAX);
    assert_eq!(summary.state, "working");
}

#[test]
fn menu_bar_summary_reports_verification_and_jobs_as_work() {
    for index in 0..4 {
        let mut runtime = record(RuntimeTransport::Http, true, 0, 0);
        match index {
            0 => runtime.active_verifications = Some(1),
            1 => runtime.queued_verifications = Some(1),
            2 => runtime.active_jobs = Some(1),
            _ => runtime.queued_jobs = Some(1),
        }
        let mut snapshot = RuntimePresenceSnapshot {
            schema_version: 1,
            records: vec![runtime],
            ..Default::default()
        };
        assert_eq!(MenuBarSummary::from_snapshot(&snapshot).state, "working");
        snapshot.partial = true;
        assert_eq!(MenuBarSummary::from_snapshot(&snapshot).state, "partial");
    }
}

#[test]
fn menu_bar_startup_diagnostics_keep_only_a_bounded_tail() {
    let mut bytes = vec![b'x'; 20000];
    bytes.extend_from_slice(b"startup failed");
    let tail = companion_error_tail(std::io::Cursor::new(&bytes)).unwrap();
    assert_eq!(tail.len(), 4096);
    assert_eq!(tail, bytes[bytes.len() - 4096..]);
    assert_eq!(
        companion_error_tail(std::io::empty()).unwrap(),
        Vec::<u8>::new()
    );
}

#[test]
fn menu_bar_companion_respects_launch_context() {
    assert!(companion_allowed(true, true, false, false));
    for flags in [
        (false, true, false, false),
        (true, false, false, false),
        (true, true, true, false),
        (true, true, false, true),
    ] {
        assert!(!companion_allowed(flags.0, flags.1, flags.2, flags.3));
    }
}

#[cfg(target_os = "macos")]
#[test]
fn menu_bar_lock_is_single_instance_and_released_on_drop() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("state");
    let first = macos::InstanceLock::acquire_at(&root).unwrap().unwrap();
    assert!(macos::InstanceLock::acquire_at(&root).unwrap().is_none());
    drop(first);
    assert!(macos::InstanceLock::acquire_at(&root).unwrap().is_some());
    assert_eq!(
        std::fs::metadata(root.join("menu-bar.lock")).unwrap().len(),
        0
    );
}

#[cfg(target_os = "macos")]
#[test]
fn menu_bar_lock_releases_even_with_an_inherited_file_description() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("state");
    let first = macos::InstanceLock::acquire_at(&root).unwrap().unwrap();
    // A fork before exec temporarily retains this same open file description.
    // Duplicate it without forking the multithreaded test process.
    let inherited = first.clone_file_for_test().unwrap();
    assert!(macos::InstanceLock::acquire_at(&root).unwrap().is_none());
    drop(first);
    let next = macos::InstanceLock::acquire_at(&root).unwrap().unwrap();
    drop(inherited);
    assert!(macos::InstanceLock::acquire_at(&root).unwrap().is_none());
    drop(next);
    assert!(macos::InstanceLock::acquire_at(&root).unwrap().is_some());
}

#[cfg(target_os = "macos")]
#[test]
fn menu_bar_lock_rejects_links_nonempty_files_and_permissive_directories() {
    use std::os::unix::fs::{symlink, PermissionsExt};
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path().join("state");
    std::fs::create_dir(&root).unwrap();
    let other = directory.path().join("keep");
    std::fs::write(&other, "do not change").unwrap();
    let lock = root.join("menu-bar.lock");
    symlink(&other, &lock).unwrap();
    assert!(macos::InstanceLock::acquire_at(&root).is_err());
    std::fs::remove_file(&lock).unwrap();
    std::fs::hard_link(&other, &lock).unwrap();
    assert!(macos::InstanceLock::acquire_at(&root).is_err());
    std::fs::remove_file(&lock).unwrap();
    std::fs::write(&lock, "not a lock").unwrap();
    assert!(macos::InstanceLock::acquire_at(&root).is_err());
    assert_eq!(std::fs::read_to_string(&lock).unwrap(), "not a lock");
    assert_eq!(std::fs::read_to_string(&other).unwrap(), "do not change");
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(macos::InstanceLock::acquire_at(&root).is_err());
    assert_eq!(
        std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o777
    );
}
