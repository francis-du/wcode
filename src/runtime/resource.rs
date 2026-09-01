//! Cooperative process resource governance.
//!
//! wcode keeps latency-sensitive networking responsive, permits high I/O
//! concurrency and short CPU bursts, then paces repeated CPU-heavy work only
//! when load stays high. Process telemetry sheds cold caches before sustained
//! memory pressure is allowed to pause new work.

use crate::harness::{ToolHarness, REPO_MAP_MAX_FILES};
use crate::monitor::{OperatorMessageKind, TaskMonitor};
use anyhow::{anyhow, bail, Result};
use serde::Serialize;
use serde_json::{json, Value};
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::{Condvar, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};
use tokio::process::{Child, Command};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;

pub const DEFAULT_MAX_CPU_PERCENT: f64 = 10.0;
pub const DEFAULT_MAX_MEMORY_MB: u64 = 512;
pub const DEFAULT_MAX_PARALLEL_TOOLS: usize = 32;
pub const TOKIO_WORKER_THREADS: usize = 4;
pub const TOKIO_MAX_BLOCKING_THREADS: usize = 16;

const MIN_MEMORY_MB: u64 = 128;
const MAX_MEMORY_MB: u64 = 32 * 1024;
const MAX_TOOL_SLOTS: usize = 256;
const SAMPLE_INTERVAL: Duration = Duration::from_millis(500);
const TRIM_INTERVAL: Duration = Duration::from_secs(10);
const MIN_TOKEN: f64 = 0.000_5;
const MAX_THROTTLE_SLEEP: Duration = Duration::from_millis(250);
const TELEMETRY_STALE_AFTER: Duration = Duration::from_secs(2);
const SUSTAINED_CPU_ALPHA: f64 = 0.08;
const SUSTAINED_CPU_GRACE: Duration = Duration::from_secs(5);
const MEMORY_CRITICAL_DELAY: Duration = Duration::from_millis(500);
const MEMORY_OVER_LIMIT_TIMEOUT: Duration = Duration::from_secs(10);
const PROCESS_NICE_VALUE: i32 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
pub struct ResourceLimits {
    /// Sustained CPU target for unattended/background work. 100% is one core.
    pub max_cpu_percent: f64,
    /// Sustained CPU ceiling used while serving active tool requests.
    pub interactive_cpu_percent: f64,
    pub max_memory_bytes: u64,
    pub requested_parallel_tools: usize,
    pub effective_parallel_tools: usize,
    pub cpu_burst_threads: usize,
    pub rayon_threads: usize,
    pub child_processes: usize,
    pub child_threads: usize,
}

impl ResourceLimits {
    pub fn new(
        max_cpu_percent: f64,
        max_memory_mb: u64,
        requested_parallel_tools: usize,
    ) -> Result<Self> {
        if !max_cpu_percent.is_finite() || !(1.0..=100.0).contains(&max_cpu_percent) {
            bail!("--max-cpu-percent must be a finite number between 1 and 100");
        }
        if !(MIN_MEMORY_MB..=MAX_MEMORY_MB).contains(&max_memory_mb) {
            bail!("--max-memory-mb must be between {MIN_MEMORY_MB} and {MAX_MEMORY_MB}");
        }
        if !(1..=MAX_TOOL_SLOTS).contains(&requested_parallel_tools) {
            bail!("max parallel tools must be between 1 and {MAX_TOOL_SLOTS}");
        }

        let host_threads = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(8)
            .max(1);
        let memory_slots = if max_memory_mb <= 256 {
            16
        } else if max_memory_mb <= 512 {
            64
        } else if max_memory_mb <= 1_024 {
            128
        } else {
            256
        };
        let cpu_burst_threads = host_threads
            .min(if max_cpu_percent <= 10.0 {
                4
            } else if max_cpu_percent <= 25.0 {
                6
            } else {
                8
            })
            .max(1);
        let rayon_threads = cpu_burst_threads;
        let child_threads = host_threads
            .min(if max_cpu_percent <= 10.0 {
                2
            } else if max_cpu_percent <= 25.0 {
                3
            } else {
                4
            })
            .max(1);
        let child_processes = if max_memory_mb <= 256 {
            1
        } else if max_memory_mb <= 1_024 {
            2
        } else {
            4
        };
        let interactive_cpu_percent = (max_cpu_percent * 8.0)
            .clamp(100.0, 200.0)
            .min(cpu_burst_threads as f64 * 100.0);

        Ok(Self {
            max_cpu_percent,
            interactive_cpu_percent,
            max_memory_bytes: max_memory_mb.saturating_mul(1024 * 1024),
            requested_parallel_tools,
            effective_parallel_tools: requested_parallel_tools.min(memory_slots).max(1),
            cpu_burst_threads,
            rayon_threads,
            child_processes,
            child_threads,
        })
    }

