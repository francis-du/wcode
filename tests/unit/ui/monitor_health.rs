use super::*;

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

#[test]
fn endpoint_health_is_independent_and_all_verified_urls_are_active() {
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    monitor.register_tunnel("cloudflare", "https://one.example");
    monitor.register_tunnel("tailscale", "https://two.example");
    monitor.mark_tunnel_endpoint_probe("https://one.example", false, 2, true);
    let status = monitor.connection_status();
    assert!(status.tunnels.iter().all(|t| t.role == "active"));
    let one = status
        .tunnels
        .iter()
        .find(|t| t.provider == "cloudflare")
        .unwrap();
    let two = status
        .tunnels
        .iter()
        .find(|t| t.provider == "tailscale")
        .unwrap();
    assert_eq!(one.state, "quarantined");
    assert_eq!(two.state, "verified");
    assert_eq!(two.consecutive_failures, 0);
    monitor.mark_tunnel_endpoint_probe("https://one.example", true, 0, false);
    assert!(monitor
        .connection_status()
        .tunnels
        .iter()
        .all(|t| t.state == "verified"));
}

#[test]
fn same_provider_endpoints_keep_independent_runtime_state() {
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    let old = "https://old-stable.example";
    let new = "https://new-stable.example";
    monitor.register_tunnel("tailscale", old);
    monitor.register_tunnel("tailscale", new);

    let status = monitor.connection_status();
    assert_eq!(status.tunnels.len(), 2);
    assert!(status
        .tunnels
        .iter()
        .any(|tunnel| tunnel.url.as_deref() == Some(old)));
    assert!(status
        .tunnels
        .iter()
        .any(|tunnel| tunnel.url.as_deref() == Some(new)));

    monitor.mark_tunnel_endpoint_probe(old, false, 2, true);
    let status = monitor.connection_status();
    assert_eq!(
        status
            .tunnels
            .iter()
            .find(|tunnel| tunnel.url.as_deref() == Some(old))
            .unwrap()
            .state,
        "quarantined"
    );
    assert_eq!(
        status
            .tunnels
            .iter()
            .find(|tunnel| tunnel.url.as_deref() == Some(new))
            .unwrap()
            .state,
        "verified"
    );

    monitor.mark_tunnel_retry("tailscale", 1, false, Duration::from_secs(2), true);
    let status = monitor.connection_status();
    assert_eq!(status.tunnels.len(), 3);
    assert!(status.tunnels.iter().any(|tunnel| {
        tunnel.provider == "tailscale"
            && tunnel.url.is_none()
            && tunnel.role == "retrying"
            && tunnel.state == "reconnecting"
    }));
    assert_eq!(
        status
            .tunnels
            .iter()
            .find(|tunnel| tunnel.url.as_deref() == Some(new))
            .unwrap()
            .state,
        "verified"
    );
}

#[test]
fn concurrent_endpoint_mode_uses_live_health_for_setup_readiness() {
    let monitor = TaskMonitor::new(["backend".to_owned()]);
    monitor.mark_public_endpoint("concurrent", Some(true));
    monitor.mark_concurrent_health(true);
    assert!(public_endpoint_ready(&monitor.snapshot()));

    monitor.mark_concurrent_health(false);
    assert!(!public_endpoint_ready(&monitor.snapshot()));
}
