use crate::tunnel::TunnelProvider;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub(in crate::app) const ENDPOINT_PROBE_INTERVAL: Duration = Duration::from_secs(3);
pub(in crate::app) const ENDPOINT_RETRY_INTERVAL: Duration = Duration::from_secs(1);
pub(in crate::app) const ENDPOINT_LEASE_TTL: Duration = Duration::from_secs(45);
const ENDPOINT_FAILURE_THRESHOLD: u8 = 2;
const ENDPOINT_RECOVERY_THRESHOLD: u8 = 2;
pub(in crate::app) const ENDPOINT_RECOVERY_GRACE: Duration = Duration::from_secs(30);
const CIRCUIT_BREAKER_THRESHOLD: u32 = 5;
const CIRCUIT_BREAKER_MIN_DELAY: u64 = 30;
static NEXT_ENDPOINT_LEASE_EPOCH: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug)]
pub(in crate::app) struct EndpointHealthLease {
    epoch: u64,
    verified_at: Instant,
    next_probe_at: Instant,
    consecutive_failures: u8,
    consecutive_successes: u8,
    quarantined_at: Option<Instant>,
    in_flight: bool,
}

impl EndpointHealthLease {
    pub(in crate::app) fn verified(now: Instant) -> Self {
        Self {
            epoch: NEXT_ENDPOINT_LEASE_EPOCH.fetch_add(1, Ordering::Relaxed),
            verified_at: now,
            next_probe_at: now + ENDPOINT_PROBE_INTERVAL,
            consecutive_failures: 0,
            consecutive_successes: 0,
            quarantined_at: None,
            in_flight: false,
        }
    }

    pub(in crate::app) fn begin_probe(&mut self, now: Instant) -> bool {
        if self.in_flight || now < self.next_probe_at {
            return false;
        }
        self.epoch = NEXT_ENDPOINT_LEASE_EPOCH.fetch_add(1, Ordering::Relaxed);
        self.in_flight = true;
        true
    }

    pub(in crate::app) fn record_success(&mut self, now: Instant) -> bool {
        let recovered = self.consecutive_failures > 0 || !self.eligible(now);
        self.in_flight = false;
        self.epoch = NEXT_ENDPOINT_LEASE_EPOCH.fetch_add(1, Ordering::Relaxed);
        self.consecutive_successes = self.consecutive_successes.saturating_add(1);
        if self.quarantined_at.is_some() && self.consecutive_successes < ENDPOINT_RECOVERY_THRESHOLD
        {
            self.next_probe_at = now + ENDPOINT_RETRY_INTERVAL;
            return false;
        }
        self.verified_at = now;
        self.next_probe_at = now + ENDPOINT_PROBE_INTERVAL;
        self.consecutive_failures = 0;
        self.quarantined_at = None;
        recovered
    }

    pub(in crate::app) fn record_failure(&mut self, now: Instant) -> bool {
        self.consecutive_failures = self.consecutive_failures.saturating_add(1);
        self.consecutive_successes = 0;
        self.next_probe_at = now + ENDPOINT_RETRY_INTERVAL;
        self.in_flight = false;
        self.epoch = NEXT_ENDPOINT_LEASE_EPOCH.fetch_add(1, Ordering::Relaxed);
        if self.consecutive_failures >= ENDPOINT_FAILURE_THRESHOLD {
            self.quarantined_at.get_or_insert(now);
        }
        // Endpoint health and process recycling have different deadlines.
        // Keep the original URL alive while the provider reconnects internally.
        self.quarantined_at
            .is_some_and(|since| now.saturating_duration_since(since) >= ENDPOINT_RECOVERY_GRACE)
    }

    #[cfg(test)]
    pub(in crate::app) fn quarantine(&mut self, now: Instant) {
        // Invalidate a probe started before quarantine; its success cannot
        // certify a recovery that has not actually been checked yet.
        self.epoch = NEXT_ENDPOINT_LEASE_EPOCH.fetch_add(1, Ordering::Relaxed);
        self.consecutive_failures = ENDPOINT_FAILURE_THRESHOLD;
        self.consecutive_successes = 0;
        self.quarantined_at.get_or_insert(now);
        self.next_probe_at = now;
        self.in_flight = false;
    }

    pub(in crate::app) fn eligible(&self, now: Instant) -> bool {
        self.quarantined_at.is_none()
            && now.saturating_duration_since(self.verified_at) <= ENDPOINT_LEASE_TTL
    }

    pub(in crate::app) fn failures(&self) -> u8 {
        self.consecutive_failures
    }

    pub(in crate::app) fn epoch(&self) -> u64 {
        self.epoch
    }
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
