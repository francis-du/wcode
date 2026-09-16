use crate::tunnel::{ActiveTunnel, TunnelProvider};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub(in crate::app) const STANDBY_PROBE_INTERVAL: Duration = Duration::from_secs(15);
pub(in crate::app) const STANDBY_RETRY_INTERVAL: Duration = Duration::from_secs(2);
pub(in crate::app) const STANDBY_LEASE_TTL: Duration = Duration::from_secs(45);
const STANDBY_FAILURE_THRESHOLD: u8 = 2;
const CIRCUIT_BREAKER_THRESHOLD: u32 = 5;
const CIRCUIT_BREAKER_MIN_DELAY: u64 = 30;
static NEXT_STANDBY_LEASE_EPOCH: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub(in crate::app) struct StandbyHealthLease {
    epoch: u64,
    verified_at: Instant,
    next_probe_at: Instant,
    consecutive_failures: u8,
    in_flight: bool,
}

impl StandbyHealthLease {
    pub(in crate::app) fn verified(now: Instant) -> Self {
        Self {
            epoch: NEXT_STANDBY_LEASE_EPOCH.fetch_add(1, Ordering::Relaxed),
            verified_at: now,
            next_probe_at: now + STANDBY_PROBE_INTERVAL,
            consecutive_failures: 0,
            in_flight: false,
        }
    }

    pub(in crate::app) fn begin_probe(&mut self, now: Instant) -> bool {
        if self.in_flight || now < self.next_probe_at {
            return false;
        }
        self.epoch = NEXT_STANDBY_LEASE_EPOCH.fetch_add(1, Ordering::Relaxed);
        self.in_flight = true;
        true
    }

    pub(in crate::app) fn record_success(&mut self, now: Instant) -> bool {
        let recovered = self.consecutive_failures > 0 || !self.eligible(now);
        self.verified_at = now;
        self.next_probe_at = now + STANDBY_PROBE_INTERVAL;
        self.consecutive_failures = 0;
        self.in_flight = false;
        self.epoch = NEXT_STANDBY_LEASE_EPOCH.fetch_add(1, Ordering::Relaxed);
        recovered
    }

    pub(in crate::app) fn record_failure(&mut self, now: Instant) -> bool {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.next_probe_at = now + STANDBY_RETRY_INTERVAL;
        self.in_flight = false;
        self.epoch = NEXT_STANDBY_LEASE_EPOCH.fetch_add(1, Ordering::Relaxed);
        self.consecutive_failures >= STANDBY_FAILURE_THRESHOLD
    }

    pub(in crate::app) fn quarantine(&mut self, now: Instant) {
        // Invalidate a probe started before quarantine; its success cannot
        // certify a recovery that has not actually been checked yet.
        self.epoch = NEXT_STANDBY_LEASE_EPOCH.fetch_add(1, Ordering::Relaxed);
        self.consecutive_failures = STANDBY_FAILURE_THRESHOLD;
        self.next_probe_at = now;
        self.in_flight = false;
    }

    pub(in crate::app) fn eligible(&self, now: Instant) -> bool {
        self.consecutive_failures < STANDBY_FAILURE_THRESHOLD
            && now.saturating_duration_since(self.verified_at) <= STANDBY_LEASE_TTL
    }

    pub(in crate::app) fn failures(&self) -> u8 {
        self.consecutive_failures
    }

    pub(in crate::app) fn age(&self, now: Instant) -> Duration {
        now.saturating_duration_since(self.verified_at)
    }

    pub(in crate::app) fn epoch(&self) -> u64 {
        self.epoch
    }
}

pub(in crate::app) fn best_standby_score(
    candidates: &[(usize, Duration, Duration)],
) -> Option<usize> {
    candidates
        .iter()
        .copied()
        .min_by_key(|(index, lease_age, uptime)| (*lease_age, std::cmp::Reverse(*uptime), *index))
        .map(|(index, _, _)| index)
}

pub(in crate::app) fn best_verified_standby(
    leases: &HashMap<String, StandbyHealthLease>,
    tunnels: &[ActiveTunnel],
    primary_url: Option<&str>,
) -> Option<usize> {
    let now = Instant::now();
    let candidates = tunnels
        .iter()
        .enumerate()
        .filter_map(|(index, tunnel)| {
            let lease = leases.get(tunnel.public_url())?;
            (primary_url != Some(tunnel.public_url()) && lease.eligible(now)).then_some((
                index,
                lease.age(now),
                tunnel.connected_for(),
            ))
        })
        .collect::<Vec<_>>();
    best_standby_score(&candidates)
}

pub(in crate::app) fn standby_lease_age(
    leases: &HashMap<String, StandbyHealthLease>,
    public_url: &str,
) -> Option<Duration> {
    leases
        .get(public_url)
        .map(|lease| lease.age(Instant::now()))
}

pub(in crate::app) fn dead_tunnel_index<F>(
    health_failed: bool,
    primary_index: Option<usize>,
    tunnel_count: usize,
    mut child_dead: F,
) -> Option<usize>
where
    F: FnMut(usize) -> bool,
{
    if tunnel_count == 0 {
        return None;
    }
    if health_failed {
        return primary_index;
    }
    (0..tunnel_count).find(|&index| child_dead(index))
}

pub(in crate::app) fn reconnect_backoff_seconds(deaths: u32) -> u64 {
    let exponent = deaths.saturating_sub(1).min(6);
    2u64.saturating_mul(1u64 << exponent).min(120)
}

pub(in crate::app) fn reconnect_delay_seconds(provider: TunnelProvider, deaths: u32) -> u64 {
    let base = reconnect_backoff_seconds(deaths);
    if base >= 120 {
        return 120;
    }
    let provider_seed = match provider {
        TunnelProvider::Auto => 0,
        TunnelProvider::Cloudflare => 1,
        TunnelProvider::LocalhostRun => 2,
        TunnelProvider::Pinggy => 0,
        TunnelProvider::Tailscale => 3,
        TunnelProvider::DevTunnel => 1,
    };
    let jitter = (provider_seed + deaths as u64) % 4;
    let staggered = base.saturating_add(jitter).min(120);
    if provider_circuit_open(deaths) {
        staggered.max(CIRCUIT_BREAKER_MIN_DELAY)
    } else {
        staggered
    }
}

pub(in crate::app) fn provider_circuit_open(deaths: u32) -> bool {
    deaths >= CIRCUIT_BREAKER_THRESHOLD
}

pub(in crate::app) fn record_dead_provider(
    death_counts: &mut HashMap<TunnelProvider, u32>,
    provider: TunnelProvider,
) -> u64 {
    let deaths = death_counts
        .entry(provider)
        .and_modify(|count| *count = count.saturating_add(1))
        .or_insert(1);
    reconnect_delay_seconds(provider, *deaths)
}

pub(in crate::app) fn record_recovered_provider(
    death_counts: &mut HashMap<TunnelProvider, u32>,
    provider: TunnelProvider,
) -> bool {
    death_counts.remove(&provider).is_some()
}

pub(in crate::app) fn provider_retry_due(due: Instant, now: Instant) -> bool {
    now >= due
}
