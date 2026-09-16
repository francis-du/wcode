use crate::auth::AuthState;
use crate::monitor::{OperatorMessageKind, TaskMonitor};
use crate::tunnel::{
    check_public_endpoint_resilient, public_endpoint_health_loop, spawn_tunnel_supervisor,
    ActiveTunnel, TunnelEvent, TunnelProvider,
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
    record_dead_provider, record_recovered_provider, standby_lease_age, StandbyHealthLease,
    STANDBY_LEASE_TTL, STANDBY_RETRY_INTERVAL,
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
    dev_tunnel_id: Option<String>,
    monitor: TaskMonitor,
    auth: Arc<AuthState>,
    forward_tx: mpsc::Sender<TunnelEvent>,
}

impl TunnelSpawnContext {
    pub(super) fn new(
        local_url: String,
        instance_id: String,
        install_missing: bool,
        dev_tunnel_id: Option<String>,
        monitor: TaskMonitor,
        auth: Arc<AuthState>,
        forward_tx: mpsc::Sender<TunnelEvent>,
    ) -> Self {
        Self {
            local_url,
            instance_id,
            install_missing,
            dev_tunnel_id,
            monitor,
            auth,
            forward_tx,
        }
    }
}

#[derive(Default)]
pub(super) struct TunnelControlState {
    pub(super) primary_url: Option<String>,
    pub(super) standby_leases: HashMap<String, StandbyHealthLease>,
    pending_respawns: Vec<(TunnelProvider, Instant)>,
    death_counts: HashMap<TunnelProvider, u32>,
    retained_stable_aliases: HashMap<String, TunnelProvider>,
    endpoint_epochs: HashMap<String, u64>,
}

impl TunnelControlState {
    pub(super) fn connected(&mut self, active: &ActiveTunnel, auth: &AuthState) -> bool {
        let Some(epoch) = active.endpoint_epoch else {
            return false;
        };
        if auth.public_url_epoch(active.public_url()) != Some(epoch) {
            return false;
        }
        let provider = active.provider();
        let public_url = active.public_url();
        // A different reconnected URL replaces the advertised primary, not the
        // old alias's trust. That alias keeps its own bounded health lease.
        if self.primary_url.as_ref().is_some_and(|previous| {
            previous != public_url && self.retained_stable_aliases.get(previous) == Some(&provider)
        }) {
            self.primary_url = None;
        }
        self.retained_stable_aliases.remove(public_url);
        self.endpoint_epochs.insert(public_url.to_owned(), epoch);
        self.pending_respawns
            .retain(|(pending, _)| *pending != provider);
        self.standby_leases.insert(
            public_url.to_owned(),
            StandbyHealthLease::verified(Instant::now()),
        );
        true
    }

    fn retain_stable_alias(&mut self, provider: TunnelProvider, public_url: &str) -> bool {
        const MAX_RETAINED_STABLE_ALIASES: usize = 8;
        if !self.retained_stable_aliases.contains_key(public_url)
            && self.retained_stable_aliases.len() >= MAX_RETAINED_STABLE_ALIASES
        {
            return false;
        }
        self.retained_stable_aliases
            .insert(public_url.to_owned(), provider);
        self.standby_leases.insert(
            public_url.to_owned(),
            StandbyHealthLease::verified(Instant::now()),
        );
        true
    }

    fn withdraw_endpoint(
        &mut self,
        public_url: &str,
        auth: &AuthState,
        monitor: &TaskMonitor,
    ) -> bool {
        // A worker can publish a same-URL replacement while old cleanup awaits
        // process exit. Compare under the registry lock, never revoke by URL alone.
        let removed = self
            .endpoint_epochs
            .remove(public_url)
            .is_some_and(|epoch| auth.unregister_public_url_if_epoch(public_url, epoch));
        if removed {
            monitor.remove_tunnel(public_url);
        }
        removed
    }