    #[cfg(test)]
    fn unmanaged() -> Self {
        Self {
            max_cpu_percent: 100.0,
            interactive_cpu_percent: 400.0,
            max_memory_bytes: 2 * 1024 * 1024 * 1024,
            requested_parallel_tools: 16,
            effective_parallel_tools: 16,
            cpu_burst_threads: 4,
            rayon_threads: 4,
            child_processes: 2,
            child_threads: 2,
        }
    }

    pub fn indexed_file_limit(self) -> usize {
        let by_memory = usize::try_from(self.max_memory_bytes / (1024 * 1024))
            .unwrap_or(512)
            .clamp(256, 2_048);
        if self.max_memory_bytes >= DEFAULT_MAX_MEMORY_MB * 1024 * 1024 {
            by_memory.max(REPO_MAP_MAX_FILES)
        } else {
            by_memory
        }
    }

    pub fn ast_file_limit(self) -> usize {
        usize::try_from(self.max_memory_bytes / (16 * 1024 * 1024))
            .unwrap_or(32)
            .clamp(8, 32)
    }

    pub fn ast_byte_limit(self) -> usize {
        usize::try_from(self.max_memory_bytes / 16)
            .unwrap_or(32 * 1024 * 1024)
            .clamp(8 * 1024 * 1024, 32 * 1024 * 1024)
    }

    pub fn semantic_session_limit(self) -> usize {
        usize::try_from(self.max_memory_bytes / (256 * 1024 * 1024))
            .unwrap_or(2)
            .clamp(1, 4)
    }

    pub fn project_cache_limit(self) -> usize {
        usize::try_from(self.max_memory_bytes / (32 * 1024 * 1024))
            .unwrap_or(32)
            .clamp(4, 32)
    }

    pub fn repo_map_cache_limit(self) -> usize {
        usize::try_from(self.max_memory_bytes / (256 * 1024 * 1024))
            .unwrap_or(4)
            .clamp(1, 4)
    }

    fn background_cpu_ratio(self) -> f64 {
        self.max_cpu_percent / 100.0
    }

    fn interactive_cpu_ratio(self) -> f64 {
        self.interactive_cpu_percent / 100.0
    }

    fn interactive_burst_seconds(self) -> f64 {
        4.0
    }

    fn interactive_debt_seconds(self) -> f64 {
        2.0
    }

