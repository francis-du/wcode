use crate::auth::AuthState;
use crate::monitor::{OperatorMessageKind, TaskMonitor};
use crate::tunnel::{
    check_public_endpoint, public_endpoint_health_loop, spawn_tunnel_supervisor, ActiveTunnel,
    TunnelEvent, TunnelProvider,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};

#[path = "tunnel_policy.rs"]
mod policy;
#[cfg(test)]
use policy::{
    best_standby_score, reconnect_backoff_seconds, reconnect_delay_seconds, STANDBY_PROBE_INTERVAL,
};
pub(super) use policy::{
    best_verified_standby, dead_tunnel_index, provider_circuit_open, provider_retry_due,
    record_dead_provider, record_recovered_provider, standby_lease_age, standby_revoked,
    StandbyHealthLease, STANDBY_LEASE_TTL, STANDBY_RETRY_INTERVAL,
};

#[derive(Debug)]
pub(super) struct StandbyProbeEvent {
    pub(super) public_url: String,
    pub(super) lease_epoch: u64,
    pub(super) result: Result<(), String>,
}

#[derive(Clone)]
pub(super) struct TunnelSpawnContext {
    local_url: String,
    instance_id: String,
    install_missing: bool,
    monitor: TaskMonitor,
    auth: Arc<AuthState>,
    forward_tx: mpsc::Sender<TunnelEvent>,
}

impl TunnelSpawnContext {
    pub(super) fn new(
        local_url: String,
        instance_id: String,
        install_missing: bool,
        monitor: TaskMonitor,
        auth: Arc<AuthState>,
        forward_tx: mpsc::Sender<TunnelEvent>,
    ) -> Self {
        Self {
            local_url,
            instance_id,
            install_missing,
            monitor,
            auth,
            forward_tx,
        }
    }
}

pub(super) struct PrimaryRuntime {
    auth: Arc<AuthState>,
    monitor: TaskMonitor,
    url_slot: Arc<RwLock<String>>,
    stop: watch::Sender<bool>,
    health_task: Option<tokio::task::JoinHandle<()>>,
}

impl PrimaryRuntime {
    pub(super) fn new(
        auth: Arc<AuthState>,
        monitor: TaskMonitor,
        url_slot: Arc<RwLock<String>>,
        stop: watch::Sender<bool>,
    ) -> Self {
        Self {
            auth,
            monitor,
            url_slot,
            stop,
            health_task: None,
        }
    }

    pub(super) async fn shutdown(&mut self) {
        let _ = self.stop.send(true);
        if let Some(task) = self.health_task.take() {
            task.abort();
            let _ = task.await;
        }
    }
}

/// Publish only after the supervisor has verified instance-matched health.
/// Register trust before exposing the URL to setup pages or operator output.
pub(super) fn spawn_initial_supervisor(
    selected: TunnelProvider,
    context: TunnelSpawnContext,
    settled_tx: watch::Sender<bool>,
    url_slot: Arc<RwLock<String>>,
) {
    tokio::spawn(async move {
        let mut events = spawn_tunnel_supervisor(
            selected,
            &context.local_url,
            &context.instance_id,
            context.install_missing,
            context.monitor.clone(),
            true,
        );
        let mut first = true;
        while let Some(event) = events.recv().await {
            if let TunnelEvent::Connected(active) = &event {
                publish_verified_endpoint(
                    &context.auth,
                    &context.monitor,
                    active.provider_label(),
                    active.public_url(),
                );
                if first {
                    let public_url = active.public_url().to_owned();
                    context.auth.set_public_url(public_url.clone());
                    *url_slot.write().expect("public URL lock poisoned") = public_url;
                    first = false;
                    let _ = settled_tx.send(true);
                }
            }
            if context.forward_tx.send(event).await.is_err() {
                return;
            }
        }
    });
}

