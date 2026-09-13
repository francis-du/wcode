use super::*;

#[test]
fn advertised_tunnel_is_registered_without_changing_the_primary() {
    let auth = AuthState::new("http://127.0.0.1:8765".to_owned());
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    publish_verified_endpoint(&auth, &monitor, "tailscale", "https://verified.example");
    let status = monitor.connection_status();
    assert_eq!(status.tunnels.len(), 1);
    let tunnel = &status.tunnels[0];
    let url = tunnel.url.as_deref().unwrap();
    let mut headers = axum::http::HeaderMap::new();
    headers.insert("host", "verified.example".parse().unwrap());
    headers.insert("origin", url.parse().unwrap());
    assert_eq!(auth.request_public_url(&headers).as_deref(), Some(url));
    assert!(auth.origin_allowed(&headers));
    assert_eq!(tunnel.provider, "tailscale");
    assert_eq!(tunnel.role, "standby");
    assert_eq!(tunnel.state, "verified");
    assert_eq!(auth.public_url(), "http://127.0.0.1:8765");
}

#[test]
fn tunnel_runtime_observability_explains_primary_standby_and_circuit_retry() {
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    monitor.register_tunnel("cloudflare", "https://primary.example");
    monitor.mark_public_url_verified();
    monitor.mark_tunnel_primary("https://primary.example");
    monitor.register_tunnel("pinggy", "https://standby.example");
    monitor.mark_tunnel_standby_probe("https://standby.example", false, 1, false);

    let live = monitor.connection_status();
    let primary = live
        .tunnels
        .iter()
        .find(|tunnel| tunnel.provider == "cloudflare")
        .unwrap();
    let standby = live
        .tunnels
        .iter()
        .find(|tunnel| tunnel.provider == "pinggy")
        .unwrap();
    assert_eq!(
        (primary.role.as_str(), primary.state.as_str()),
        ("primary", "healthy")
    );
    assert_eq!(
        (standby.role.as_str(), standby.state.as_str()),
        ("standby", "suspect")
    );
    assert_eq!(standby.consecutive_failures, 1);

    monitor.remove_tunnel("https://standby.example");
    monitor.mark_tunnel_retry("pinggy", 4, true, Duration::from_secs(60));
    let retry = monitor.connection_status();
    let pinggy = retry
        .tunnels
        .iter()
        .find(|tunnel| tunnel.provider == "pinggy")
        .unwrap();
    assert_eq!(
        (pinggy.role.as_str(), pinggy.state.as_str()),
        ("retrying", "circuit-open")
    );
    assert_eq!(pinggy.death_count, 4);
    assert!(pinggy.circuit_open);
    assert!(pinggy.retry_in_seconds.is_some_and(|seconds| seconds <= 60));
}

#[test]
fn empty_tunnel_set_never_produces_a_dead_index() {
    assert_eq!(dead_tunnel_index(true, None, 0, |_| true), None);
    assert_eq!(dead_tunnel_index(false, None, 0, |_| true), None);
}

#[test]
fn health_failure_targets_the_explicit_primary_instead_of_vector_order() {
    assert_eq!(dead_tunnel_index(true, Some(1), 3, |_| false), Some(1));
    assert_eq!(
        dead_tunnel_index(false, Some(1), 3, |index| index == 2),
        Some(2)
    );
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
        8
    );
    assert_eq!(
        record_dead_provider(&mut deaths, TunnelProvider::Tailscale),
        16
    );
    assert_eq!(record_dead_provider(&mut deaths, TunnelProvider::Pinggy), 6);
    assert_eq!(deaths[&TunnelProvider::Tailscale], 2);
    assert_eq!(deaths[&TunnelProvider::Pinggy], 1);

    assert!(record_recovered_provider(
        &mut deaths,
        TunnelProvider::Tailscale
    ));
    assert!(!record_recovered_provider(
        &mut deaths,
        TunnelProvider::Tailscale
    ));
    assert!(!deaths.contains_key(&TunnelProvider::Tailscale));
    assert_eq!(deaths[&TunnelProvider::Pinggy], 1);
}

#[test]
fn provider_fault_sequence_opens_circuit_then_resets_after_verified_recovery() {
    let provider = TunnelProvider::Cloudflare;
    let mut deaths = HashMap::new();
    let delays = (0..4)
        .map(|_| record_dead_provider(&mut deaths, provider))
        .collect::<Vec<_>>();
    assert_eq!(delays, vec![9, 10, 23, 60]);
    assert_eq!(deaths[&provider], 4);
    assert!(provider_circuit_open(deaths[&provider]));

    let now = Instant::now();
    let due = now + Duration::from_secs(delays[3]);
    assert!(!provider_retry_due(due, now + Duration::from_secs(59)));
    assert!(provider_retry_due(due, now + Duration::from_secs(60)));

    assert!(record_recovered_provider(&mut deaths, provider));
    assert!(!deaths.contains_key(&provider));
    assert!(!provider_circuit_open(0));
    assert_eq!(record_dead_provider(&mut deaths, provider), 9);
    assert_eq!(deaths[&provider], 1);
    assert!(!provider_circuit_open(deaths[&provider]));
}