    fn background_burst_seconds(self) -> f64 {
        0.5
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryPressure {
    Normal,
    Elevated,
    Critical,
    OverLimit,
}

#[derive(Clone, Copy, Debug)]
pub enum WorkClass {
    Interactive,
    Background,
}

#[derive(Clone, Debug, Serialize)]
pub struct ResourceSnapshot {
    pub max_cpu_percent: f64,
    pub interactive_cpu_percent: f64,
    pub cpu_percent: Option<f64>,
    pub sustained_cpu_percent: Option<f64>,
    pub cpu_pressure: bool,
    pub max_memory_bytes: u64,
    pub resident_memory_bytes: Option<u64>,
    pub peak_resident_memory_bytes: u64,
    pub memory_percent: Option<f64>,
    pub memory_pressure: MemoryPressure,
    pub requested_parallel_tools: usize,
    pub effective_parallel_tools: usize,
    pub cpu_burst_threads: usize,
    pub rayon_threads: usize,
    pub child_processes: usize,
    pub child_threads: usize,
    pub governed_cpu_ms: u64,
    pub throttle_sleep_ms: u64,
    pub admission_delays: u64,
    pub admission_rejections: u64,
    pub cache_trims: u64,
    pub sample_count: u64,
    pub last_sample_ms_ago: Option<u64>,
}

struct CpuBudget {
    last_refill: Instant,
    background_credit_seconds: f64,
    tokens: f64,
}

struct Telemetry {
    previous_sample: Option<ProcessSample>,
    previous_sample_at: Option<Instant>,
    cpu_percent: Option<f64>,
    sustained_cpu_percent: Option<f64>,
    cpu_pressure_since: Option<Instant>,
    resident_memory_bytes: Option<u64>,
    peak_resident_memory_bytes: u64,
    memory_pressure: MemoryPressure,
    governed_cpu: Duration,
    throttle_sleep: Duration,
    admission_delays: u64,
    admission_rejections: u64,
    cache_trims: u64,
    sample_count: u64,
    last_sample_at: Option<Instant>,
}

impl Default for Telemetry {
    fn default() -> Self {
        Self {
            previous_sample: None,
            previous_sample_at: None,
            cpu_percent: None,
            sustained_cpu_percent: None,
            cpu_pressure_since: None,
            resident_memory_bytes: None,
            peak_resident_memory_bytes: 0,
            memory_pressure: MemoryPressure::Normal,
            governed_cpu: Duration::ZERO,
            throttle_sleep: Duration::ZERO,
            admission_delays: 0,
            admission_rejections: 0,
            cache_trims: 0,
            sample_count: 0,
            last_sample_at: None,
        }
    }
}

#[derive(Clone, Copy)]
struct ProcessSample {
    cpu_time: Duration,
    resident_memory_bytes: u64,
}

#[derive(Default)]
struct CpuActivity {
    active: usize,
    interactive: usize,
    background: usize,
}

pub struct ResourceGovernor {
    limits: ResourceLimits,
    cpu_activity: Mutex<CpuActivity>,
    cpu_activity_changed: Condvar,
    cpu_budget: Mutex<CpuBudget>,
    child_slot: std::sync::Arc<Semaphore>,
    telemetry: Mutex<Telemetry>,
}

static GOVERNOR: OnceLock<ResourceGovernor> = OnceLock::new();

pub fn install(limits: ResourceLimits) -> Result<&'static ResourceGovernor> {
    if let Some(existing) = GOVERNOR.get() {
        if existing.limits != limits {
            bail!("resource governor was already initialized with different limits");
        }
        return Ok(existing);
    }
    let governor = ResourceGovernor::new(limits);
    GOVERNOR
        .set(governor)
        .map_err(|_| anyhow!("resource governor initialization raced"))?;
    Ok(GOVERNOR
        .get()
        .expect("resource governor was set immediately before this read"))
}

pub fn global() -> &'static ResourceGovernor {
    GOVERNOR.get_or_init(|| ResourceGovernor::new(fallback_limits()))
}

#[cfg(test)]
fn fallback_limits() -> ResourceLimits {
    ResourceLimits::unmanaged()
}

#[cfg(not(test))]
fn fallback_limits() -> ResourceLimits {
    ResourceLimits::new(
        DEFAULT_MAX_CPU_PERCENT,
        DEFAULT_MAX_MEMORY_MB,
        DEFAULT_MAX_PARALLEL_TOOLS,
    )
    .expect("built-in resource limits are valid")
}

pub fn limits() -> ResourceLimits {
    global().limits
}

pub fn cpu_work(class: WorkClass) -> CpuWorkGuard {
    global().begin_cpu_work(class)
}

pub fn snapshot() -> ResourceSnapshot {
    global().snapshot()
}

pub fn capabilities() -> Value {
    json!({
        "mode": "burst-friendly-sustained-load-governance",
        "cpu_scope": "high short bursts are allowed; repeated internal CPU work is paced and unattended background work uses the configured sustained target",
        "memory_scope": "resident-memory soft budget with burst headroom, cache/session shedding, and delayed fail-closed admission only under sustained over-limit pressure",
        "limits": snapshot(),
    })
}

impl ResourceGovernor {
    fn new(limits: ResourceLimits) -> Self {
        Self {
            limits,
            cpu_activity: Mutex::new(CpuActivity::default()),
            cpu_activity_changed: Condvar::new(),
            cpu_budget: Mutex::new(CpuBudget {
                last_refill: Instant::now(),
                background_credit_seconds: limits.background_burst_seconds(),
                tokens: limits.interactive_burst_seconds(),
            }),
            child_slot: std::sync::Arc::new(Semaphore::new(limits.child_processes)),
            telemetry: Mutex::new(Telemetry::default()),
        }
    }

