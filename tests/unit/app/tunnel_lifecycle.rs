use super::*;

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
