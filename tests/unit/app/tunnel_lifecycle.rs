use super::*;

#[test]
fn advertised_tunnel_is_registered_without_changing_the_primary() {
    let auth = AuthState::new("http://127.0.0.1:8765".to_owned());
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    publish_verified_endpoint(&auth, &monitor, "tailscale", "https://verified.example");
    for (_, url) in monitor.tunnel_links() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("host", "verified.example".parse().unwrap());
        headers.insert("origin", url.parse().unwrap());
        assert_eq!(auth.request_public_url(&headers), Some(url));
        assert!(auth.origin_allowed(&headers));
    }
    assert_eq!(monitor.tunnel_links().len(), 1);
    assert_eq!(auth.public_url(), "http://127.0.0.1:8765");
}

#[test]
fn empty_tunnel_set_never_produces_a_dead_index() {
    assert_eq!(dead_tunnel_index(true, 0, |_| true), None);
    assert_eq!(dead_tunnel_index(false, 0, |_| true), None);
}

#[test]
fn health_failure_targets_only_the_primary_when_one_exists() {
    assert_eq!(dead_tunnel_index(true, 3, |_| false), Some(0));
    assert_eq!(dead_tunnel_index(false, 3, |index| index == 2), Some(2));
}

#[test]
fn reconnect_backoff_is_bounded_and_recovery_resets_history() {
    assert_eq!(reconnect_backoff_seconds(1), 5);
    assert_eq!(reconnect_backoff_seconds(2), 10);
    assert_eq!(reconnect_backoff_seconds(5), 80);
    assert_eq!(reconnect_backoff_seconds(6), 160);
    assert_eq!(reconnect_backoff_seconds(7), 300);
    assert_eq!(reconnect_backoff_seconds(20), 300);

    let mut deaths = HashMap::new();
    assert_eq!(
        record_dead_provider(&mut deaths, TunnelProvider::Tailscale),
        5
    );
    assert_eq!(
        record_dead_provider(&mut deaths, TunnelProvider::Tailscale),
        10
    );
    assert_eq!(record_dead_provider(&mut deaths, TunnelProvider::Pinggy), 5);
    assert_eq!(deaths[&TunnelProvider::Tailscale], 2);
    assert_eq!(deaths[&TunnelProvider::Pinggy], 1);

    record_recovered_provider(&mut deaths, TunnelProvider::Tailscale);
    assert!(!deaths.contains_key(&TunnelProvider::Tailscale));
    assert_eq!(deaths[&TunnelProvider::Pinggy], 1);
}
