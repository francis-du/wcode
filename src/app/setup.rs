use crate::agent_install;
use crate::workspace::Workspace;
use anyhow::{bail, Result};
use serde_json::to_string_pretty;
use std::io::{self, IsTerminal, Write};
use std::path::Path;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SetupScope {
    Global,
    Project,
}

pub(super) fn run(
    project_root: &Path,
    dry_run: bool,
    json: bool,
    global: bool,
    project: bool,
) -> Result<()> {
    if global && project {
        bail!("--global and --project cannot be used together");
    }
    let interactive = io::stdin().is_terminal() && io::stdout().is_terminal() && !json;
    let scope = if global {
        SetupScope::Global
    } else if project {
        SetupScope::Project
    } else if interactive {
        choose_scope()?
    } else {
        // A model/CI process must not silently mutate user-level configuration.
        SetupScope::Project
    };

    match scope {
        SetupScope::Global => run_global(dry_run, json, interactive),
        SetupScope::Project => run_project(project_root, dry_run, json),
    }
}

fn choose_scope() -> Result<SetupScope> {
    println!("\nWCode setup");
    println!("  1) Global (recommended)  Configure detected user-level Agent settings once.");
    println!("                          MCP command: wcode mcp-stdio");
    println!("                          The Agent's working directory becomes the Workspace.");
    println!("  2) Current project       Configure only this repository.");
    println!("  3) Cancel");
    print!("\nChoose [1]: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    match input.trim() {
        "" | "1" => Ok(SetupScope::Global),
        "2" => Ok(SetupScope::Project),
        "3" | "q" | "Q" => bail!("setup cancelled"),
        _ => bail!("invalid setup selection"),
    }
}

fn run_global(dry_run: bool, json: bool, interactive: bool) -> Result<()> {
    let workspace = agent_install::user_home_workspace()?;
    let preview = agent_install::apply_install(
        &workspace,
        agent_install::plan_global_install(&workspace),
        true,
    );
    if json {
        if !dry_run {
            bail!("global setup writes require an interactive TTY confirmation; use --dry-run --json for automation");
        }
        println!("{}", to_string_pretty(&preview)?);
        return Ok(());
    }
    println!("\nGlobal MCP entries use `wcode mcp-stdio`; no Workspace path is embedded.");
    agent_install::print_human(&preview);
    if dry_run {
        return Ok(());
    }
    if !interactive {
        bail!("global setup writes require an interactive TTY confirmation");
    }
    print!("\nApply these user-level configuration changes? [y/N]: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    if !matches!(input.trim(), "y" | "Y" | "yes" | "YES") {
        bail!("global setup cancelled; no user-level configuration was changed");
    }
    let summary = agent_install::apply_install(
        &workspace,
        agent_install::plan_global_install(&workspace),
        false,
    );
    agent_install::print_human(&summary);
    ensure_success(&summary)
}

fn run_project(project_root: &Path, dry_run: bool, json: bool) -> Result<()> {
    let workspace = Workspace::new_with_security(
        project_root,
        true,
        false,
        crate::workspace::WorkspaceSecurity::default(),
    )?;
    let summary =
        agent_install::apply_install(&workspace, agent_install::plan_install(&workspace), dry_run);
    if json {
        println!("{}", to_string_pretty(&summary)?);
    } else {
        agent_install::print_human(&summary);
    }
    ensure_success(&summary)
}

fn ensure_success(summary: &agent_install::AgentInstallSummary) -> Result<()> {
    if summary.failed.is_empty() {
        Ok(())
    } else {
        bail!(
            "{} Agent integration(s) failed safe configuration",
            summary.failed.len()
        )
    }
}
