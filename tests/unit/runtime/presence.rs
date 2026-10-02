use super::*;
use crate::monitor::TaskMonitor;

fn publisher(
    root: &Path,
    id: &str,
    transport: RuntimeTransport,
    now: u64,
) -> RuntimePresencePublisher {
    RuntimePresencePublisher::new_at_root(
        root,
        id,
        transport,
        ["project-a".to_owned(), "project-b".to_owned()],
        (transport == RuntimeTransport::Http).then_some("http://127.0.0.1:8765"),
        now,
    )
    .unwrap()
}

#[test]
fn runtime_presence_projects_http_and_stdio_from_the_same_bounded_store() {
    let root = tempfile::tempdir().unwrap();
    let now = 1_000_000;
    let http = publisher(
        root.path(),
        "http-runtime",
        RuntimeTransport::Http,
        now - 1_000,
    );
    let stdio = publisher(
        root.path(),
        "stdio-runtime",
        RuntimeTransport::Stdio,
        now - 1_000,
    );
    let http_monitor = TaskMonitor::new(["project-a".to_owned(), "project-b".to_owned()]);
    http_monitor.mark_mcp_initialized();
    http_monitor.mark_oauth_authorized();
    http_monitor.mark_public_endpoint("quick-tunnel", Some(true));

    let verification_running = http_monitor.queue("project-a", "verify_project", "verification", 0);
    verification_running.start();
    let _verification_queued = http_monitor.queue("project-b", "verify_project", "verification", 0);

    let job_running = http_monitor.queue("project-a", "run_command", "durable job", 0);
    job_running.bind_command_job("job-running");
    job_running.start();
    let job_queued = http_monitor.queue("project-b", "run_command", "durable job", 0);
    job_queued.bind_command_job("job-queued");

    // A synchronous run_command is a task but not a durable command job.
    let synchronous_command =
        http_monitor.queue("project-a", "run_command", "synchronous command", 0);
    synchronous_command.start();

    let stdio_monitor = TaskMonitor::new(["project-a".to_owned()]);
    stdio_monitor.mark_mcp_connected();

    http.publish_status(&http_monitor.connection_status(), now)
        .unwrap();
    stdio
        .publish_status(&stdio_monitor.connection_status(), now + 1)
        .unwrap();
    let snapshot = snapshot_at(root.path(), now + 2).unwrap();

    assert_eq!(snapshot.records.len(), 2);
    assert!(!snapshot.partial);
    let http = snapshot
        .records
        .iter()
        .find(|record| record.transport == RuntimeTransport::Http)
        .unwrap();
    assert!(http.mcp_connected);
    assert!(http.oauth_authorized);
    assert_eq!(http.initialize_count, 1);
    assert_eq!(http.local_url.as_deref(), Some("http://127.0.0.1:8765"));
    assert_eq!(http.active_verifications, Some(1));
    assert_eq!(http.queued_verifications, Some(1));
    assert_eq!(http.active_jobs, Some(1));
    assert_eq!(http.queued_jobs, Some(1));
    let stdio = snapshot
        .records
        .iter()
        .find(|record| record.transport == RuntimeTransport::Stdio)
        .unwrap();
    assert!(stdio.mcp_connected);
    assert!(stdio.local_url.is_none());
    assert_eq!(stdio.active_verifications, Some(0));
    assert_eq!(stdio.queued_verifications, Some(0));
    assert_eq!(stdio.active_jobs, Some(0));
    assert_eq!(stdio.queued_jobs, Some(0));
}

