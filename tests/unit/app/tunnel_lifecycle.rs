use super::policy::ENDPOINT_LEASE_TTL;
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
    let mut lease = EndpointHealthLease::verified(Instant::now());
    lease.begin_probe(Instant::now() + ENDPOINT_PROBE_INTERVAL);
    let epoch = lease.epoch();
    let mut leases = HashMap::from([(url.clone(), lease)]);
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    for _ in 0..2 {
        assert!(!handle_endpoint_probe(
            &mut leases,
            &[],
            EndpointProbeEvent {
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
    let mut lease = EndpointHealthLease::verified(now);
    assert!(lease.begin_probe(now + ENDPOINT_PROBE_INTERVAL));
    let stale_epoch = lease.epoch();
    lease.quarantine(now + ENDPOINT_PROBE_INTERVAL);
    let mut leases = HashMap::from([(url.clone(), lease)]);
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    handle_endpoint_probe(
        &mut leases,
        &[],
        EndpointProbeEvent {
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
    let mut lease = EndpointHealthLease::verified(now);
    assert!(lease.begin_probe(now + ENDPOINT_PROBE_INTERVAL));
    let first = lease.epoch();
    lease.record_failure(now + ENDPOINT_PROBE_INTERVAL);
    assert!(lease.begin_probe(now + ENDPOINT_PROBE_INTERVAL + ENDPOINT_RETRY_INTERVAL));
    assert_ne!(
        first,
        lease.epoch(),
        "a previous attempt must not settle a newer probe"
    );
}

#[test]
fn every_verified_tunnel_is_registered_as_an_active_endpoint() {
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
    assert_eq!(tunnel.role, "active");
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
fn tunnel_runtime_observability_explains_concurrent_endpoints_and_circuit_retry() {
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    monitor.register_tunnel("cloudflare", "https://primary.example");
    monitor.mark_public_url_verified();
    monitor.register_tunnel("pinggy", "https://endpoint.example");
    monitor.mark_tunnel_endpoint_probe("https://endpoint.example", false, 1, false);

    let live = monitor.connection_status();
    let primary = live
        .tunnels
        .iter()
        .find(|tunnel| tunnel.provider == "cloudflare")
        .unwrap();
    let endpoint = live
        .tunnels
        .iter()
        .find(|tunnel| tunnel.provider == "pinggy")
        .unwrap();
    assert_eq!(
        (primary.role.as_str(), primary.state.as_str()),
        ("active", "verified")
    );
    assert_eq!(
        (endpoint.role.as_str(), endpoint.state.as_str()),
        ("active", "suspect")
    );
    assert_eq!(endpoint.consecutive_failures, 1);

    monitor.remove_tunnel("https://endpoint.example");
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

    monitor.mark_tunnel_retry("tailscale", 1, false, Duration::from_secs(2), true);
    let retained = monitor.connection_status();
    let endpoint = retained
        .tunnels
        .iter()
        .find(|tunnel| tunnel.url.as_deref() == Some("https://stable.example"))
        .unwrap();
    assert_eq!(endpoint.role, "active");
    assert_eq!(endpoint.state, "verified");
    let retry = retained
        .tunnels
        .iter()
        .find(|tunnel| tunnel.provider == "tailscale" && tunnel.url.is_none())
        .unwrap();
    assert_eq!(retry.role, "retrying");
    assert_eq!(retry.state, "reconnecting");
    assert_eq!(retry.death_count, 1);
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
    assert_eq!(state.endpoint_leases.len(), 8);
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
async fn stale_endpoint_cleanup_preserves_same_url_replacement_trust() {
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
    assert!(recycle_revoked_endpoint(&mut state, &mut tunnels, url, &auth, &monitor).await);
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

    assert!(state.retain_stable_alias(TunnelProvider::Tailscale, old_url));
    publish_verified_endpoint(&auth, &monitor, "tailscale", new_url);
    let active = registered_fixture(&auth, TunnelProvider::Tailscale, new_url.to_owned());
    state.connected(&active, &auth);
    assert!(state.endpoint_leases.contains_key(old_url));
    let mut tunnels = vec![active];
    assert!(recycle_revoked_endpoint(&mut state, &mut tunnels, old_url, &auth, &monitor).await);
    assert!(auth.public_url_epoch(old_url).is_none());
    assert!(auth.public_url_epoch(new_url).is_some());
    assert!(!state.retained_stable_aliases.contains_key(old_url));
    assert!(!state.endpoint_leases.contains_key(old_url));
    assert!(state.pending_respawns.is_empty());
    assert_eq!(tunnels.len(), 1);
    assert!(auth.public_url_epoch(new_url).is_some());
}

#[tokio::test]
async fn retained_aliases_receive_instance_matched_probes_without_a_provider_child() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let router = axum::Router::new().route(
        "/healthz/probe",
        axum::routing::get(|| async {
            axum::Json(serde_json::json!({"ok":true,"instance_id":"retained-probe-test"}))
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let mut state = TunnelControlState::default();
    assert!(state.retain_stable_alias(TunnelProvider::Tailscale, &url));
    state.endpoint_leases.insert(
        url.clone(),
        EndpointHealthLease::verified(
            Instant::now() - ENDPOINT_PROBE_INTERVAL - Duration::from_secs(1),
        ),
    );
    let (tx, mut rx) = mpsc::channel(2);
    schedule_endpoint_probes(
        &mut state.endpoint_leases,
        &[],
        &state.retained_stable_aliases,
        "retained-probe-test",
        &tx,
    );
    schedule_endpoint_probes(
        &mut state.endpoint_leases,
        &[],
        &state.retained_stable_aliases,
        "retained-probe-test",
        &tx,
    );
    let result = tokio::time::timeout(Duration::from_secs(12), rx.recv()).await;
    server.abort();
    let event = result.expect("retained alias probe must finish").unwrap();
    assert_eq!(event.public_url, url);
    assert_eq!(event.lease_epoch, state.endpoint_leases[&url].epoch());
    assert!(event.result.is_ok(), "{:?}", event.result);
    assert!(
        rx.try_recv().is_err(),
        "same lease must remain single-flight"
    );
}

#[test]
fn retained_alias_keeps_concurrent_runtime_running_without_a_child() {
    let auth = Arc::new(AuthState::new("http://127.0.0.1:8765".to_owned()));
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    let url_slot = Arc::new(RwLock::new("http://127.0.0.1:8765".to_owned()));
    let display = EndpointDisplay::new(auth.clone(), monitor.clone(), url_slot.clone());
    let mut state = TunnelControlState::default();
    let url = "https://stable.example";
    publish_verified_endpoint(&auth, &monitor, "tailscale", url);
    assert!(state.retain_stable_alias(TunnelProvider::Tailscale, url));

    refresh_endpoint_display(&state, &[], &display, "http://127.0.0.1:8765");

    let status = monitor.connection_status();
    assert_eq!(status.public_endpoint.as_deref(), Some("concurrent"));
    assert_eq!(status.tunnel_running, Some(true));
    assert_eq!(status.public_url_healthy, Some(true));
    assert_eq!(auth.public_url(), url);
    assert_eq!(url_slot.read().unwrap().as_str(), url);
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
fn provider_retry_due_respects_scheduled_deadline() {
    let now = Instant::now();
    let due = now + Duration::from_secs(60);
    assert!(!provider_retry_due(due, now + Duration::from_secs(59)));
    assert!(provider_retry_due(due, now + Duration::from_secs(60)));
    assert!(provider_retry_due(due, now + Duration::from_secs(120)));
}

#[test]
fn stale_endpoint_probe_result_cannot_mutate_a_replacement_lease() {
    let url = "https://endpoint.example".to_owned();
    let start = Instant::now();
    let old_lease = EndpointHealthLease::verified(start);
    let old_epoch = old_lease.epoch();
    let replacement = EndpointHealthLease::verified(start + Duration::from_secs(1));
    let replacement_epoch = replacement.epoch();
    assert_ne!(old_epoch, replacement_epoch);

    let mut leases = HashMap::from([(url.clone(), replacement)]);
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    monitor.register_tunnel("pinggy", &url);

    assert!(!handle_endpoint_probe(
        &mut leases,
        &[],
        EndpointProbeEvent {
            public_url: url.clone(),
            lease_epoch: old_epoch,
            result: Err("stale probe failure".to_owned()),
        },
        &monitor,
    ));
    assert_eq!(leases[&url].failures(), 0);

    assert!(!handle_endpoint_probe(
        &mut leases,
        &[],
        EndpointProbeEvent {
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
    let mut lease = EndpointHealthLease::verified(now);
    lease.quarantine(now - policy::ENDPOINT_RECOVERY_GRACE);
    let epoch = lease.epoch();
    let mut leases = HashMap::from([(url.clone(), lease)]);
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    monitor.register_tunnel("tailscale", &url);
    assert!(handle_endpoint_probe(
        &mut leases,
        &[],
        EndpointProbeEvent {
            public_url: url.clone(),
            lease_epoch: epoch,
            result: Err("endpoint still unreachable".to_owned()),
        },
        &monitor,
    ));
    assert!(!leases[&url].eligible(Instant::now()));
}

#[test]
fn endpoint_lease_outlives_the_bounded_health_probe_queue() {
    let max_endpoints = TunnelProvider::auto_candidates().len() + MAX_RETAINED_STABLE_ALIASES;
    let waves = max_endpoints.div_ceil(crate::tunnel::PUBLIC_HEALTH_PARALLELISM);
    let worst_case =
        Duration::from_secs((crate::tunnel::PUBLIC_HEALTH_TIMEOUT.as_secs() + 1) * waves as u64);
    assert!(ENDPOINT_LEASE_TTL > worst_case);
}

#[test]
fn endpoint_health_lease_needs_two_failures_and_expires_without_refresh() {
    let start = Instant::now();
    let mut lease = EndpointHealthLease::verified(start);
    assert!(lease.eligible(start + ENDPOINT_LEASE_TTL - Duration::from_secs(1)));
    assert!(!lease.begin_probe(start + ENDPOINT_PROBE_INTERVAL - Duration::from_secs(1)));
    assert!(lease.begin_probe(start + ENDPOINT_PROBE_INTERVAL));
    assert!(!lease.record_failure(start + ENDPOINT_PROBE_INTERVAL));
    assert_eq!(lease.failures(), 1);
    let retry_due = start + ENDPOINT_PROBE_INTERVAL + ENDPOINT_RETRY_INTERVAL;
    assert!(lease.eligible(retry_due - Duration::from_secs(1)));
    assert!(!lease.begin_probe(retry_due - Duration::from_secs(1)));
    assert!(lease.begin_probe(retry_due));
    assert!(!lease.record_failure(retry_due));
    assert!(!lease.eligible(retry_due));

    let recovered_at = retry_due + Duration::from_secs(1);
    assert!(!lease.record_success(recovered_at));
    assert!(!lease.eligible(recovered_at));
    let recovered_at = recovered_at + ENDPOINT_RETRY_INTERVAL;
    assert!(lease.record_success(recovered_at));
    assert!(lease.eligible(recovered_at));
    assert!(!lease.eligible(recovered_at + ENDPOINT_LEASE_TTL + Duration::from_secs(1)));
}

#[test]
fn quarantine_keeps_the_url_for_bounded_recovery_and_resets_interrupted_successes() {
    let now = Instant::now();
    let mut lease = EndpointHealthLease::verified(now);
    lease.quarantine(now);
    assert!(!lease.record_failure(now + policy::ENDPOINT_RECOVERY_GRACE - Duration::from_secs(1)));
    assert!(!lease.eligible(now));
    assert!(!lease.record_success(now + Duration::from_secs(2)));
    assert!(!lease.record_failure(now + Duration::from_secs(3)));
    assert!(!lease.record_success(now + Duration::from_secs(4)));
    assert!(!lease.eligible(now + Duration::from_secs(4)));
    assert!(lease.record_success(now + Duration::from_secs(5)));
    assert!(lease.eligible(now + Duration::from_secs(5)));
    lease.quarantine(now + Duration::from_secs(10));
    assert!(!lease.record_failure(now + policy::ENDPOINT_RECOVERY_GRACE));
    assert!(lease.record_failure(now + Duration::from_secs(10) + policy::ENDPOINT_RECOVERY_GRACE));
}

#[test]
fn endpoint_observability_does_not_claim_recovery_on_the_first_success() {
    let now = Instant::now();
    let url = "https://recovering.example".to_owned();
    let mut lease = EndpointHealthLease::verified(now);
    lease.quarantine(now);
    let mut leases = HashMap::from([(url.clone(), lease)]);
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    monitor.register_tunnel("cloudflare", &url);
    for (expected_eligible, expected_state) in [(false, "quarantined"), (true, "verified")] {
        let epoch = leases[&url].epoch();
        assert!(!handle_endpoint_probe(
            &mut leases,
            &[],
            EndpointProbeEvent {
                public_url: url.clone(),
                lease_epoch: epoch,
                result: Ok(()),
            },
            &monitor
        ));
        assert_eq!(leases[&url].eligible(Instant::now()), expected_eligible);
        assert_eq!(monitor.connection_status().tunnels[0].state, expected_state);
    }
}

#[test]
fn reconnect_rate_limit_preserves_multiline_diagnostics_and_cooldown() {
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    let mut state = TunnelControlState::default();
    let before = Instant::now();
    state.reconnect_failed(TunnelProvider::Cloudflare,
        "cloudflared exited before producing a public URL:\nquick tunnel provisioning failed with status 429: error code: 1015",
        &[], &monitor);
    assert_eq!(state.pending_respawns.len(), 1);
    assert!(state.pending_respawns[0].1 >= before + Duration::from_secs(300));
    let status = monitor.connection_status();
    let retry = &status.tunnels[0];
    assert!(retry.circuit_open);
    assert_eq!(retry.role, "retrying");
    assert!(retry.retry_in_seconds.unwrap() >= 299);
}

#[tokio::test]
async fn concurrent_endpoints_probe_independently_and_slow_health_can_recover() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let router = axum::Router::new()
        .route(
            "/fast/healthz/probe",
            axum::routing::get(|| async {
                axum::Json(serde_json::json!({"ok":true,"instance_id":"parallel"}))
            }),
        )
        .route(
            "/slow/healthz/probe",
            axum::routing::get(|| async {
                tokio::time::sleep(Duration::from_secs(4)).await;
                axum::Json(serde_json::json!({"ok":true,"instance_id":"parallel"}))
            }),
        );
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let urls = [format!("{origin}/slow"), format!("{origin}/fast")];
    let mut state = TunnelControlState::default();
    let auth = AuthState::new(origin.clone());
    let monitor = TaskMonitor::new(["demo".to_owned()]);
    let tunnels = urls
        .iter()
        .map(|url| {
            publish_verified_endpoint(&auth, &monitor, "test", url);
            let tunnel = registered_fixture(&auth, TunnelProvider::Cloudflare, url.clone());
            state.connected(&tunnel, &auth);
            state.endpoint_leases.insert(
                url.clone(),
                EndpointHealthLease::verified(Instant::now() - ENDPOINT_PROBE_INTERVAL),
            );
            tunnel
        })
        .collect::<Vec<_>>();
    let (tx, mut rx) = mpsc::channel(4);
    schedule_endpoint_probes(
        &mut state.endpoint_leases,
        &tunnels,
        &HashMap::new(),
        "parallel",
        &tx,
    );
    let fast = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(fast.public_url, urls[1]);
    assert!(fast.result.is_ok(), "{:?}", fast.result);
    let slow = tokio::time::timeout(Duration::from_secs(8), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(slow.public_url, urls[0]);
    assert!(
        slow.result.is_ok(),
        "a healthy endpoint slower than 3s must pass: {:?}",
        slow.result
    );
    server.abort();
}
