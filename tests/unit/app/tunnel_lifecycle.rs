use super::*;

fn registered_fixture(auth: &AuthState, provider: TunnelProvider, url: String) -> ActiveTunnel {
    let epoch = auth
        .public_url_epoch(&url)
        .unwrap_or_else(|| auth.register_public_url(url.clone()));
    let mut active = ActiveTunnel::test_fixture(provider, url);
    active.endpoint_epoch = Some(epoch);
    active
}

#[test]
fn queued_connection_cannot_borrow_a_newer_registration() {
    let auth = AuthState::new("http://127.0.0.1:8765".to_owned());
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    let url = "https://queued.example";
    publish_verified_endpoint(&auth, &monitor, "tailscale", url);
    let old_epoch = auth.public_url_epoch(url).unwrap();
    let active = registered_fixture(&auth, TunnelProvider::Tailscale, url.to_owned());
    publish_verified_endpoint(&auth, &monitor, "tailscale", url);
    let newer_epoch = auth.public_url_epoch(url).unwrap();
    assert_ne!(old_epoch, newer_epoch);
    let mut state = TunnelControlState::default();
    state.connected(&active, &auth);
    assert!(
        !state.endpoint_epochs.contains_key(url),
        "queued old event borrowed replacement ownership"
    );
}

#[test]
fn duplicate_probe_completion_is_not_a_second_failure() {
    let url = "https://duplicate.example".to_owned();
    let mut lease = StandbyHealthLease::verified(Instant::now());
    lease.begin_probe(Instant::now() + STANDBY_PROBE_INTERVAL);
    let epoch = lease.epoch();
    let mut leases = HashMap::from([(url.clone(), lease)]);
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    for _ in 0..2 {
        assert!(!handle_standby_probe(
            &mut leases,
            &[],
            StandbyProbeEvent {
                public_url: url.clone(),
                lease_epoch: epoch,
                result: Err("same completion".to_owned())
            },
            &monitor
        ));
    }
    assert_eq!(leases[&url].failures(), 1);
}

#[test]
fn quarantine_invalidates_a_pre_quarantine_success() {
    let url = "https://lease.example".to_owned();
    let now = Instant::now();
    let mut lease = StandbyHealthLease::verified(now);
    assert!(lease.begin_probe(now + STANDBY_PROBE_INTERVAL));
    let stale_epoch = lease.epoch();
    lease.quarantine(now + STANDBY_PROBE_INTERVAL);
    let mut leases = HashMap::from([(url.clone(), lease)]);
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    handle_standby_probe(
        &mut leases,
        &[],
        StandbyProbeEvent {
            public_url: url.clone(),
            lease_epoch: stale_epoch,
            result: Ok(()),
        },
        &monitor,
    );
    assert!(
        !leases[&url].eligible(Instant::now()),
        "late success must not undo quarantine"
    );
}

