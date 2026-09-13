use super::*;

#[test]
fn stale_managed_primary_health_result_cannot_poison_a_promoted_tunnel() {
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    let old = "https://old.example";
    let promoted = "https://promoted.example";
    monitor.register_tunnel("cloudflare", old);
    monitor.register_tunnel("pinggy", promoted);
    monitor.mark_tunnel_primary(old);
    monitor.mark_public_url_verified();
    assert!(monitor.mark_managed_public_url_check(old, false, Some("old failure".to_owned())));
    assert_eq!(
        monitor.connection_status().public_url_consecutive_failures,
        1
    );

    monitor.mark_tunnel_primary(promoted);
    monitor.mark_public_url_verified();
    assert!(!monitor.mark_managed_public_url_check(
        old,
        false,
        Some("stale old failure".to_owned())
    ));
    let after_stale = monitor.connection_status();
    assert_eq!(after_stale.public_url_healthy, Some(true));
    assert_eq!(after_stale.public_url_consecutive_failures, 0);
    assert!(after_stale.public_url_error.is_none());

    assert!(monitor.mark_managed_public_url_check(
        promoted,
        false,
        Some("current failure".to_owned())
    ));
    let current = monitor.connection_status();
    assert_eq!(current.public_url_healthy, Some(true));
    assert_eq!(current.public_url_consecutive_failures, 1);
    assert_eq!(current.public_url_error.as_deref(), Some("current failure"));
}

#[test]
fn public_url_health_requires_three_failures_and_two_successes_to_recover() {
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    monitor.mark_public_url_verified();
    assert_eq!(monitor.connection_status().public_url_healthy, Some(true));

    monitor.mark_public_url_check(false, Some("first".to_owned()));
    let status = monitor.connection_status();
    assert_eq!(status.public_url_healthy, Some(true));
    assert_eq!(status.public_url_consecutive_failures, 1);

    monitor.mark_public_url_check(false, Some("second".to_owned()));
    assert_eq!(
        monitor.connection_status().public_url_consecutive_failures,
        2
    );
    assert_eq!(monitor.connection_status().public_url_healthy, Some(true));

    monitor.mark_public_url_check(false, Some("third".to_owned()));
    let status = monitor.connection_status();
    assert_eq!(status.public_url_healthy, Some(false));
    assert_eq!(status.public_url_consecutive_failures, 3);
    assert_eq!(status.public_url_error.as_deref(), Some("third"));
    assert!(status.public_url_last_checked_seconds_ago.is_some());

    monitor.mark_public_url_check(true, None);
    assert_eq!(monitor.connection_status().public_url_healthy, Some(false));
    monitor.mark_public_url_check(true, None);
    let status = monitor.connection_status();
    assert_eq!(status.public_url_healthy, Some(true));
    assert_eq!(status.public_url_consecutive_failures, 0);
    assert!(status.public_url_error.is_none());

    monitor.mark_public_url_pending(Some("no verified standby".to_owned()));
    let pending = monitor.connection_status();
    assert_eq!(pending.public_url_healthy, None);
    assert!(pending.public_url_last_checked_seconds_ago.is_none());
    assert_eq!(
        pending.public_url_error.as_deref(),
        Some("no verified standby")
    );
}
