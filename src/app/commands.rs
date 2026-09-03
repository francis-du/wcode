use crate::agent_plugin;
use clap::Subcommand;

#[derive(Clone, Debug, PartialEq, Eq, Subcommand)]
pub(super) enum ControlCommand {
    /// Configure detected local coding agents. Interactive setup offers Global first, then Current project.
    Setup {
        /// Preview detected hosts and planned changes without writing.
        #[arg(long)]
        dry_run: bool,
        /// Configure verified user-level Agent settings. Actual writes require a local TTY confirmation.
        #[arg(long, conflicts_with = "project")]
        global: bool,
        /// Configure only the current project and skip the interactive scope chooser.
        #[arg(long, conflicts_with = "global")]
        project: bool,
        /// Emit machine-readable JSON. Without an explicit scope, non-interactive setup defaults to project scope.
        #[arg(long)]
        json: bool,
    },
    /// Update this wcode installation from the latest verified release.
    Update,
    /// Export a portable Agent Plugins 1.0 package or use the legacy installer surface.
    #[command(hide = true)]
    AgentPlugin {
        /// Repository-relative output directory. Existing files are never overwritten.
        #[arg(long, default_value = "wcode")]
        output: String,
        /// Export connection profile. The canonical skill-only profile never guesses a Workspace.
        #[arg(long, value_enum, default_value = "skill-only")]
        profile: agent_plugin::AgentPluginProfile,
        /// Streamable HTTP endpoint used by the remote-http or auto profile. Secrets are never embedded.
        #[arg(long)]
        remote_url: Option<String>,
        /// Detect every known local Agent host and safely merge project-local wcode configuration.
        #[arg(long)]
        install_all: bool,
        /// Show detection evidence and planned files without writing.
        #[arg(long, requires = "install_all")]
        dry_run: bool,
        /// Emit machine-readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Serve the same MCP runtime over stdin/stdout for local coding agents and Agent Plugins.
    McpStdio,
    /// Inspect local Design, Graph, Semantic, Evidence, and Reconciliation runtime state.
    Intelligence {
        /// Refresh detected first-party LSP semantic providers before rendering status.
        #[arg(long)]
        refresh_semantic: bool,
        /// Fail closed when Design, Traceability, Product Scope, or required convention gates are incomplete.
        #[arg(long)]
        check: bool,
        /// Emit machine-readable JSON instead of the compact terminal summary.
        #[arg(long)]
        json: bool,
    },
    /// Inspect persisted Verification Plans and their current readiness gates.
    Verification {
        /// Inspect one specific Verification Plan ID. Omit to list recent plans.
        #[arg(long = "plan-id", alias = "plan")]
        plan: Option<String>,
        /// Execute configured or auto-discovered Property/Mutation/Fuzz/Canary stages first.
        #[arg(long)]
        execute_stages: bool,
        /// Emit machine-readable JSON instead of the compact terminal summary.
        #[arg(long)]
        json: bool,
    },
}