#[test]
fn runtime_presence_delayed_heartbeat_cannot_overwrite_a_newer_connection() {
    use std::sync::mpsc;
    use std::time::Duration;

    let root = tempfile::tempdir().unwrap();
    let publisher = publisher(
        root.path(),
        "ordered-runtime",
        RuntimeTransport::Stdio,
        now_ms(),
    );
    let monitor = TaskMonitor::new(["project-a".to_owned()]);
    let (captured_tx, captured_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (started_tx, started_rx) = mpsc::channel();
    let (completed_tx, completed_rx) = mpsc::channel();
    let (early_completion, heartbeat, connection) = std::thread::scope(|scope| {
        let publisher = &publisher;
        let monitor = &monitor;
        let heartbeat = scope.spawn(move || {
            publisher.publish_snapshot(|| {
                let old = monitor.connection_status();
                captured_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                old
            })
        });
        captured_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        monitor.mark_mcp_connected();
        let connection = scope.spawn(move || {
            started_tx.send(()).unwrap();
            let result = publisher.publish(monitor);
            completed_tx.send(()).unwrap();
            result
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let early = completed_rx
            .recv_timeout(Duration::from_millis(100))
            .is_ok();
        // Release before assertions so even a broken writer is reaped cleanly.
        release_tx.send(()).unwrap();
        (early, heartbeat.join().unwrap(), connection.join().unwrap())
    });
    assert!(
        !early_completion,
        "newer publication bypassed an unfinished snapshot"
    );
    heartbeat.unwrap();
    connection.unwrap();
    let record = read_record(&publisher.inner.path).unwrap();
    assert!(
        record.mcp_connected,
        "delayed heartbeat restored a stale disconnected state"
    );
}

#[tokio::test]
async fn runtime_presence_stopped_heartbeat_does_not_recreate_removed_record() {
    let root = tempfile::tempdir().unwrap();
    let publisher = publisher(
        root.path(),
        "stopped-runtime",
        RuntimeTransport::Stdio,
        now_ms(),
    );
    let monitor = TaskMonitor::new(["project-a".to_owned()]);
    publisher.publish(&monitor).unwrap();
    let mut heartbeat = publisher.spawn_heartbeat(monitor);
    tokio::task::yield_now().await;
    heartbeat.shutdown().await;
    publisher.remove().unwrap();
    assert!(!publisher.inner.path.exists());
    assert!(snapshot_at(root.path(), now_ms())
        .unwrap()
        .records
        .is_empty());
}

#[test]
fn runtime_presence_removed_publisher_cannot_reappear_from_a_late_update() {
    let root = tempfile::tempdir().unwrap();
    let publisher = publisher(
        root.path(),
        "removed-runtime",
        RuntimeTransport::Stdio,
        now_ms(),
    );
    let late_writer = publisher.clone();
    let monitor = TaskMonitor::new(["project-a".to_owned()]);
    publisher.publish(&monitor).unwrap();
    publisher.remove().unwrap();
    late_writer.publish(&monitor).unwrap();
    assert!(
        !publisher.inner.path.exists(),
        "a late update recreated a stopped runtime"
    );
    publisher.remove().unwrap();
    assert!(snapshot_at(root.path(), now_ms())
        .unwrap()
        .records
        .is_empty());
}

#[test]
fn runtime_presence_removal_waits_for_an_inflight_publication() {
    use std::sync::mpsc;

    let root = tempfile::tempdir().unwrap();
    let publisher = publisher(
        root.path(),
        "removing-runtime",
        RuntimeTransport::Stdio,
        now_ms(),
    );
    let monitor = TaskMonitor::new(["project-a".to_owned()]);
    let (captured_tx, captured_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (started_tx, started_rx) = mpsc::channel();
    let (removed_tx, removed_rx) = mpsc::channel();
    let early_removal = std::thread::scope(|scope| {
        let publisher = &publisher;
        let monitor = &monitor;
        let writer = scope.spawn(move || {
            publisher.publish_snapshot(|| {
                captured_tx.send(()).unwrap();
                release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                monitor.connection_status()
            })
        });
        captured_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let remover = scope.spawn(move || {
            started_tx.send(()).unwrap();
            let result = publisher.remove();
            removed_tx.send(()).unwrap();
            result
        });
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let early = removed_rx.recv_timeout(Duration::from_millis(100)).is_ok();
        release_tx.send(()).unwrap();
        writer.join().unwrap().unwrap();
        remover.join().unwrap().unwrap();
        early
    });
    assert!(!early_removal, "cleanup bypassed an in-flight publication");
    publisher.publish(&monitor).unwrap();
    assert!(
        !publisher.inner.path.exists(),
        "publication recreated a removed runtime"
    );
}

#[cfg(unix)]
#[test]
fn runtime_presence_reader_rejects_file_aliases_without_touching_the_target() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let publisher = publisher(
        root.path(),
        "file-alias-runtime",
        RuntimeTransport::Stdio,
        now_ms(),
    );
    publisher
        .publish(&TaskMonitor::new(["project-a".to_owned()]))
        .unwrap();
    let original = fs::read(&publisher.inner.path).unwrap();
    let target = outside.path().join(record_filename("file-alias-runtime"));
    fs::rename(&publisher.inner.path, &target).unwrap();

    symlink(&target, &publisher.inner.path).unwrap();
    assert!(read_record(&publisher.inner.path).is_err());
    assert!(publisher.remove().is_err());
    assert_eq!(fs::read(&target).unwrap(), original);
    fs::remove_file(&publisher.inner.path).unwrap();

    fs::hard_link(&target, &publisher.inner.path).unwrap();
    assert!(read_record(&publisher.inner.path).is_err());
    assert!(publisher.remove().is_err());
    assert_eq!(fs::read(&target).unwrap(), original);
    fs::remove_file(&publisher.inner.path).unwrap();

    fs::create_dir(&publisher.inner.path).unwrap();
    assert!(read_record(&publisher.inner.path).is_err());
    fs::remove_dir(&publisher.inner.path).unwrap();
    fs::write(
        &publisher.inner.path,
        vec![b' '; MAX_RECORD_BYTES as usize + 1],
    )
    .unwrap();
    assert!(read_record(&publisher.inner.path).is_err());
    assert_eq!(fs::read(&target).unwrap(), original);
}

#[tokio::test]
async fn runtime_presence_dropped_heartbeat_releases_its_publisher() {
    let root = tempfile::tempdir().unwrap();
    let publisher = publisher(
        root.path(),
        "cancelled-runtime",
        RuntimeTransport::Stdio,
        now_ms(),
    );
    let monitor = TaskMonitor::new(["project-a".to_owned()]);
    publisher.publish(&monitor).unwrap();
    let path = publisher.inner.path.clone();
    let heartbeat = publisher.spawn_heartbeat(monitor);
    tokio::task::yield_now().await;
    drop(heartbeat);
    drop(publisher);
    tokio::time::timeout(Duration::from_secs(2), async {
        while path.exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("dropping a session left its presence heartbeat running");
}

#[test]
fn runtime_presence_parallel_startup_accepts_a_shared_directory() {
    let root = tempfile::tempdir().unwrap();
    let barrier = std::sync::Barrier::new(16);
    std::thread::scope(|scope| {
        let handles: Vec<_> = (0..16)
            .map(|index| {
                let root = root.path();
                let barrier = &barrier;
                scope.spawn(move || {
                    barrier.wait();
                    RuntimePresencePublisher::new_at_root(
                        root,
                        &format!("parallel-{index}"),
                        RuntimeTransport::Stdio,
                        ["project-a".to_owned()],
                        None,
                        now_ms(),
                    )
                })
            })
            .collect();
        for handle in handles {
            handle
                .join()
                .unwrap()
                .expect("parallel startup rejected an existing safe directory");
        }
    });
}

#[test]
fn runtime_presence_readers_observe_complete_records_during_replacement() {
    let root = tempfile::tempdir().unwrap();
    let publisher = publisher(
        root.path(),
        "replace-runtime",
        RuntimeTransport::Http,
        now_ms(),
    );
    let monitor = TaskMonitor::new(["project-a".to_owned()]);
    publisher.publish(&monitor).unwrap();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let writer = scope.spawn(|| {
            barrier.wait();
            for _ in 0..128 {
                publisher.publish(&monitor).unwrap();
            }
        });
        barrier.wait();
        for _ in 0..256 {
            let record =
                read_record(&publisher.inner.path).expect("reader saw a missing or partial record");
            assert_eq!(record.instance_id, "replace-runtime");
        }
        writer.join().unwrap();
    });
}

#[test]
fn runtime_presence_never_serializes_tokens_owner_args_paths_or_raw_errors() {
    let root = tempfile::tempdir().unwrap();
    let now = 2_000_000;
    let publisher = publisher(root.path(), "safe-runtime", RuntimeTransport::Http, now);
    let monitor = TaskMonitor::new(["workspace-id".to_owned()]);
    monitor.mark_mcp_connected();
    publisher
        .publish_status(&monitor.connection_status(), now + 1)
        .unwrap();
    let bytes = fs::read(&publisher.inner.path).unwrap();
    let text = String::from_utf8(bytes).unwrap();

    for forbidden in [
        "ui_token",
        "oauth_token",
        "owner",
        "arguments",
        "command",
        "source_path",
        "PRIVATE",
    ] {
        assert!(!text.contains(forbidden), "{forbidden} leaked: {text}");
    }
    assert!(text.contains("\"project-a\""));
}

#[test]
fn runtime_presence_stale_corrupt_and_future_records_are_not_active_success() {
    let root = tempfile::tempdir().unwrap();
    let now = 3_000_000;
    let stale = publisher(
        root.path(),
        "stale-runtime",
        RuntimeTransport::Http,
        now - ACTIVE_TTL_MS - 10,
    );
    let monitor = TaskMonitor::new(["project".to_owned()]);
    stale
        .publish_status(&monitor.connection_status(), now - ACTIVE_TTL_MS - 1)
        .unwrap();

    let directory = ensure_directory(root.path()).unwrap();
    fs::write(directory.join("corrupt.json"), b"{broken").unwrap();

    let future = publisher(root.path(), "future-runtime", RuntimeTransport::Stdio, now);
    future
        .publish_status(&monitor.connection_status(), now + MAX_FUTURE_SKEW_MS + 1)
        .unwrap();

    let snapshot = snapshot_at(root.path(), now).unwrap();
    assert!(snapshot.records.is_empty());
    assert_eq!(snapshot.stale_records, 2);
    assert_eq!(snapshot.invalid_records, 1);
    assert!(snapshot.partial);
}

#[test]
fn runtime_presence_rejects_non_loopback_or_secret_bearing_urls() {
    for value in [
        "https://127.0.0.1:8765",
        "http://example.com:8765",
        "http://user:secret@127.0.0.1:8765",
        "http://127.0.0.1:8765/?token=secret",
        "http://127.0.0.1:8765/#token=secret",
    ] {
        assert!(validate_local_url(value).is_err(), "{value}");
    }
    assert_eq!(
        validate_local_url("http://localhost:8765").unwrap(),
        "http://localhost:8765"
    );
}

#[cfg(unix)]
#[test]
fn runtime_presence_rejects_symlink_directory_and_hardlinked_record() {
    use std::os::unix::fs::symlink;

    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();

    let alias_parent = tempfile::tempdir().unwrap();
    let alias_root = alias_parent.path().join("state-link");
    symlink(outside.path(), &alias_root).unwrap();
    assert!(snapshot_at(&alias_root, 1).is_err());

    let missing_beneath_alias = alias_root.join("new-state");
    assert!(RuntimePresencePublisher::new_at_root(
        &missing_beneath_alias,
        "ancestor-link-runtime",
        RuntimeTransport::Http,
        Vec::<String>::new(),
        None,
        1,
    )
    .is_err());
    assert!(!outside.path().join("new-state").exists());

    symlink(outside.path(), root.path().join("runtime-presence")).unwrap();
    assert!(RuntimePresencePublisher::new_at_root(
        root.path(),
        "parent-link-runtime",
        RuntimeTransport::Http,
        Vec::<String>::new(),
        None,
        1,
    )
    .is_err());
    assert!(snapshot_at(root.path(), 1).is_err());

    let leaf_root = tempfile::tempdir().unwrap();
    fs::create_dir_all(leaf_root.path().join("runtime-presence")).unwrap();
    symlink(
        outside.path(),
        leaf_root
            .path()
            .join("runtime-presence")
            .join(STORE_VERSION),
    )
    .unwrap();
    assert!(RuntimePresencePublisher::new_at_root(
        leaf_root.path(),
        "leaf-link-runtime",
        RuntimeTransport::Http,
        Vec::<String>::new(),
        None,
        1,
    )
    .is_err());
    assert!(snapshot_at(leaf_root.path(), 1).is_err());

    let clean = tempfile::tempdir().unwrap();
    let publisher = publisher(clean.path(), "linked-runtime", RuntimeTransport::Http, 10);
    publisher
        .publish_status(&TaskMonitor::new(["a".to_owned()]).connection_status(), 11)
        .unwrap();
    let linked = publisher.inner.directory.join("copy.json");
    fs::hard_link(&publisher.inner.path, &linked).unwrap();
    assert!(snapshot_at(clean.path(), 12).unwrap().partial);
}

#[test]
fn runtime_presence_ignores_its_atomic_staging_file_without_inventing_partial_coverage() {
    let root = tempfile::tempdir().unwrap();
    let directory = ensure_directory(root.path()).unwrap();
    fs::write(directory.join(".presence-interrupted.tmp"), b"{incomplete").unwrap();
    let snapshot = snapshot_at(root.path(), 10).unwrap();
    assert!(snapshot.records.is_empty());
    assert_eq!(snapshot.invalid_records, 0);
    assert!(!snapshot.partial);
}

#[test]
fn runtime_presence_scan_is_bounded_and_reports_partial_coverage() {
    let root = tempfile::tempdir().unwrap();
    let directory = ensure_directory(root.path()).unwrap();
    for index in 0..=MAX_SCAN_ENTRIES {
        fs::write(directory.join(format!("junk-{index:03}")), b"x").unwrap();
    }
    let snapshot = snapshot_at(root.path(), 10).unwrap();
    assert!(snapshot.scan_truncated);
    assert!(snapshot.partial);
    assert!(snapshot.records.is_empty());
}

#[test]
fn runtime_presence_drop_only_removes_the_publishers_owned_identity() {
    let root = tempfile::tempdir().unwrap();
    let now = 4_000_000;
    let first = publisher(root.path(), "first-runtime", RuntimeTransport::Http, now);
    let second = publisher(root.path(), "second-runtime", RuntimeTransport::Stdio, now);
    let monitor = TaskMonitor::new(["a".to_owned()]);
    first
        .publish_status(&monitor.connection_status(), now + 1)
        .unwrap();
    second
        .publish_status(&monitor.connection_status(), now + 1)
        .unwrap();
    let first_path = first.inner.path.clone();
    let second_path = second.inner.path.clone();

    drop(first);
    assert!(!first_path.exists());
    assert!(second_path.exists());
    let snapshot = snapshot_at(root.path(), now + 2).unwrap();
    assert_eq!(snapshot.records.len(), 1);
    assert_eq!(snapshot.records[0].instance_id, "second-runtime");
}