    fn begin_cpu_work(&'static self, class: WorkClass) -> CpuWorkGuard {
        // Wait outside the lane, then re-check after lane acquisition. Without
        // the second check a large queue could all spend the same burst credit.
        loop {
            self.wait_for_cpu_credit(class);
            self.acquire_cpu_slot(class);
            if self.cpu_credit_available(class) {
                break;
            }
            self.release_cpu_slot(class);
        }
        CpuWorkGuard {
            governor: self,
            class,
            started_wall: Instant::now(),
            started_cpu: platform::thread_cpu_time(),
            _thread_bound: PhantomData,
        }
    }

    fn wait_for_cpu_credit(&self, class: WorkClass) {
        loop {
            let wait = {
                let mut budget = lock_recover(&self.cpu_budget);
                refill_budget(&mut budget, self.limits);
                let (credit, floor, ratio) = match class {
                    WorkClass::Interactive => (
                        budget.tokens,
                        -self.limits.interactive_debt_seconds(),
                        self.limits.interactive_cpu_ratio(),
                    ),
                    WorkClass::Background => (
                        budget.background_credit_seconds,
                        0.0,
                        self.limits.background_cpu_ratio(),
                    ),
                };
                if credit >= floor + MIN_TOKEN {
                    return;
                }
                Duration::from_secs_f64(((floor + MIN_TOKEN - credit) / ratio).max(0.001))
                    .min(MAX_THROTTLE_SLEEP)
            };
            std::thread::sleep(wait);
            lock_recover(&self.telemetry).throttle_sleep += wait;
        }
    }

    fn cpu_credit_available(&self, class: WorkClass) -> bool {
        let mut budget = lock_recover(&self.cpu_budget);
        refill_budget(&mut budget, self.limits);
        match class {
            WorkClass::Interactive => {
                budget.tokens >= -self.limits.interactive_debt_seconds() + MIN_TOKEN
            }
            WorkClass::Background => budget.background_credit_seconds >= MIN_TOKEN,
        }
    }

    fn acquire_cpu_slot(&self, class: WorkClass) {
        let mut activity = lock_recover(&self.cpu_activity);
        while activity.active >= self.limits.cpu_burst_threads
            || (matches!(class, WorkClass::Background)
                && (activity.background > 0 || activity.interactive > 0))
        {
            activity = self
                .cpu_activity_changed
                .wait(activity)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        activity.active = activity.active.saturating_add(1);
        match class {
            WorkClass::Interactive => activity.interactive = activity.interactive.saturating_add(1),
            WorkClass::Background => activity.background = activity.background.saturating_add(1),
        }
    }

    fn release_cpu_slot(&self, class: WorkClass) {
        let mut activity = lock_recover(&self.cpu_activity);
        activity.active = activity.active.saturating_sub(1);
        match class {
            WorkClass::Interactive => activity.interactive = activity.interactive.saturating_sub(1),
            WorkClass::Background => activity.background = activity.background.saturating_sub(1),
        }
        self.cpu_activity_changed.notify_all();
    }

    fn finish_cpu_work(
        &self,
        started_cpu: Option<Duration>,
        started_wall: Instant,
        class: WorkClass,
    ) {
        let cpu_used = platform::thread_cpu_time()
            .zip(started_cpu)
            .and_then(|(end, start)| end.checked_sub(start))
            .unwrap_or_else(|| started_wall.elapsed());
        {
            let mut budget = lock_recover(&self.cpu_budget);
            refill_budget(&mut budget, self.limits);
            let used = cpu_used.as_secs_f64();
            budget.background_credit_seconds -= used;
            if matches!(class, WorkClass::Background) {
                // Background maintenance must not consume the next foreground burst.
                budget.tokens += used;
            }
            budget.tokens -= cpu_used.as_secs_f64();
        }
        lock_recover(&self.telemetry).governed_cpu += cpu_used;
        self.release_cpu_slot(class);
    }

    pub async fn admit_tool(&self) -> Result<(), String> {
        let started = Instant::now();
        loop {
            let snapshot = self.snapshot();
            if snapshot
                .last_sample_ms_ago
                .is_some_and(|age| age > duration_ms(TELEMETRY_STALE_AFTER))
            {
                return Ok(());
            }

            let memory_delay = match snapshot.memory_pressure {
                MemoryPressure::Normal | MemoryPressure::Elevated => None,
                MemoryPressure::Critical if started.elapsed() < MEMORY_CRITICAL_DELAY => {
                    Some(Duration::from_millis(50))
                }
                MemoryPressure::Critical => None,
                MemoryPressure::OverLimit if started.elapsed() < MEMORY_OVER_LIMIT_TIMEOUT => {
                    Some(Duration::from_millis(100))
                }
                MemoryPressure::OverLimit => {
                    let mut telemetry = lock_recover(&self.telemetry);
                    telemetry.admission_rejections =
                        telemetry.admission_rejections.saturating_add(1);
                    return Err(format!(
                        "wcode resident memory remained above 125% of its {} MiB soft budget for {}s; caches were shed and the new task was rejected",
                        self.limits.max_memory_bytes / (1024 * 1024),
                        MEMORY_OVER_LIMIT_TIMEOUT.as_secs()
                    ));
                }
            };
            if let Some(delay) = memory_delay {
                self.record_admission_delay(delay);
                tokio::time::sleep(delay).await;
                continue;
            }

            if snapshot.cpu_pressure {
                let sustained = snapshot.sustained_cpu_percent.unwrap_or_default();
                let ratio = sustained / self.limits.interactive_cpu_percent.max(1.0);
                let delay = Duration::from_secs_f64(((ratio - 1.0) * 0.075).clamp(0.025, 0.250));
                self.record_admission_delay(delay);
                tokio::time::sleep(delay).await;
            }
            // CPU pressure reduces the arrival rate but must not freeze cheap reads.
            // Actual CPU-heavy sections still pay the shared CPU-time budget.
            return Ok(());
        }
    }

    fn record_admission_delay(&self, delay: Duration) {
        let mut telemetry = lock_recover(&self.telemetry);
        telemetry.admission_delays = telemetry.admission_delays.saturating_add(1);
        telemetry.throttle_sleep += delay;
    }

    pub async fn acquire_child(&self) -> Result<OwnedSemaphorePermit, String> {
        self.admit_tool().await?;
        self.child_slot
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "resource governor is shutting down".to_owned())
    }

