use crate::tunnel::TunnelProvider;
use std::collections::HashMap;

pub(super) fn dead_tunnel_index<F>(
    health_failed: bool,
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
        return Some(0);
    }
    (0..tunnel_count).find(|&index| child_dead(index))
}

pub(super) fn reconnect_backoff_seconds(deaths: u32) -> u64 {
    let exponent = deaths.saturating_sub(1).min(6);
    5u64.saturating_mul(1u64 << exponent).min(300)
}

pub(super) fn record_dead_provider(
    death_counts: &mut HashMap<TunnelProvider, u32>,
    provider: TunnelProvider,
) -> u64 {
    let deaths = death_counts
        .entry(provider)
        .and_modify(|count| *count = count.saturating_add(1))
        .or_insert(1);
    reconnect_backoff_seconds(*deaths)
}

pub(super) fn record_recovered_provider(
    death_counts: &mut HashMap<TunnelProvider, u32>,
    provider: TunnelProvider,
) {
    death_counts.remove(&provider);
}

#[cfg(test)]
#[path = "../../tests/unit/app/tunnel_lifecycle.rs"]
mod tests;