    pub(super) fn reconnect_failed(
        &mut self,
        provider: TunnelProvider,
        error: &str,
        tunnels: &[ActiveTunnel],
        monitor: &TaskMonitor,
    ) {
        if tunnels.iter().any(|tunnel| tunnel.provider() == provider) {
            return;
        }
        // Provider startup failure is not endpoint-health evidence. Retained
        // aliases are supervised independently, even after another URL connects.
        let cause = format!(
            "half-open reconnect failed · {}",
            error.lines().next().unwrap_or("unknown error")
        );
        schedule_provider_retry(
            &mut self.death_counts,
            &mut self.pending_respawns,
            provider,
            &cause,
            self.retained_stable_aliases
                .values()
                .any(|retained| *retained == provider),
            monitor,
        );
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
            context.dev_tunnel_id.as_deref(),
            context.monitor.clone(),
            true,
        );
        let mut first = true;
        while let Some(mut event) = events.recv().await {
            if let TunnelEvent::Connected(active) = &mut event {
                active.endpoint_epoch = Some(publish_verified_endpoint(
                    &context.auth,
                    &context.monitor,
                    active.provider_label(),
                    active.public_url(),
                ));
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
            context.dev_tunnel_id.as_deref(),
            context.monitor.clone(),
            false,
        );
        while let Some(mut event) = events.recv().await {
            if let TunnelEvent::Connected(active) = &mut event {
                active.endpoint_epoch = Some(publish_verified_endpoint(
                    &context.auth,
                    &context.monitor,
                    active.provider_label(),
                    active.public_url(),
                ));
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
        let result = check_public_endpoint_resilient(&public_url, &instance_id).await;
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
    retained: &HashMap<String, TunnelProvider>,
    primary_url: Option<&str>,
    instance_id: &str,
    sender: &mpsc::Sender<StandbyProbeEvent>,
) {
    let now = Instant::now();
    for public_url in tunnels
        .iter()
        .map(ActiveTunnel::public_url)
        .chain(retained.keys().map(String::as_str))
    {
        if primary_url == Some(public_url) {
            continue;
        }
        let Some(lease) = leases.get_mut(public_url) else {
            continue;
        };
        if lease.begin_probe(now) {
            spawn_standby_probe(
                public_url.to_owned(),
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
) -> bool {
    let Some(lease) = leases.get_mut(&event.public_url) else {
        return false;
    };
    if lease.epoch() != event.lease_epoch {
        return false;
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
            false
        }
        Err(error) => {
            let revoked = lease.record_failure(now);
            monitor.mark_tunnel_standby_probe(&event.public_url, false, lease.failures(), revoked);
            let detail = if revoked {
                format!(
                    "{provider} standby revoked after {} failed checks · recycling provider · {error}",
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
            revoked
        }
    }
}

pub(super) async fn recycle_revoked_standby(
    state: &mut TunnelControlState,
    tunnels: &mut Vec<ActiveTunnel>,
    public_url: &str,
    auth: &AuthState,
    monitor: &TaskMonitor,
) -> bool {
    if state.primary_url.as_deref() == Some(public_url) {
        return false;
    }
    let index = tunnels
        .iter()
        .position(|tunnel| tunnel.public_url() == public_url);
    let provider = index
        .map(|index| tunnels[index].provider())
        .or_else(|| state.retained_stable_aliases.get(public_url).copied());
    let Some(provider) = provider else {
        return false;
    };
    if let Some(index) = index {
        let mut tunnel = tunnels.remove(index);
        tunnel.stop().await;
    }
    state.standby_leases.remove(public_url);
    state.retained_stable_aliases.remove(public_url);
    let withdrawn = state.withdraw_endpoint(public_url, auth, monitor);
    let replacement_published = !withdrawn && auth.public_url_epoch(public_url).is_some();
    if !replacement_published && !tunnels.iter().any(|tunnel| tunnel.provider() == provider) {
        schedule_provider_retry(
            &mut state.death_counts,
            &mut state.pending_respawns,
            provider,
            "endpoint stayed unreachable after quarantine",
            false,
            monitor,
        );
    }
    true
}

pub(super) fn quarantine_endpoint(
    leases: &mut HashMap<String, StandbyHealthLease>,
    public_url: &str,
    monitor: &TaskMonitor,
) {
    let now = Instant::now();
    let lease = leases
        .entry(public_url.to_owned())
        .or_insert_with(|| StandbyHealthLease::verified(now));
    lease.quarantine(now);
    monitor.mark_tunnel_standby_probe(public_url, false, lease.failures(), true);
}

pub(super) async fn stable_alias_survives_provider_exit(
    provider: TunnelProvider,
    public_url: &str,
    instance_id: &str,
) -> bool {
    provider.has_stable_endpoint()
        && check_public_endpoint_resilient(public_url, instance_id)
            .await
            .is_ok()
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
) -> u64 {
    let epoch = auth.register_public_url(public_url.to_owned());
    monitor.register_tunnel(provider, public_url);
    monitor.operator_message(
        OperatorMessageKind::Success,
        "tunnel",
        format!("{provider} connected · {public_url}"),
    );
    epoch
}

pub(super) fn schedule_provider_retry(
    death_counts: &mut HashMap<TunnelProvider, u32>,
    pending_respawns: &mut Vec<(TunnelProvider, Instant)>,
    provider: TunnelProvider,
    cause: &str,
    retain_endpoint: bool,
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
        retain_endpoint,
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

pub(super) async fn maintain_tunnels(
    state: &mut TunnelControlState,
    tunnels: &mut Vec<ActiveTunnel>,
    primary_runtime: &mut PrimaryRuntime,
    spawn_context: &TunnelSpawnContext,
    standby_probe_tx: &mpsc::Sender<StandbyProbeEvent>,
) {
    let auth = spawn_context.auth.as_ref();
    let monitor = &spawn_context.monitor;
    let local_url = spawn_context.local_url.as_str();
    reset_stable_provider_backoff(
        tunnels,
        &mut state.death_counts,
        Duration::from_secs(30),
        monitor,
    );
    schedule_standby_probes(
        &mut state.standby_leases,
        tunnels,
        &state.retained_stable_aliases,
        state.primary_url.as_deref(),
        auth.instance_id(),
        standby_probe_tx,
    );
    let retry_now = Instant::now();
    let due_providers = state
        .pending_respawns
        .iter()
        .filter_map(|(provider, due)| provider_retry_due(*due, retry_now).then_some(*provider))
        .collect::<Vec<_>>();
    state
        .pending_respawns
        .retain(|(_, due)| !provider_retry_due(*due, retry_now));
    for provider in due_providers {
        spawn_reconnect_attempt(provider, spawn_context.clone());
    }

    let primary_index = state
        .primary_url
        .as_ref()
        .and_then(|url| tunnels.iter().position(|tunnel| tunnel.public_url() == url));
    if primary_index.is_some() && monitor.connection_status().public_url_healthy == Some(false) {
        if let Some(index) = primary_index {
            let failed_url = tunnels[index].public_url().to_owned();
            quarantine_endpoint(&mut state.standby_leases, &failed_url, monitor);
            state.primary_url = None;
            deactivate_primary(
                primary_runtime,
                local_url,
                "primary endpoint quarantined after repeated health failures; probing for in-place recovery".to_owned(),
                !tunnels.is_empty(),
            );
            promote_best_standby(
                &state.standby_leases,
                tunnels,
                &mut state.primary_url,
                primary_runtime,
            );
        }
    }

    let primary_index = state
        .primary_url
        .as_ref()
        .and_then(|url| tunnels.iter().position(|tunnel| tunnel.public_url() == url));
    if primary_index.is_none() && monitor.connection_status().public_url_healthy == Some(false) {
        if let Some(primary_url) = state.primary_url.clone() {
            if state.retained_stable_aliases.remove(&primary_url).is_some() {
                state.standby_leases.remove(&primary_url);
                state.withdraw_endpoint(&primary_url, auth, monitor);
                state.primary_url = None;
                deactivate_primary(
                    primary_runtime,
                    local_url,
                    "retained stable endpoint became unreachable; trust revoked and failover resumed".to_owned(),
                    !tunnels.is_empty(),
                );
                promote_best_standby(
                    &state.standby_leases,
                    tunnels,
                    &mut state.primary_url,
                    primary_runtime,
                );
            }
        }
    }
    let primary_index = state
        .primary_url
        .as_ref()
        .and_then(|url| tunnels.iter().position(|tunnel| tunnel.public_url() == url));
    let dead_index = dead_tunnel_index(false, primary_index, tunnels.len(), |index| {
        matches!(tunnels[index].try_wait(), Ok(Some(_)) | Err(_))
    });
    if let Some(index) = dead_index {
        let reason = format!(
            "{} tunnel exited with {}",
            tunnels[index].provider_label(),
            tunnels[index]
                .try_wait()
                .ok()
                .flatten()
                .map(|status| status.to_string())
                .unwrap_or_else(|| "unknown status".to_owned())
        );
        let dead_provider = tunnels[index].provider();
        let dead_url = tunnels[index].public_url().to_owned();
        let was_primary = primary_index == Some(index);
        monitor.operator_message(
            OperatorMessageKind::Warning,
            "tunnel",
            format!("{reason}; respawning {}", dead_provider.label()),
        );
        let retain_stable_alias =
            stable_alias_survives_provider_exit(dead_provider, &dead_url, auth.instance_id()).await
                && state.retain_stable_alias(dead_provider, &dead_url);
        let mut dead = tunnels.remove(index);
        dead.stop().await;
        if !retain_stable_alias {
            state.standby_leases.remove(&dead_url);
        }
        if retain_stable_alias {
            monitor.operator_message(
                OperatorMessageKind::Info,
                "tunnel",
                format!(
                    "{} process exited but its stable endpoint still reaches this runtime; Host trust retained while reconnecting",
                    dead_provider.label()
                ),
            );
        } else {
            state.withdraw_endpoint(&dead_url, auth, monitor);
        }
        if was_primary && !retain_stable_alias {
            state.primary_url = None;
            deactivate_primary(
                primary_runtime,
                local_url,
                reason.clone(),
                !tunnels.is_empty(),
            );
        }
        if tunnels.is_empty() && !retain_stable_alias {
            monitor.mark_tunnel_stopped(reason.clone());
        }
        // A newer same-URL registration is already being attached. Do not
        // overwrite its telemetry or spawn another provider from stale cleanup.
        let replacement_published =
            !retain_stable_alias && auth.public_url_epoch(&dead_url).is_some();
        if !replacement_published {
            schedule_provider_retry(
                &mut state.death_counts,
                &mut state.pending_respawns,
                dead_provider,
                "runtime endpoint recycled",
                retain_stable_alias,
                monitor,
            );
        }
    }
    if state.primary_url.is_none() {
        promote_best_standby(
            &state.standby_leases,
            tunnels,
            &mut state.primary_url,
            primary_runtime,
        );
    }
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