pub(super) fn spawn_reconnect_attempt(provider: TunnelProvider, context: TunnelSpawnContext) {
    tokio::spawn(async move {
        let mut events = spawn_tunnel_supervisor(
            provider,
            &context.local_url,
            &context.instance_id,
            context.install_missing,
            context.monitor.clone(),
            false,
        );
        while let Some(event) = events.recv().await {
            if let TunnelEvent::Connected(active) = &event {
                publish_verified_endpoint(
                    &context.auth,
                    &context.monitor,
                    active.provider_label(),
                    active.public_url(),
                );
            }
            if context.forward_tx.send(event).await.is_err() {
                return;
            }
        }
    });
}

pub(super) fn spawn_standby_probe(
    public_url: String,
    lease_epoch: u64,
    instance_id: String,
    sender: mpsc::Sender<StandbyProbeEvent>,
) {
    tokio::spawn(async move {
        let result = check_public_endpoint(&public_url, &instance_id).await;
        let _ = sender
            .send(StandbyProbeEvent {
                public_url,
                lease_epoch,
                result,
            })
            .await;
    });
}

pub(super) fn schedule_standby_probes(
    leases: &mut HashMap<String, StandbyHealthLease>,
    tunnels: &[ActiveTunnel],
    primary_url: Option<&str>,
    instance_id: &str,
    sender: &mpsc::Sender<StandbyProbeEvent>,
) {
    let now = Instant::now();
    for tunnel in tunnels {
        if primary_url == Some(tunnel.public_url()) {
            continue;
        }
        let lease = leases
            .entry(tunnel.public_url().to_owned())
            .or_insert_with(|| StandbyHealthLease::verified(now));
        if lease.begin_probe(now) {
            spawn_standby_probe(
                tunnel.public_url().to_owned(),
                lease.epoch(),
                instance_id.to_owned(),
                sender.clone(),
            );
        }
    }
}

pub(super) fn handle_standby_probe(
    leases: &mut HashMap<String, StandbyHealthLease>,
    tunnels: &[ActiveTunnel],
    event: StandbyProbeEvent,
    monitor: &TaskMonitor,
) {
    let Some(lease) = leases.get_mut(&event.public_url) else {
        return;
    };
    if lease.epoch() != event.lease_epoch {
        return;
    }
    let provider = tunnels
        .iter()
        .find(|tunnel| tunnel.public_url() == event.public_url)
        .map(ActiveTunnel::provider_label)
        .unwrap_or("unknown");
    let now = Instant::now();
    match event.result {
        Ok(()) => {
            let recovered = lease.record_success(now);
            monitor.mark_tunnel_standby_probe(&event.public_url, true, 0, false);
            if recovered {
                monitor.operator_message(
                    OperatorMessageKind::Success,
                    "tunnel",
                    format!("{provider} standby health lease recovered"),
                );
            }
        }
        Err(error) => {
            let revoked = lease.record_failure(now);
            monitor.mark_tunnel_standby_probe(&event.public_url, false, lease.failures(), revoked);
            let detail = if revoked {
                format!(
                    "{provider} standby lease revoked after {} failed checks · {error}",
                    lease.failures()
                )
            } else {
                format!(
                    "{provider} standby probe failed {}/2 · retrying in {}s · {error}",
                    lease.failures(),
                    STANDBY_RETRY_INTERVAL.as_secs()
                )
            };
            monitor.operator_message(OperatorMessageKind::Warning, "tunnel", detail);
        }
    }
}

pub(super) fn activate_primary(active: &ActiveTunnel, runtime: &mut PrimaryRuntime) {
    let public_url = active.public_url().to_owned();
    *runtime.url_slot.write().expect("public URL lock poisoned") = public_url.clone();
    runtime.auth.set_public_url(public_url.clone());
    runtime
        .monitor
        .mark_public_endpoint("quick-tunnel", Some(true));
    runtime.monitor.mark_public_url_verified();
    runtime.monitor.mark_tunnel_primary(&public_url);
    if let Some(task) = runtime.health_task.take() {
        task.abort();
    }
    runtime.health_task = Some(tokio::spawn(public_endpoint_health_loop(
        public_url,
        runtime.auth.instance_id().to_owned(),
        runtime.monitor.clone(),
        runtime.stop.subscribe(),
    )));
}