#[test]
fn probe_epoch_is_unique_for_each_attempt() {
    let now = Instant::now();
    let mut lease = StandbyHealthLease::verified(now);
    assert!(lease.begin_probe(now + STANDBY_PROBE_INTERVAL));
    let first = lease.epoch();
    lease.record_failure(now + STANDBY_PROBE_INTERVAL);
    assert!(lease.begin_probe(now + STANDBY_PROBE_INTERVAL + STANDBY_RETRY_INTERVAL));
    assert_ne!(
        first,
        lease.epoch(),
        "a previous attempt must not settle a newer probe"
    );
}

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
fn reconnecting_stable_provider_does_not_revoke_a_still_retained_alias() {
    let auth = AuthState::new("http://127.0.0.1:8765".to_owned());
    auth.register_public_url("https://old.example".to_owned());
    let mut state = TunnelControlState::default();
    state
        .retained_stable_aliases
        .insert("https://old.example".to_owned(), TunnelProvider::Tailscale);
    let active = registered_fixture(
        &auth,
        TunnelProvider::Tailscale,
        "https://new.example".to_owned(),
    );

    state.connected(&active, &auth);

    let mut old = axum::http::HeaderMap::new();
    old.insert("host", "old.example".parse().unwrap());
    assert_eq!(
        auth.request_public_url(&old).as_deref(),
        Some("https://old.example")
    );
    assert_eq!(
        state.retained_stable_aliases.get("https://old.example"),
        Some(&TunnelProvider::Tailscale)
    );
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
    monitor.mark_tunnel_retry("pinggy", 4, true, Duration::from_secs(60), false);
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

    monitor.register_tunnel("tailscale", "https://stable.example");
    monitor.mark_public_url_verified();
    monitor.mark_tunnel_primary("https://stable.example");
    monitor.mark_tunnel_retry("tailscale", 1, false, Duration::from_secs(2), true);
    let retained = monitor.connection_status();
    let tailscale = retained
        .tunnels
        .iter()
        .find(|tunnel| tunnel.provider == "tailscale")
        .unwrap();
    assert_eq!(tailscale.url.as_deref(), Some("https://stable.example"));
    assert_eq!(tailscale.role, "primary");
    assert_eq!(tailscale.state, "healthy");
    assert_eq!(tailscale.death_count, 1);
}

#[test]
fn retained_aliases_are_bounded_and_reconnect_failure_is_not_revocation_evidence() {
    let auth = AuthState::new("http://127.0.0.1:8765".to_owned());
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    let mut state = TunnelControlState::default();
    for index in 0..8 {
        let url = format!("https://stable-{index}.example");
        publish_verified_endpoint(&auth, &monitor, "tailscale", &url);
        state.connected(
            &registered_fixture(&auth, TunnelProvider::Tailscale, url.clone()),
            &auth,
        );
        assert!(state.retain_stable_alias(TunnelProvider::Tailscale, &url));
    }
    assert!(!state.retain_stable_alias(TunnelProvider::Tailscale, "https://ninth.example"));
    assert_eq!(state.retained_stable_aliases.len(), 8);
    assert_eq!(state.standby_leases.len(), 8);
    state.reconnect_failed(TunnelProvider::Tailscale, "startup failed", &[], &monitor);
    for index in 0..8 {
        assert!(auth
            .public_url_epoch(&format!("https://stable-{index}.example"))
            .is_some());
    }
    assert_eq!(state.pending_respawns.len(), 1);
    let live = registered_fixture(
        &auth,
        TunnelProvider::Tailscale,
        "https://new.example".to_owned(),
    );
    state.connected(&live, &auth);
    state.reconnect_failed(
        TunnelProvider::Tailscale,
        "late startup failure",
        &[live],
        &monitor,
    );
    assert!(state.pending_respawns.is_empty());
}

#[tokio::test]
async fn stale_standby_cleanup_preserves_same_url_replacement_trust() {
    let auth = AuthState::new("http://127.0.0.1:8765".to_owned());
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    let url = "https://stable.example";
    publish_verified_endpoint(&auth, &monitor, "tailscale", url);
    let active = registered_fixture(&auth, TunnelProvider::Tailscale, url.to_owned());
    let mut state = TunnelControlState::default();
    state.connected(&active, &auth);
    let old_epoch = state.endpoint_epochs[url];
    let mut tunnels = vec![active];
    // A reconnect worker publishes before the old cleanup reaches its registry write.
    publish_verified_endpoint(&auth, &monitor, "tailscale", url);
    let replacement_epoch = auth.public_url_epoch(url).unwrap();
    assert_ne!(old_epoch, replacement_epoch);
    assert!(recycle_revoked_standby(&mut state, &mut tunnels, url, &auth, &monitor).await);
    assert_eq!(auth.public_url_epoch(url), Some(replacement_epoch));
    assert!(monitor
        .connection_status()
        .tunnels
        .iter()
        .any(|entry| entry.url.as_deref() == Some(url)));
}

