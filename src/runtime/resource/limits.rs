use super::{ResourceLimits, DEFAULT_MAX_MEMORY_MB, MAX_MEMORY_MB, MAX_TOOL_SLOTS, MIN_MEMORY_MB};
use crate::harness::REPO_MAP_MAX_FILES;
use anyhow::{bail, Result};

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
        // Keep worker concurrency available for short interactive bursts. After
        // burst/debt credits are spent, pace foreground work at one CPU core;
        // unattended/background work keeps the stricter configured target.
        let cpu_burst_threads = host_threads
            .min(16)
            .min(usize::try_from(max_memory_mb / 64).unwrap_or(16))
            .min(requested_parallel_tools)
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
        // Heavy repository commands are latency-sensitive foreground work. Use
        // ~128 MiB of soft budget per child and never advertise more children
        // than the foreground CPU lanes can sustain at `child_threads` each.
        // The default 512 MiB profile remains memory-bounded at four 2-thread
        // children, while 1 GiB+ profiles can scale toward eight children on
        // hosts with more than eight foreground CPU lanes.
        let child_processes = usize::try_from(max_memory_mb / 128)
            .unwrap_or(8)
            .clamp(1, 8)
            .min(cpu_burst_threads.div_ceil(child_threads).max(1))
            .min(requested_parallel_tools)
            .max(1);
        let interactive_cpu_percent = 100.0;

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

    pub(crate) fn host_child_process_limit(self) -> usize {
        let memory_mb =
            usize::try_from(self.max_memory_bytes / (1024 * 1024)).unwrap_or(usize::MAX);
        (memory_mb / 64)
            .clamp(1, 16)
            .min(self.cpu_burst_threads.max(self.child_processes))
            .min(self.effective_parallel_tools)
            .max(self.child_processes)
    }

    #[cfg(test)]
    pub(super) fn unmanaged() -> Self {
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

    pub(crate) fn io_parallelism(self) -> usize {
        // Each active file operation may hold original, replacement and output
        // buffers. Share one bounded I/O pool instead of multiplying per batch.
        usize::try_from(self.max_memory_bytes / (16 * 1024 * 1024))
            .unwrap_or(32)
            .clamp(1, 32)
            .min(self.effective_parallel_tools)
            .max(1)
    }

    pub(super) fn probe_process_limit(self) -> usize {
        // Small, fixed-form Git inspections get separate capacity; compilers
        // and arbitrary commands keep the existing heavy-process bound.
        usize::try_from(self.max_memory_bytes / (64 * 1024 * 1024))
            .unwrap_or(8)
            .clamp(1, 8)
            .min(self.cpu_burst_threads)
            .min(self.effective_parallel_tools)
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

    pub(super) fn background_cpu_ratio(self) -> f64 {
        self.max_cpu_percent / 100.0
    }

    pub(super) fn interactive_cpu_ratio(self) -> f64 {
        self.interactive_cpu_percent / 100.0
    }

    pub(super) fn interactive_burst_seconds(self) -> f64 {
        // Foreground coding/indexing already has a hard worker cap. Keep a
        // larger latency-first credit window so a burst of independent tool
        // work does not turn into token-bucket sleeps while the user waits.
        8.0
    }

    pub(super) fn interactive_debt_seconds(self) -> f64 {
        4.0
    }

    pub(super) fn background_burst_seconds(self) -> f64 {
        0.5
    }
}