pub(super) fn deactivate_primary(
    runtime: &mut PrimaryRuntime,
    local_url: &str,
    reason: String,
    tunnel_running: bool,
) {
    if let Some(task) = runtime.health_task.take() {
        task.abort();
    }
    *runtime.url_slot.write().expect("public URL lock poisoned") = local_url.to_owned();
    runtime.auth.set_public_url(local_url.to_owned());
    runtime
        .monitor
        .mark_public_endpoint("pending", Some(tunnel_running));
    runtime.monitor.mark_public_url_pending(Some(reason));
}

pub(super) fn promote_best_standby(
    leases: &HashMap<String, StandbyHealthLease>,
    tunnels: &[ActiveTunnel],
    primary_url: &mut Option<String>,
    runtime: &mut PrimaryRuntime,
) -> bool {
    let Some(index) = best_verified_standby(leases, tunnels, None) else {
        return false;
    };
    let active = &tunnels[index];
    let public_url = active.public_url().to_owned();
    let lease_age = standby_lease_age(leases, &public_url).unwrap_or_default();
    let provider = active.provider_label();
    let uptime = active.connected_for();
    *primary_url = Some(public_url);
    activate_primary(active, runtime);
    runtime.monitor.operator_message(
        OperatorMessageKind::Success,
        "tunnel",
        format!(
            "failover to {} · standby lease {}s old · uptime {}s",
            provider,
            lease_age.as_secs(),
            uptime.as_secs()
        ),
    );
    true
}

pub(super) fn publish_verified_endpoint(
    auth: &AuthState,
    monitor: &TaskMonitor,
    provider: &str,
    public_url: &str,
) {
    auth.register_public_url(public_url.to_owned());
    monitor.register_tunnel(provider, public_url);
    monitor.operator_message(
        OperatorMessageKind::Success,
        "tunnel",
        format!("{provider} connected · {public_url}"),
    );
}

pub(super) fn schedule_provider_retry(
    death_counts: &mut HashMap<TunnelProvider, u32>,
    pending_respawns: &mut Vec<(TunnelProvider, Instant)>,
    provider: TunnelProvider,
    cause: &str,
    monitor: &TaskMonitor,
) {
    pending_respawns.retain(|(pending, _)| *pending != provider);
    let delay = record_dead_provider(death_counts, provider);
    let deaths = death_counts.get(&provider).copied().unwrap_or(1);
    let circuit_open = provider_circuit_open(deaths);
    monitor.mark_tunnel_retry(
        provider.label(),
        deaths,
        circuit_open,
        Duration::from_secs(delay),
    );
    monitor.operator_message(
        OperatorMessageKind::Warning,
        "tunnel",
        format!(
            "{} {cause} · retry in {delay}s · death #{deaths}{}",
            provider.label(),
            if circuit_open { " · circuit open" } else { "" }
        ),
    );
    pending_respawns.push((provider, Instant::now() + Duration::from_secs(delay)));
}

pub(super) fn reset_stable_provider_backoff(
    tunnels: &[ActiveTunnel],
    death_counts: &mut HashMap<TunnelProvider, u32>,
    stable_after: Duration,
    monitor: &TaskMonitor,
) {
    for tunnel in tunnels {
        if tunnel.stable_for(stable_after)
            && record_recovered_provider(death_counts, tunnel.provider())
        {
            monitor.mark_tunnel_stable(tunnel.provider_label());
            monitor.operator_message(
                OperatorMessageKind::Info,
                "tunnel",
                format!(
                    "{} stable for {}s · reconnect backoff reset",
                    tunnel.provider_label(),
                    stable_after.as_secs()
                ),
            );
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/app/tunnel_lifecycle.rs"]
mod tests;