#[tokio::test]
async fn retired_alias_cleanup_does_not_recycle_its_live_replacement_provider() {
    let auth = AuthState::new("http://127.0.0.1:8765".to_owned());
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    let mut state = TunnelControlState::default();
    let old_url = "https://old.example";
    let new_url = "https://new.example";
    publish_verified_endpoint(&auth, &monitor, "tailscale", old_url);
    state.connected(
        &registered_fixture(&auth, TunnelProvider::Tailscale, old_url.to_owned()),
        &auth,
    );
    state.primary_url = Some(old_url.to_owned());
    assert!(state.retain_stable_alias(TunnelProvider::Tailscale, old_url));
    publish_verified_endpoint(&auth, &monitor, "tailscale", new_url);
    let active = registered_fixture(&auth, TunnelProvider::Tailscale, new_url.to_owned());
    state.connected(&active, &auth);
    assert!(
        state.primary_url.is_none(),
        "old retained primary must not strand the replacement as standby"
    );
    assert!(state.standby_leases.contains_key(old_url));
    let mut tunnels = vec![active];
    state.primary_url = Some(new_url.to_owned());
    assert!(recycle_revoked_standby(&mut state, &mut tunnels, old_url, &auth, &monitor).await);
    assert!(auth.public_url_epoch(old_url).is_none());
    assert!(auth.public_url_epoch(new_url).is_some());
    assert!(!state.retained_stable_aliases.contains_key(old_url));
    assert!(!state.standby_leases.contains_key(old_url));
    assert!(state.pending_respawns.is_empty());
    assert_eq!(tunnels.len(), 1);
    assert!(!recycle_revoked_standby(&mut state, &mut tunnels, new_url, &auth, &monitor).await);
}

