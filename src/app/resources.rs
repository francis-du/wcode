use crate::resource;
use crate::workspace::WorkspaceSecurity;
use anyhow::Result;
use clap::Args;

fn default_parallel_tools() -> usize {
    resource::DEFAULT_MAX_PARALLEL_TOOLS
}

#[derive(Clone, Copy)]
pub(super) struct SetupGuideOptions {
    pub(super) local_only: bool,
    pub(super) max_parallel_tools: usize,
    pub(super) max_cpu_percent: f64,
    pub(super) max_memory_mb: u64,
    pub(super) input_token_price_per_million_usd: f64,
    pub(super) security: WorkspaceSecurity,
}

#[derive(Debug, Args)]
pub(super) struct ResourceArgs {
    /// Requested cap for concurrent tool bodies. CPU-heavy sections are governed separately.
    #[arg(short = 'j', long = "max-parallel-tools", default_value_t = default_parallel_tools(), help_heading = "Runtime")]
    parallel_tools: usize,

    /// Sustained CPU target for unattended background work; interactive requests may burst higher.
    #[arg(long, default_value_t = resource::DEFAULT_MAX_CPU_PERCENT, help_heading = "Runtime")]
    max_cpu_percent: f64,

    /// Soft resident-memory budget in MiB with temporary burst headroom before admission pauses.
    #[arg(long, default_value_t = resource::DEFAULT_MAX_MEMORY_MB, help_heading = "Runtime")]
    max_memory_mb: u64,
}

impl ResourceArgs {
    pub(super) fn activate(&self) -> Result<resource::ResourceLimits> {
        let limits = resource::ResourceLimits::new(
            self.max_cpu_percent,
            self.max_memory_mb,
            self.parallel_tools,
        )?;
        resource::install(limits)?;
        resource::configure_rayon()?;
        if let Err(error) = resource::lower_process_priority() {
            eprintln!("  ! resources    process priority unchanged: {error}");
        }
        Ok(limits)
    }
}
