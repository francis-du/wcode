use crate::agent_plugin;
use anyhow::{anyhow, Result};
use clap::{Command, CommandFactory, Subcommand};
use serde_json::{json, Value};
use std::io::{self, Write};

#[derive(Clone, Debug, PartialEq, Eq, Subcommand)]
pub(super) enum ControlCommand {
    /// List all CLI commands and parameters, including advanced and compatibility options.
    #[command(
        after_help = "Examples:\n  wcode help-all\n  wcode help-all setup\n  wcode help-all agent-plugin\n  wcode help-all --json\nThis displays definitions only; it never executes the selected command."
    )]
    HelpAll {
        /// Optional subcommand path. Omit to show the entire CLI reference.
        #[arg(value_name = "COMMAND", num_args = 0..)]
        command_path: Vec<String>,
        /// Print the declarative CLI catalog as JSON, not live configuration or permissions.
        #[arg(long)]
        json: bool,
    },
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

const CATALOG_NOTE: &str = "Built-in CLI definitions only; not the MCP tool catalog or a list of authorized external programs.\nGlobal options work before or after subcommands; other root options must precede the subcommand.\nHidden options remain supported; deprecated or ignored compatibility options are labeled in their descriptions.\nDefaults are declarations, not live settings. Preset-dependent values are resolved by `wcode --show-config`.\nDisplaying help never grants permissions or executes the selected command.\nUse `wcode help-all <COMMAND>` for a focused page, or add `--json` for automation.";

pub(super) fn print_complete_help(path: &[String], as_json: bool) -> Result<()> {
    let output = complete_help(path, as_json)?;
    write_catalog(&mut io::stdout().lock(), output.as_bytes())?;
    Ok(())
}

fn write_catalog(writer: &mut impl Write, bytes: &[u8]) -> io::Result<()> {
    match writer.write_all(bytes).and_then(|()| writer.flush()) {
        Err(error) if error.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        result => result,
    }
}

fn complete_help(path: &[String], as_json: bool) -> Result<String> {
    // Build the canonical definition once, including generated help/version
    // flags and inherited global arguments. Never initialize a Workspace.
    let mut root = super::Args::command();
    root.build();
    let mut selected = &root;
    for name in path {
        selected = selected.find_subcommand(name).ok_or_else(|| {
            let available = selected
                .get_subcommands()
                .map(Command::get_name)
                .collect::<Vec<_>>();
            anyhow!(
                "unknown command path {path:?}; {:?} has subcommands: {}",
                selected.get_name(),
                if available.is_empty() {
                    "(none)".to_owned()
                } else {
                    available.join(", ")
                }
            )
        })?;
    }
    if as_json {
        return Ok(format!(
            "{}\n",
            serde_json::to_string_pretty(&json!({
                "schema_version": 1,
                "scope": "cli",
                "version": env!("CARGO_PKG_VERSION"),
                "runtime_started": false,
                "note": CATALOG_NOTE,
                "command": command_catalog(selected),
            }))?
        ));
    }
    let mut output = Vec::new();
    writeln!(
        output,
        "WCode {} — Complete CLI reference\n\n{CATALOG_NOTE}",
        env!("CARGO_PKG_VERSION")
    )?;
    write_command_help(selected, &mut output)?;
    Ok(String::from_utf8(output)?)
}

fn write_command_help(command: &Command, output: &mut impl Write) -> io::Result<()> {
    writeln!(
        output,
        "\n=== {}{} ===\n",
        command.get_bin_name().unwrap_or(command.get_name()),
        if command.is_hide_set() {
            " [hidden from default help]"
        } else {
            ""
        }
    )?;
    let aliases = command.get_all_aliases().collect::<Vec<_>>();
    if !aliases.is_empty() {
        writeln!(output, "Command aliases: {}\n", aliases.join(", "))?;
    }
    // Change only a display copy. The parser's permissions, conflicts,
    // defaults, validation and standard-help visibility remain untouched.
    let mut visible = command
        .clone()
        .hide(false)
        .color(clap::ColorChoice::Never)
        .after_help(None::<&str>)
        .after_long_help(None::<&str>)
        .mut_subcommands(|sub| sub.hide(false))
        .mut_args(|arg| {
            let mut help = arg
                .get_long_help()
                .or_else(|| arg.get_help())
                .map(ToString::to_string)
                .unwrap_or_default();
            if arg.is_hide_set() || arg.is_hide_long_help_set() || arg.is_hide_short_help_set() {
                help.push_str(" [Hidden from default help.]");
            }
            if arg.is_global_set() {
                help.push_str(" [Global option.]");
            }
            let aliases = arg
                .get_all_aliases()
                .unwrap_or_default()
                .into_iter()
                .map(|alias| format!("--{alias}"))
                .chain(
                    arg.get_all_short_aliases()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|alias| format!("-{alias}")),
                )
                .collect::<Vec<_>>();
            if !aliases.is_empty() {
                help.push_str(&format!(" Aliases: {}.", aliases.join(", ")));
            }
            arg.hide(false)
                .hide_short_help(false)
                .hide_long_help(false)
                .hide_possible_values(false)
                .long_help(help)
        });
    writeln!(output, "{}", visible.render_long_help())?;
    for child in command.get_subcommands() {
        write_command_help(child, output)?;
    }
    Ok(())
}

fn command_catalog(command: &Command) -> Value {
    let arguments = command.get_arguments().map(|arg| {
        let values = arg.get_possible_values().into_iter().map(|value| json!({
            "name": value.get_name(),
            "description": value.get_help().map(ToString::to_string),
            "hidden": value.is_hide_set(),
        })).collect::<Vec<_>>();
        json!({
            "id": arg.get_id().as_str(),
            "short": arg.get_short().map(|short| short.to_string()),
            "long": arg.get_long(),
            "aliases": arg.get_all_aliases().unwrap_or_default(),
            "short_aliases": arg.get_all_short_aliases().unwrap_or_default(),
            "description": arg.get_long_help().or_else(|| arg.get_help()).map(ToString::to_string),
            "heading": arg.get_help_heading(),
            "global": arg.is_global_set(),
            "positional": arg.is_positional(),
            "required": arg.is_required_set(),
            "hidden": arg.is_hide_set() || arg.is_hide_long_help_set() || arg.is_hide_short_help_set(),
            "action": format!("{:?}", arg.get_action()),
            "value_names": arg.get_value_names().unwrap_or_default().iter().map(|v| v.as_str()).collect::<Vec<_>>(),
            "default_values": arg.get_default_values().iter().map(|v| v.to_string_lossy()).collect::<Vec<_>>(),
            "possible_values": values,
        })
    }).collect::<Vec<_>>();
    json!({
        "name": command.get_name(),
        "invocation": command.get_bin_name().unwrap_or(command.get_name()),
        "description": command.get_long_about().or_else(|| command.get_about()).map(ToString::to_string),
        "aliases": command.get_all_aliases().collect::<Vec<_>>(),
        "hidden": command.is_hide_set(),
        "arguments": arguments,
        "subcommands": command.get_subcommands().map(command_catalog).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
#[path = "../../tests/unit/app/commands.rs"]
mod tests;