    pub fn background_ready(&self) -> bool {
        let snapshot = self.snapshot();
        let telemetry_fresh = snapshot
            .last_sample_ms_ago
            .is_some_and(|age| age <= duration_ms(TELEMETRY_STALE_AFTER));
        if telemetry_fresh
            && (snapshot.memory_pressure != MemoryPressure::Normal || snapshot.cpu_pressure)
        {
            return false;
        }
        if telemetry_fresh
            && snapshot
                .sustained_cpu_percent
                .is_some_and(|cpu| cpu > self.limits.max_cpu_percent.mul_add(1.5, 2.0))
        {
            return false;
        }
        if lock_recover(&self.cpu_activity).interactive > 0 {
            return false;
        }
        let mut budget = lock_recover(&self.cpu_budget);
        refill_budget(&mut budget, self.limits);
        budget.background_credit_seconds >= MIN_TOKEN
    }

    fn sample_process(&self) -> ResourceSnapshot {
        let now = Instant::now();
        let sample = platform::process_sample();
        let mut telemetry = lock_recover(&self.telemetry);
        if let Some(sample) = sample {
            if let (Some(previous), Some(previous_at)) =
                (telemetry.previous_sample, telemetry.previous_sample_at)
            {
                let wall = now.saturating_duration_since(previous_at).as_secs_f64();
                if wall > 0.0 {
                    let cpu = sample
                        .cpu_time
                        .checked_sub(previous.cpu_time)
                        .unwrap_or_default()
                        .as_secs_f64();
                    let current = cpu / wall * 100.0;
                    telemetry.cpu_percent = Some(match telemetry.cpu_percent {
                        Some(prior) => prior * 0.35 + current * 0.65,
                        None => current,
                    });
                    let sustained = telemetry
                        .sustained_cpu_percent
                        .unwrap_or_default()
                        .mul_add(1.0 - SUSTAINED_CPU_ALPHA, current * SUSTAINED_CPU_ALPHA);
                    telemetry.sustained_cpu_percent = Some(sustained);
                    if sustained > self.limits.interactive_cpu_percent * 1.10 {
                        telemetry.cpu_pressure_since.get_or_insert(now);
                    } else if sustained < self.limits.interactive_cpu_percent * 0.85 {
                        telemetry.cpu_pressure_since = None;
                    }
                }
            }
            telemetry.resident_memory_bytes = Some(sample.resident_memory_bytes);
            telemetry.peak_resident_memory_bytes = telemetry
                .peak_resident_memory_bytes
                .max(sample.resident_memory_bytes);
            telemetry.memory_pressure =
                memory_pressure(sample.resident_memory_bytes, self.limits.max_memory_bytes);
            telemetry.previous_sample = Some(sample);
            telemetry.previous_sample_at = Some(now);
            telemetry.sample_count = telemetry.sample_count.saturating_add(1);
            telemetry.last_sample_at = Some(now);
        }
        snapshot_from(self.limits, &telemetry, now)
    }

