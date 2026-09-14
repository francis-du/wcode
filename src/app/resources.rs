use crate::resource;
use crate::workspace::WorkspaceSecurity;
use anyhow::Result;
use clap::{Args, ValueEnum};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
pub(super) enum Performance {
    /// Everyday coding; keeps the existing default resource budget.
    #[default]
    Balanced,
    /// More capacity for larger batches; uses a larger memory budget.
    Fast,
    /// Lower memory and background CPU budget for smaller machines.
    Light,
}

impl Performance {
    fn defaults(self) -> (f64, u64, usize) {
        match self {
            Self::Balanced => (
                resource::DEFAULT_MAX_CPU_PERCENT,
                resource::DEFAULT_MAX_MEMORY_MB,
                resource::DEFAULT_MAX_PARALLEL_TOOLS,
            ),
            Self::Fast => (resource::DEFAULT_MAX_CPU_PERCENT, 1024, 64),
            Self::Light => (5.0, 256, 16),
        }
    }
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
    /// Resource preset. Advanced numeric options override only their own value.
    #[arg(
        long,
        value_enum,
        default_value = "balanced",
        global = true,
        help_heading = "Experience"
    )]
    performance: Performance,

    /// Requested cap for concurrent tool bodies. Overrides the performance preset.
    #[arg(
        short = 'j',
        long = "max-parallel-tools",
        global = true,
        help_heading = "Runtime",
        hide = true
    )]
    parallel_tools: Option<usize>,

    /// Sustained CPU target for unattended background work; interactive requests may burst higher.
    #[arg(long, global = true, help_heading = "Runtime", hide = true)]
    max_cpu_percent: Option<f64>,

    /// Soft resident-memory budget in MiB with temporary burst headroom before admission pauses.
    #[arg(long, global = true, help_heading = "Runtime", hide = true)]
    max_memory_mb: Option<u64>,
}

impl ResourceArgs {
    pub(super) fn resolve(&self) -> Result<resource::ResourceLimits> {
        let (cpu, memory, parallel) = self.performance.defaults();
        resource::ResourceLimits::new(
            self.max_cpu_percent.unwrap_or(cpu),
            self.max_memory_mb.unwrap_or(memory),
            self.parallel_tools.unwrap_or(parallel),
        )
    }

    pub(super) fn activate(&self) -> Result<resource::ResourceLimits> {
        let limits = self.resolve()?;
        resource::install(limits)?;
        resource::configure_rayon()?;
        if self.performance == Performance::Light {
            if let Err(error) = resource::lower_process_priority() {
                eprintln!("  ! resources    process priority unchanged: {error}");
            }
        }
        Ok(limits)
    }
}

impl super::Args {
    pub(super) fn setup_launch_args(&self) -> Result<Vec<String>> {
        self.resources.resolve()?;
        if self.full_access
            || self.allow_risky_exec
            || self.allow_destructive_writes
            || self.allow_broad_workspace
            || self.allow_overlapping_workspaces
        {
            anyhow::bail!("setup does not persist broad permission grants; approve individual operations in the running agent or configure an explicitly trusted launch separately");
        }
        let mut args = vec!["mcp-stdio".to_owned()];
        let preset = match self.resources.performance {
            Performance::Balanced => None,
            Performance::Fast => Some("fast"),
            Performance::Light => Some("light"),
        };
        if let Some(preset) = preset {
            args.extend(["--performance".to_owned(), preset.to_owned()]);
        }
        for (name, value) in [
            (
                "--max-parallel-tools",
                self.resources.parallel_tools.map(|value| value.to_string()),
            ),
            (
                "--max-cpu-percent",
                self.resources
                    .max_cpu_percent
                    .map(|value| value.to_string()),
            ),
            (
                "--max-memory-mb",
                self.resources.max_memory_mb.map(|value| value.to_string()),
            ),
        ] {
            if let Some(value) = value {
                args.extend([name.to_owned(), value]);
            }
        }
        for (enabled, flag) in [
            (self.allow_write, "--read-only"),
            (self.allow_exec, "--no-exec"),
            (self.allow_semantic, "--no-semantic"),
        ] {
            if !enabled {
                args.push(flag.to_owned());
            }
        }
        Ok(args)
    }

    // Resolve through the startup path without installing a governor, loading
    // credentials, discovering roots, binding a socket or starting any child.
    pub(super) fn configuration_preview(&self) -> Result<Value> {
        let limits = self.resources.resolve()?;
        let public_url = self
            .public_url
            .as_deref()
            .map(crate::tunnel::normalize_public_url)
            .transpose()?;
        let mode = if matches!(self.command, Some(super::ControlCommand::McpStdio)) {
            "stdio"
        } else if self.command.is_some() {
            "command_only"
        } else if self.no_tunnel {
            "local_http"
        } else if public_url.is_some() {
            "external_http"
        } else {
            "managed_http"
        };
        let setup_launch = if matches!(self.command, Some(super::ControlCommand::Setup { .. })) {
            Some(json!({"command": "wcode", "args": self.setup_launch_args()?}))
        } else {
            None
        };
        let source = |explicit: bool| if explicit { "command_line" } else { "preset" };
        Ok(json!({
            "schema_version": 1,
            "preview": true,
            "runtime_started": false,
            "workspace_paths": self.workspace,
            "setup_launch": setup_launch,
            "resources": {
                "preset": self.resources.performance,
                "effective": limits,
                "sources": {
                    "parallel_tools": source(self.resources.parallel_tools.is_some()),
                    "max_cpu_percent": source(self.resources.max_cpu_percent.is_some()),
                    "max_memory_mb": source(self.resources.max_memory_mb.is_some()),
                },
            },
            "connection": {"mode": mode, "host": self.host, "port": self.port, "public_url": public_url},
            "permissions": {
                "write_enabled": self.allow_write || self.full_access,
                "exec_enabled": self.allow_exec || self.full_access,
                "semantic_enabled": (self.allow_semantic || self.full_access) && (self.allow_exec || self.full_access),
                "full_access": self.full_access,
                "risky_exec_enabled": self.allow_risky_exec || self.full_access,
                "destructive_writes_enabled": self.allow_destructive_writes || self.full_access,
            },
            "monitor_requested": self.monitor,
            "notes": [
                "Preview only; this is not the configuration of an already running process.",
                "Workspace paths and endpoint reachability are validated on startup.",
                "Resource limits are capacity bounds, not promised speedups or descendant-process memory limits.",
                "Performance presets never grant permissions; only an explicit setup operation saves agent launch options.",
            ],
        }))
    }
}

#[cfg(test)]
#[path = "../../tests/unit/app/resources.rs"]
mod tests;
