use crate::agent_install;
use crate::workspace::Workspace;
use anyhow::{bail, Result};
use serde_json::to_string_pretty;
use std::io::{self, BufRead, IsTerminal, Write};
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
    launch_args: &[String],
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
        SetupScope::Global => run_global(dry_run, json, interactive, launch_args),
        SetupScope::Project => run_project(project_root, dry_run, json, launch_args),
    }
}

fn choose_scope() -> Result<SetupScope> {
    println!("\nWCode setup");
    println!("  1) Global (recommended)  Set up WCode once for supported coding agents.");
    println!("                          They run `wcode mcp-stdio` automatically.");
    println!(
        "                          The agent's current directory becomes the project Workspace."
    );
    println!("  2) Current project       Set up WCode only for this project.");
    println!("  3) Cancel");
    choose_scope_with_io(&mut io::stdin().lock(), &mut io::stdout().lock())
}

fn choose_scope_with_io(input: &mut impl BufRead, output: &mut impl Write) -> Result<SetupScope> {
    loop {
        write!(output, "\nChoose [1]: ")?;
        output.flush()?;
        let mut line = String::new();
        if input.read_line(&mut line)? == 0 {
            bail!("setup cancelled; input closed without a selection");
        }
        match line.trim() {
            "" | "1" => return Ok(SetupScope::Global),
            "2" => return Ok(SetupScope::Project),
            "3" | "q" | "Q" => bail!("setup cancelled"),
            _ => writeln!(
                output,
                "Choose 1, 2 or 3; no configuration has been changed."
            )?,
        }
    }
}

fn run_global(dry_run: bool, json: bool, interactive: bool, launch_args: &[String]) -> Result<()> {
    let workspace = agent_install::user_home_workspace()?;
    let plan = if launch_args == ["mcp-stdio"] {
        agent_install::plan_global_install(&workspace)
    } else {
        agent_install::plan_configured_install(&workspace, true, launch_args)
    };
    let preview = agent_install::apply_install(&workspace, plan.clone(), true);
    if json {
        if !dry_run {
            bail!("global setup writes require an interactive TTY confirmation; use --dry-run --json for automation");
        }
        println!("{}", to_string_pretty(&preview)?);
        return ensure_success(&preview);
    }
    println!("\nEach coding agent supplies its current project automatically. Review the launch options below.");
    agent_install::print_human(&preview);
    if dry_run {
        return ensure_success(&preview);
    }
    ensure_success(&preview)?;
    if !interactive {
        bail!("global setup writes require an interactive TTY confirmation");
    }
    print!("\nApply this WCode setup? [y/N]: ");
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    if !input.trim().eq_ignore_ascii_case("y") && !input.trim().eq_ignore_ascii_case("yes") {
        bail!("WCode setup cancelled; no user-level configuration was changed");
    }
    // Apply exactly the reviewed plan; SHA guards reject files changed while
    // the operator was deciding instead of silently generating a new plan.
    let summary = agent_install::apply_install(&workspace, plan, false);
    agent_install::print_human(&summary);
    ensure_success(&summary)
}

fn run_project(
    project_root: &Path,
    dry_run: bool,
    json: bool,
    launch_args: &[String],
) -> Result<()> {
    let workspace = Workspace::new_with_security(
        project_root,
        true,
        false,
        crate::workspace::WorkspaceSecurity::default(),
    )?;
    let plan = agent_install::plan_configured_install(&workspace, false, launch_args);
    let summary = agent_install::apply_install(&workspace, plan, dry_run);
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
            "WCode could not configure {} coding agent integration(s) safely",
            summary.failed.len()
        )
    }
}

#[cfg(test)]
#[path = "../../tests/unit/app/setup.rs"]
mod tests;