    fn snapshot(&self) -> ResourceSnapshot {
        let now = Instant::now();
        snapshot_from(self.limits, &lock_recover(&self.telemetry), now)
    }

    pub fn record_cache_trim(&self) {
        let mut telemetry = lock_recover(&self.telemetry);
        telemetry.cache_trims = telemetry.cache_trims.saturating_add(1);
    }
}

pub struct CpuWorkGuard {
    governor: &'static ResourceGovernor,
    class: WorkClass,
    started_wall: Instant,
    started_cpu: Option<Duration>,
    // Thread CPU clocks are meaningful only on the thread where the guard started.
    _thread_bound: PhantomData<Rc<()>>,
}

impl Drop for CpuWorkGuard {
    fn drop(&mut self) {
        self.governor
            .finish_cpu_work(self.started_cpu, self.started_wall, self.class);
    }
}

pub fn configure_rayon() -> Result<()> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(limits().rayon_threads)
        .thread_name(|index| format!("wcode-cpu-{index}"))
        .build_global()
        .map_err(|error| anyhow!("cannot configure the global CPU worker pool: {error}"))
}

pub fn lower_process_priority() -> Result<()> {
    platform::lower_process_priority(PROCESS_NICE_VALUE)
}

pub fn apply_child_limits(command: &mut Command) {
    platform::configure_process_group(command);
    let limits = limits();
    let threads = limits.child_threads.to_string();
    command
        .env("CARGO_BUILD_JOBS", &threads)
        .env("CMAKE_BUILD_PARALLEL_LEVEL", &threads)
        .env("GOMAXPROCS", &threads)
        .env(
            "JAVA_TOOL_OPTIONS",
            format!("-XX:ActiveProcessorCount={threads}"),
        )
        .env("GRADLE_OPTS", format!("-Dorg.gradle.workers.max={threads}"))
        .env("MAKEFLAGS", format!("-j{threads}"))
        .env("NEXTEST_TEST_THREADS", &threads)
        .env("OMP_NUM_THREADS", &threads)
        .env("OPENBLAS_NUM_THREADS", &threads)
        .env("MKL_NUM_THREADS", &threads)
        .env("NUMEXPR_NUM_THREADS", &threads)
        .env("RAYON_NUM_THREADS", &threads)
        .env("RUST_TEST_THREADS", &threads)
        .env("SWIFTPM_MAXIMUM_PARALLELISM", &threads)
        .env("UV_THREADPOOL_SIZE", &threads)
        .env("VECLIB_MAXIMUM_THREADS", &threads);
}

pub struct ChildProcessGuard {
    process_group: Option<u32>,
}

impl ChildProcessGuard {
    pub fn terminate(&mut self) {
        platform::terminate_process_group_id(self.process_group.take());
    }
}

impl Drop for ChildProcessGuard {
    fn drop(&mut self) {
        self.terminate();
    }
}

pub fn supervise_child(child: &Child) -> ChildProcessGuard {
    ChildProcessGuard {
        process_group: child.id(),
    }
}

pub fn terminate_child(child: &mut Child) {
    platform::terminate_process_group(child);
    let _ = child.start_kill();
}