#[test]
fn reconnect_jitter_is_deterministic_bounded_and_provider_staggered() {
    let first_delays = [
        reconnect_delay_seconds(TunnelProvider::Cloudflare, 1),
        reconnect_delay_seconds(TunnelProvider::LocalhostRun, 1),
        reconnect_delay_seconds(TunnelProvider::Pinggy, 1),
        reconnect_delay_seconds(TunnelProvider::Tailscale, 1),
    ];
    assert_eq!(first_delays, [9, 11, 6, 8]);
    assert_eq!(
        reconnect_delay_seconds(TunnelProvider::Cloudflare, 1),
        first_delays[0]
    );
    assert!(!provider_circuit_open(3));
    assert!(provider_circuit_open(4));
    assert!(reconnect_delay_seconds(TunnelProvider::Tailscale, 4) >= 60);
    assert_eq!(reconnect_delay_seconds(TunnelProvider::Cloudflare, 20), 300);
    assert!(first_delays.iter().all(|delay| (5..=11).contains(delay)));
}

#[test]
fn standby_selection_prefers_freshest_lease_then_longest_uptime() {
    assert_eq!(
        best_standby_score(&[
            (0, Duration::from_secs(20), Duration::from_secs(500)),
            (1, Duration::from_secs(2), Duration::from_secs(20)),
        ]),
        Some(1)
    );
    assert_eq!(
        best_standby_score(&[
            (0, Duration::from_secs(2), Duration::from_secs(20)),
            (1, Duration::from_secs(2), Duration::from_secs(50)),
        ]),
        Some(1)
    );
    assert_eq!(
        best_standby_score(&[
            (2, Duration::from_secs(2), Duration::from_secs(50)),
            (1, Duration::from_secs(2), Duration::from_secs(50)),
        ]),
        Some(1)
    );
    assert_eq!(best_standby_score(&[]), None);
}

#[tokio::test]
async fn primary_runtime_shutdown_aborts_an_inflight_health_probe() {
    let auth = Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned()));
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    let (stop_tx, stop_rx) = watch::channel(false);
    let mut runtime = PrimaryRuntime::new(
        auth,
        monitor,
        Arc::new(RwLock::new("http://127.0.0.1:8765".to_owned())),
        stop_tx,
    );
    runtime.health_task = Some(tokio::spawn(async {
        tokio::time::sleep(Duration::from_secs(60)).await;
    }));

    tokio::time::timeout(Duration::from_millis(250), runtime.shutdown())
        .await
        .expect("shutdown must cancel an in-flight health probe without waiting for its timeout");
    assert!(*stop_rx.borrow());
    assert!(runtime.health_task.is_none());
}

#[test]
fn provider_retry_due_respects_scheduled_deadline() {
    let now = Instant::now();
    let due = now + Duration::from_secs(60);
    assert!(!provider_retry_due(due, now + Duration::from_secs(59)));
    assert!(provider_retry_due(due, now + Duration::from_secs(60)));
    assert!(provider_retry_due(due, now + Duration::from_secs(120)));
}

#[test]
fn stale_standby_probe_result_cannot_mutate_a_replacement_lease() {
    let url = "https://standby.example".to_owned();
    let start = Instant::now();
    let old_lease = StandbyHealthLease::verified(start);
    let old_epoch = old_lease.epoch();
    let replacement = StandbyHealthLease::verified(start + Duration::from_secs(1));
    let replacement_epoch = replacement.epoch();
    assert_ne!(old_epoch, replacement_epoch);

    let mut leases = HashMap::from([(url.clone(), replacement)]);
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    monitor.register_tunnel("pinggy", &url);

    handle_standby_probe(
        &mut leases,
        &[],
        StandbyProbeEvent {
            public_url: url.clone(),
            lease_epoch: old_epoch,
            result: Err("stale probe failure".to_owned()),
        },
        &monitor,
    );
    assert_eq!(leases[&url].failures(), 0);

    handle_standby_probe(
        &mut leases,
        &[],
        StandbyProbeEvent {
            public_url: url.clone(),
            lease_epoch: replacement_epoch,
            result: Err("current probe failure".to_owned()),
        },
        &monitor,
    );
    assert_eq!(leases[&url].failures(), 1);
}

#[test]
fn standby_health_lease_needs_two_failures_and_expires_without_refresh() {
    let start = Instant::now();
    let mut lease = StandbyHealthLease::verified(start);
    assert!(lease.eligible(start + Duration::from_secs(74)));
    assert!(!lease.begin_probe(start + Duration::from_secs(29)));
    assert!(lease.begin_probe(start + STANDBY_PROBE_INTERVAL));
    assert!(!lease.record_failure(start + STANDBY_PROBE_INTERVAL));
    assert_eq!(lease.failures(), 1);
    assert!(lease.eligible(start + Duration::from_secs(34)));
    assert!(!lease.begin_probe(start + Duration::from_secs(34)));
    assert!(lease.begin_probe(start + Duration::from_secs(35)));
    assert!(lease.record_failure(start + Duration::from_secs(35)));
    assert!(lease.revoked());
    assert!(!lease.eligible(start + Duration::from_secs(35)));

    assert!(lease.record_success(start + Duration::from_secs(36)));
    assert!(!lease.revoked());
    assert!(lease.eligible(start + Duration::from_secs(36)));
    assert!(!lease.eligible(start + Duration::from_secs(112)));
}