#[tokio::test]
async fn retained_aliases_receive_instance_matched_probes_without_a_provider_child() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = axum::Router::new().route(
        "/healthz",
        axum::routing::get(|| async {
            axum::Json(serde_json::json!({"ok":true,"instance_id":"retained-probe-test"}))
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut state = TunnelControlState::default();
    assert!(state.retain_stable_alias(TunnelProvider::Tailscale, &url));
    state.standby_leases.insert(
        url.clone(),
        StandbyHealthLease::verified(
            Instant::now() - STANDBY_PROBE_INTERVAL - Duration::from_secs(1),
        ),
    );
    let (tx, mut rx) = mpsc::channel(2);
    schedule_standby_probes(
        &mut state.standby_leases,
        &[],
        &state.retained_stable_aliases,
        None,
        "retained-probe-test",
        &tx,
    );
    schedule_standby_probes(
        &mut state.standby_leases,
        &[],
        &state.retained_stable_aliases,
        None,
        "retained-probe-test",
        &tx,
    );
    let result = tokio::time::timeout(Duration::from_secs(12), rx.recv()).await;
    server.abort();
    let event = result.expect("retained alias probe must finish").unwrap();
    assert_eq!(event.public_url, url);
    assert_eq!(event.lease_epoch, state.standby_leases[&url].epoch());
    assert!(event.result.is_ok(), "{:?}", event.result);
    assert!(
        rx.try_recv().is_err(),
        "same lease must remain single-flight"
    );
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
    assert_eq!(reconnect_backoff_seconds(1), 2);
    assert_eq!(reconnect_backoff_seconds(2), 4);
    assert_eq!(reconnect_backoff_seconds(5), 32);
    assert_eq!(reconnect_backoff_seconds(6), 64);
    assert_eq!(reconnect_backoff_seconds(7), 120);
    assert_eq!(reconnect_backoff_seconds(20), 120);

    let mut deaths = HashMap::new();
    assert_eq!(
        record_dead_provider(&mut deaths, TunnelProvider::Tailscale),
        2
    );
    assert_eq!(
        record_dead_provider(&mut deaths, TunnelProvider::Tailscale),
        5
    );
    assert_eq!(record_dead_provider(&mut deaths, TunnelProvider::Pinggy), 3);
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
    let delays = (0..5)
        .map(|_| record_dead_provider(&mut deaths, provider))
        .collect::<Vec<_>>();
    assert_eq!(delays, vec![4, 7, 8, 17, 34]);
    assert_eq!(deaths[&provider], 5);
    assert!(provider_circuit_open(deaths[&provider]));

    let now = Instant::now();
    let due = now + Duration::from_secs(delays[4]);
    assert!(!provider_retry_due(due, now + Duration::from_secs(33)));
    assert!(provider_retry_due(due, now + Duration::from_secs(34)));

    assert!(record_recovered_provider(&mut deaths, provider));
    assert!(!deaths.contains_key(&provider));
    assert!(!provider_circuit_open(0));
    assert_eq!(record_dead_provider(&mut deaths, provider), 4);
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
    assert_eq!(first_delays, [4, 5, 3, 2]);
    assert_eq!(
        reconnect_delay_seconds(TunnelProvider::Cloudflare, 1),
        first_delays[0]
    );
    assert!(!provider_circuit_open(4));
    assert!(provider_circuit_open(5));
    assert!(reconnect_delay_seconds(TunnelProvider::Tailscale, 5) >= 30);
    assert_eq!(reconnect_delay_seconds(TunnelProvider::Cloudflare, 20), 120);
    assert!(first_delays.iter().all(|delay| (2..=5).contains(delay)));
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

    assert!(!handle_standby_probe(
        &mut leases,
        &[],
        StandbyProbeEvent {
            public_url: url.clone(),
            lease_epoch: old_epoch,
            result: Err("stale probe failure".to_owned()),
        },
        &monitor,
    ));
    assert_eq!(leases[&url].failures(), 0);

    assert!(!handle_standby_probe(
        &mut leases,
        &[],
        StandbyProbeEvent {
            public_url: url.clone(),
            lease_epoch: replacement_epoch,
            result: Err("current probe failure".to_owned()),
        },
        &monitor,
    ));
    assert_eq!(leases[&url].failures(), 1);
}

#[test]
fn quarantined_endpoint_revokes_after_failed_recovery_probe() {
    let url = "https://stuck.example".to_owned();
    let now = Instant::now();
    let mut lease = StandbyHealthLease::verified(now);
    lease.quarantine(now);
    let epoch = lease.epoch();
    let mut leases = HashMap::from([(url.clone(), lease)]);
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    monitor.register_tunnel("tailscale", &url);
    assert!(handle_standby_probe(
        &mut leases,
        &[],
        StandbyProbeEvent {
            public_url: url.clone(),
            lease_epoch: epoch,
            result: Err("endpoint still unreachable".to_owned()),
        },
        &monitor,
    ));
    assert!(!leases[&url].eligible(Instant::now()));
}

#[test]
fn standby_health_lease_needs_two_failures_and_expires_without_refresh() {
    let start = Instant::now();
    let mut lease = StandbyHealthLease::verified(start);
    assert!(lease.eligible(start + STANDBY_LEASE_TTL - Duration::from_secs(1)));
    assert!(!lease.begin_probe(start + STANDBY_PROBE_INTERVAL - Duration::from_secs(1)));
    assert!(lease.begin_probe(start + STANDBY_PROBE_INTERVAL));
    assert!(!lease.record_failure(start + STANDBY_PROBE_INTERVAL));
    assert_eq!(lease.failures(), 1);
    let retry_due = start + STANDBY_PROBE_INTERVAL + STANDBY_RETRY_INTERVAL;
    assert!(lease.eligible(retry_due - Duration::from_secs(1)));
    assert!(!lease.begin_probe(retry_due - Duration::from_secs(1)));
    assert!(lease.begin_probe(retry_due));
    assert!(lease.record_failure(retry_due));
    assert!(!lease.eligible(retry_due));

    let recovered_at = retry_due + Duration::from_secs(1);
    assert!(lease.record_success(recovered_at));
    assert!(lease.eligible(recovered_at));
    assert!(!lease.eligible(recovered_at + STANDBY_LEASE_TTL + Duration::from_secs(1)));
}
