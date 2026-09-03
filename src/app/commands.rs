use crate::agent_plugin;
use clap::Subcommand;

#[derive(Clone, Debug, PartialEq, Eq, Subcommand)]
pub(super) enum ControlCommand {
    /// Set up WCode for detected coding agents.
    Setup {
        /// Preview what WCode would configure without changing files.
        #[arg(long)]
        dry_run: bool,
        /// Install user-level configuration for supported coding agents.
        #[arg(long, conflicts_with = "project")]
        global: bool,
        /// Configure only the current project.
        #[arg(long, conflicts_with = "global")]
        project: bool,
        /// Print JSON. Without a prompt, setup changes only this project.
        #[arg(long)]
        json: bool,
    },
    /// Update WCode to the latest verified release.
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
    /// Connect a coding agent over MCP using its current project directory.
    McpStdio,
    /// Show project health, language-server readiness, and code intelligence.
    Intelligence {
        /// Discover and initialize available language servers before showing status.
        #[arg(long)]
        refresh_semantic: bool,
        /// Exit with failure when required project checks are incomplete.
        #[arg(long)]
        check: bool,
        /// Print JSON instead of the terminal summary.
        #[arg(long)]
        json: bool,
    },
    /// Show verification plans and whether they are ready to run.
    Verification {
        /// Show one verification plan. Omit to list recent plans.
        #[arg(long = "plan-id", alias = "plan")]
        plan: Option<String>,
        /// Run configured advanced checks before showing status.
        #[arg(long)]
        execute_stages: bool,
        /// Print JSON instead of the terminal summary.
        #[arg(long)]
        json: bool,
    },
}
