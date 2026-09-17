use crate::auth::AuthState;
use crate::monitor::{OperatorMessageKind, TaskMonitor};
use crate::tunnel::{
    check_public_endpoint, spawn_tunnel_supervisor, ActiveTunnel, TunnelEvent, TunnelProvider,
};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, watch};

const MAX_RETAINED_STABLE_ALIASES: usize = 8;

#[path = "tunnel_policy.rs"]
mod policy;
pub(super) use policy::{
    provider_circuit_open, provider_retry_due, record_dead_provider, record_recovered_provider,
    EndpointHealthLease, ENDPOINT_RETRY_INTERVAL,
};
#[cfg(test)]
use policy::{reconnect_backoff_seconds, reconnect_delay_seconds, ENDPOINT_PROBE_INTERVAL};

#[derive(Debug)]
pub(super) struct EndpointProbeEvent {
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
    pub(super) endpoint_leases: HashMap<String, EndpointHealthLease>,
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

        self.retained_stable_aliases.remove(public_url);
        self.endpoint_epochs.insert(public_url.to_owned(), epoch);
        self.pending_respawns
            .retain(|(pending, _)| *pending != provider);
        self.endpoint_leases.insert(
            public_url.to_owned(),
            EndpointHealthLease::verified(Instant::now()),
        );
        true
    }

    fn retain_stable_alias(&mut self, provider: TunnelProvider, public_url: &str) -> bool {
        if !self.retained_stable_aliases.contains_key(public_url)
            && self.retained_stable_aliases.len() >= MAX_RETAINED_STABLE_ALIASES
        {
            return false;
        }
        self.retained_stable_aliases
            .insert(public_url.to_owned(), provider);
        self.endpoint_leases
            .entry(public_url.to_owned())
            .or_insert_with(|| EndpointHealthLease::verified(Instant::now()));
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
        let cause = format!("half-open reconnect failed · {}", error);
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

// The display URL is a convenience for local links; it never determines
// which endpoints accept MCP, their health, or their lifecycle.
pub(super) struct EndpointDisplay {
    auth: Arc<AuthState>,
    monitor: TaskMonitor,
    url_slot: Arc<RwLock<String>>,
}
impl EndpointDisplay {
    pub(super) fn new(
        auth: Arc<AuthState>,
        monitor: TaskMonitor,
        url_slot: Arc<RwLock<String>>,
    ) -> Self {
        Self {
            auth,
            monitor,
            url_slot,
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
        loop {
            let event = tokio::select! {
                _ = context.forward_tx.closed() => return,
                event = events.recv() => event,
            };
            let Some(mut event) = event else { return };
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
        loop {
            let event = tokio::select! {
                _ = context.forward_tx.closed() => return,
                event = events.recv() => event,
            };
            let Some(mut event) = event else { return };
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

pub(super) fn spawn_endpoint_probe(
    public_url: String,
    lease_epoch: u64,
    instance_id: String,
    sender: mpsc::Sender<EndpointProbeEvent>,
) {
    tokio::spawn(async move {
        let result = tokio::select! {
            _ = sender.closed() => return,
            result = check_public_endpoint(&public_url, &instance_id) => result,
        };
        let _ = sender
            .send(EndpointProbeEvent {
                public_url,
                lease_epoch,
                result,
            })
            .await;
    });
}

pub(super) fn schedule_endpoint_probes(
    leases: &mut HashMap<String, EndpointHealthLease>,
    tunnels: &[ActiveTunnel],
    retained: &HashMap<String, TunnelProvider>,

    instance_id: &str,
    sender: &mpsc::Sender<EndpointProbeEvent>,
) {
    let now = Instant::now();
    for public_url in tunnels
        .iter()
        .map(ActiveTunnel::public_url)
        .chain(retained.keys().map(String::as_str))
    {
        let Some(lease) = leases.get_mut(public_url) else {
            continue;
        };
        if lease.begin_probe(now) {
            spawn_endpoint_probe(
                public_url.to_owned(),
                lease.epoch(),
                instance_id.to_owned(),
                sender.clone(),
            );
        }
    }
}

pub(super) fn handle_endpoint_probe(
    leases: &mut HashMap<String, EndpointHealthLease>,
    tunnels: &[ActiveTunnel],
    event: EndpointProbeEvent,
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
            let eligible = lease.eligible(now);
            monitor.mark_tunnel_endpoint_probe(
                &event.public_url,
                eligible,
                lease.failures(),
                !eligible,
            );
            if recovered {
                monitor.operator_message(
                    OperatorMessageKind::Success,
                    "tunnel",
                    format!("{provider} endpoint health lease recovered"),
                );
            }
            false
        }
        Err(error) => {
            let revoked = lease.record_failure(now);
            monitor.mark_tunnel_endpoint_probe(
                &event.public_url,
                false,
                lease.failures(),
                !lease.eligible(now),
            );
            let detail = if revoked {
                format!(
                    "{provider} endpoint revoked after {} failed checks · recycling provider · {error}",
                    lease.failures()
                )
            } else if !lease.eligible(now) {
                format!("{provider} endpoint quarantined · allowing in-place recovery · {error}")
            } else {
                format!(
                    "{provider} endpoint probe failed {}/2 · retrying in {}s · {error}",
                    lease.failures(),
                    ENDPOINT_RETRY_INTERVAL.as_secs()
                )
            };
            monitor.operator_message(OperatorMessageKind::Warning, "tunnel", detail);
            revoked
        }
    }
}

pub(super) async fn recycle_revoked_endpoint(
    state: &mut TunnelControlState,
    tunnels: &mut Vec<ActiveTunnel>,
    public_url: &str,
    auth: &AuthState,
    monitor: &TaskMonitor,
) -> bool {
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
    state.endpoint_leases.remove(public_url);
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

pub(super) fn refresh_endpoint_display(
    state: &TunnelControlState,
    tunnels: &[ActiveTunnel],
    display: &EndpointDisplay,
    local_url: &str,
) {
    let now = Instant::now();
    let current = display.auth.public_url();
    let live = |url: &str| {
        state
            .endpoint_leases
            .get(url)
            .is_some_and(|lease| lease.eligible(now))
    };
    // Prefer a stable address for copying, but all registered URLs remain active.
    let next = tunnels
        .iter()
        .filter(|t| live(t.public_url()))
        .min_by_key(|t| {
            (
                !t.provider().has_stable_endpoint(),
                t.public_url() != current,
            )
        })
        .map(|t| t.public_url().to_owned())
        .or_else(|| {
            state
                .retained_stable_aliases
                .keys()
                .filter(|url| live(url))
                .min()
                .cloned()
        });
    let healthy = next.is_some();
    let next = next.unwrap_or_else(|| local_url.to_owned());
    if current != next {
        display.auth.set_public_url(next.clone());
        *display.url_slot.write().expect("public URL lock poisoned") = next;
    }
    display.monitor.mark_public_endpoint(
        "concurrent",
        Some(!tunnels.is_empty() || !state.retained_stable_aliases.is_empty()),
    );
    display.monitor.mark_concurrent_health(healthy);
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
    let delay = record_dead_provider(death_counts, provider)
        .max(crate::tunnel::provider_failure_cooldown(provider, cause).as_secs());
    let deaths = death_counts.get(&provider).copied().unwrap_or(1);
    let circuit_open = provider_circuit_open(deaths) || delay >= 300;
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
    display: &EndpointDisplay,
    spawn_context: &TunnelSpawnContext,
    endpoint_probe_tx: &mpsc::Sender<EndpointProbeEvent>,
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
    schedule_endpoint_probes(
        &mut state.endpoint_leases,
        tunnels,
        &state.retained_stable_aliases,
        auth.instance_id(),
        endpoint_probe_tx,
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

    // Child exit is handled immediately; network checks run independently.
    let mut index = 0;
    while index < tunnels.len() {
        if !matches!(tunnels[index].try_wait(), Ok(Some(_)) | Err(_)) {
            index += 1;
            continue;
        }
        let provider = tunnels[index].provider();
        let url = tunnels[index].public_url().to_owned();
        // A previously verified stable endpoint keeps its existing bounded
        // lease while an independent worker checks it. Never block other lanes.
        let retain = provider.has_stable_endpoint()
            && state
                .endpoint_leases
                .get(&url)
                .is_some_and(|lease| lease.eligible(Instant::now()))
            && state.retain_stable_alias(provider, &url);
        let mut dead = tunnels.remove(index);
        dead.stop().await;
        if !retain {
            state.endpoint_leases.remove(&url);
            state.withdraw_endpoint(&url, auth, monitor);
        }
        let replaced = !retain && auth.public_url_epoch(&url).is_some();
        if !replaced {
            schedule_provider_retry(
                &mut state.death_counts,
                &mut state.pending_respawns,
                provider,
                "provider process exited",
                retain,
                monitor,
            );
        }
    }
    refresh_endpoint_display(state, tunnels, display, local_url);
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
