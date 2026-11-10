//! The sampler's own state: the bounded measured history the Observatory shows,
//! the raw process sample it is built from, and the CPU burst budget.
use super::*;

/// Two minutes of history at the sampling interval. The ring holds measured
/// points and drops the oldest when full, so it can never grow without bound.
const MAX_RESOURCE_HISTORY: usize = 240;

/// One measured point. `elapsed_ms` is relative to the process start, not a
/// wall clock, and the points are whatever the sampler actually produced: a
/// skipped tick leaves a gap rather than an interpolated value.
#[derive(Clone, Debug, Serialize)]
pub struct ResourceSamplePoint {
    pub elapsed_ms: u64,
    pub cpu_percent: Option<f64>,
    pub sustained_cpu_percent: Option<f64>,
    pub resident_memory_bytes: Option<u64>,
    pub memory_pressure: MemoryPressure,
    pub child_queue_waiting: usize,
    pub probe_queue_waiting: usize,
}

pub(super) fn push_sample(telemetry: &mut Telemetry, snapshot: &ResourceSnapshot, now: Instant) {
    telemetry.history.push_back(ResourceSamplePoint {
        elapsed_ms: duration_ms(now.saturating_duration_since(telemetry.started_at)),
        cpu_percent: snapshot.cpu_percent,
        sustained_cpu_percent: snapshot.sustained_cpu_percent,
        resident_memory_bytes: snapshot.resident_memory_bytes,
        memory_pressure: snapshot.memory_pressure,
        child_queue_waiting: snapshot.child_queue.waiting,
        probe_queue_waiting: snapshot.probe_queue.waiting,
    });
    while telemetry.history.len() > MAX_RESOURCE_HISTORY {
        telemetry.history.pop_front();
    }
}

#[derive(Clone, Copy)]
pub(super) struct ProcessSample {
    pub(super) cpu_time: Duration,
    pub(super) resident_memory_bytes: u64,
}

pub(super) struct CpuBudget {
    pub(super) last_refill: Instant,
    pub(super) background_credit_seconds: f64,
    pub(super) tokens: f64,
}