pub fn spawn_monitor(harness: ToolHarness, monitor: TaskMonitor) -> JoinHandle<()> {
    tokio::spawn(async move {
        let governor = global();
        let mut ticker = tokio::time::interval(SAMPLE_INTERVAL);
        ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut prior_pressure = MemoryPressure::Normal;
        let mut last_trim = Instant::now()
            .checked_sub(TRIM_INTERVAL)
            .unwrap_or_else(Instant::now);
        loop {
            ticker.tick().await;
            let snapshot = governor.sample_process();
            if snapshot.memory_pressure != prior_pressure {
                match snapshot.memory_pressure {
                    MemoryPressure::Normal if prior_pressure != MemoryPressure::Normal => {
                        monitor.operator_message(
                            OperatorMessageKind::Success,
                            "resources",
                            "memory pressure recovered",
                        );
                    }
                    MemoryPressure::Elevated => monitor.operator_message(
                        OperatorMessageKind::Warning,
                        "resources",
                        format!(
                            "memory at {:.0}% of the soft budget; trimming cold caches",
                            snapshot.memory_percent.unwrap_or_default()
                        ),
                    ),
                    MemoryPressure::Critical => monitor.operator_message(
                        OperatorMessageKind::Warning,
                        "resources",
                        format!(
                            "memory exceeded the soft budget ({:.0}%); dropping idle indexes and LSP sessions",
                            snapshot.memory_percent.unwrap_or_default()
                        ),
                    ),
                    MemoryPressure::OverLimit => monitor.operator_message(
                        OperatorMessageKind::Warning,
                        "resources",
                        "memory exceeded 125% of the soft budget; pausing new tool admissions",
                    ),
                    MemoryPressure::Normal => {}
                }
                prior_pressure = snapshot.memory_pressure;
            }
            if snapshot.memory_pressure != MemoryPressure::Normal
                && last_trim.elapsed() >= TRIM_INTERVAL
            {
                let aggressive = matches!(
                    snapshot.memory_pressure,
                    MemoryPressure::Critical | MemoryPressure::OverLimit
                );
                harness.trim_memory(aggressive);
                if aggressive {
                    platform::release_memory();
                }
                governor.record_cache_trim();
                last_trim = Instant::now();
            }
        }
    })
}

fn refill_budget(budget: &mut CpuBudget, limits: ResourceLimits) {
    let now = Instant::now();
    let elapsed = now.saturating_duration_since(budget.last_refill);
    budget.last_refill = now;
    let elapsed_seconds = elapsed.as_secs_f64();
    budget.tokens = (budget.tokens + elapsed_seconds * limits.interactive_cpu_ratio())
        .min(limits.interactive_burst_seconds());
    budget.background_credit_seconds = (budget.background_credit_seconds
        + elapsed_seconds * limits.background_cpu_ratio())
    .min(limits.background_burst_seconds());
}

fn memory_pressure(resident: u64, limit: u64) -> MemoryPressure {
    if resident >= limit.saturating_mul(125) / 100 {
        MemoryPressure::OverLimit
    } else if resident >= limit {
        MemoryPressure::Critical
    } else if resident >= limit.saturating_mul(80) / 100 {
        MemoryPressure::Elevated
    } else {
        MemoryPressure::Normal
    }
}

fn snapshot_from(limits: ResourceLimits, telemetry: &Telemetry, now: Instant) -> ResourceSnapshot {
    ResourceSnapshot {
        max_cpu_percent: limits.max_cpu_percent,
        interactive_cpu_percent: limits.interactive_cpu_percent,
        cpu_percent: telemetry.cpu_percent,
        sustained_cpu_percent: telemetry.sustained_cpu_percent,
        cpu_pressure: telemetry
            .cpu_pressure_since
            .is_some_and(|since| now.saturating_duration_since(since) >= SUSTAINED_CPU_GRACE),
        max_memory_bytes: limits.max_memory_bytes,
        resident_memory_bytes: telemetry.resident_memory_bytes,
        peak_resident_memory_bytes: telemetry.peak_resident_memory_bytes,
        memory_percent: telemetry
            .resident_memory_bytes
            .map(|resident| resident as f64 / limits.max_memory_bytes as f64 * 100.0),
        memory_pressure: telemetry.memory_pressure,
        requested_parallel_tools: limits.requested_parallel_tools,
        effective_parallel_tools: limits.effective_parallel_tools,
        cpu_burst_threads: limits.cpu_burst_threads,
        rayon_threads: limits.rayon_threads,
        child_processes: limits.child_processes,
        child_threads: limits.child_threads,
        governed_cpu_ms: duration_ms(telemetry.governed_cpu),
        throttle_sleep_ms: duration_ms(telemetry.throttle_sleep),
        admission_delays: telemetry.admission_delays,
        admission_rejections: telemetry.admission_rejections,
        cache_trims: telemetry.cache_trims,
        sample_count: telemetry.sample_count,
        last_sample_ms_ago: telemetry
            .last_sample_at
            .map(|sample| duration_ms(now.saturating_duration_since(sample))),
    }
}

fn duration_ms(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn lock_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[path = "resource/platform.rs"]
mod platform;

#[cfg(test)]
#[path = "../../tests/unit/runtime/resource.rs"]
mod tests;
