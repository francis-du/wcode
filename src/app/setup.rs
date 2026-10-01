use crate::agent_install;
use crate::git_provider::{GitHubEnrollment, GITHUB_ENROLLMENT_FILE};
use crate::workspace::Workspace;
use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::to_string_pretty;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::Path;

#[path = "setup_design.rs"]
mod setup_design;
#[path = "setup_policy.rs"]
mod setup_policy;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SetupScope {
    Global,
    Project,
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct GitHubEnrollmentInput<'a> {
    pub(super) repository: Option<&'a str>,
    pub(super) repository_id: Option<u64>,
    pub(super) app_id: Option<u64>,
    pub(super) check_name: Option<&'a str>,
}

impl GitHubEnrollmentInput<'_> {
    fn requested(self) -> bool {
        self.repository.is_some()
            || self.repository_id.is_some()
            || self.app_id.is_some()
            || self.check_name.is_some()
    }
}

pub(super) fn run(
    project_root: &Path,
    dry_run: bool,
    json: bool,
    global: bool,
    project: bool,
    launch_args: &[String],
    github: GitHubEnrollmentInput<'_>,
) -> Result<()> {
    if global && project {
        bail!("--global and --project cannot be used together");
    }
    if global && github.requested() {
        bail!("GitHub publisher enrollment is project-scoped and cannot be global");
    }
    let interactive = io::stdin().is_terminal() && io::stdout().is_terminal() && !json;
    let scope = if global {
        SetupScope::Global
    } else if project || github.requested() {
        SetupScope::Project
    } else if interactive {
        choose_scope()?
    } else {
        // A model/CI process must not silently mutate user-level configuration.
        SetupScope::Project
    };

    match scope {
        SetupScope::Global => run_global(dry_run, json, interactive, launch_args),
        SetupScope::Project => run_project(
            project_root,
            dry_run,
            json,
            interactive,
            launch_args,
            github,
        ),
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
    interactive: bool,
    launch_args: &[String],
    github: GitHubEnrollmentInput<'_>,
) -> Result<()> {
    let workspace = Workspace::new_with_security(
        project_root,
        true,
        false,
        crate::workspace::WorkspaceSecurity::default(),
    )?;
    // Host configuration writes keep their existing setup contract; project
    // bootstrap and draft editing honor the selected runtime read-only mode.
    let design_workspace = Workspace::new_with_security(
        project_root,
        !launch_args.iter().any(|argument| argument == "--read-only"),
        false,
        crate::workspace::WorkspaceSecurity::default(),
    )?;
    // Reject invalid or conflicting enrollment before host or Design writes.
    // The write below rechecks current state and remains create-only; this
    // preflight does not make the whole setup an atomic filesystem transaction.
    configure_github_enrollment(&design_workspace, true, github)?;
    let plan = agent_install::plan_configured_install(&workspace, false, launch_args);
    let summary = agent_install::apply_install(&workspace, plan, dry_run);
    let design_initialization = setup_design::initialize(&design_workspace, dry_run)?;
    let suggestion = setup_policy::suggest(&design_workspace)?;
    let github_enrollment = configure_github_enrollment(&design_workspace, dry_run, github)?;
    if json {
        // Preserve existing installation fields for clients; add draft advice.
        let mut value = serde_json::to_value(&summary)?;
        value["acceptance_policy_suggestion"] = serde_json::to_value(&suggestion)?;
        value["design_initialization"] = serde_json::to_value(&design_initialization)?;
        value["github_publisher_enrollment"] = serde_json::to_value(&github_enrollment)?;
        println!("{}", to_string_pretty(&value)?);
    } else {
        agent_install::print_human(&summary);
        setup_design::print(&design_initialization, &mut io::stdout().lock())?;
        setup_policy::print(&suggestion, &mut io::stdout().lock())?;
        print_github_enrollment(&github_enrollment, &mut io::stdout().lock())?;
    }
    ensure_success(&summary)?;
    if interactive && !dry_run && design_workspace.write_enabled() {
        setup_policy::review_with_io(
            &design_workspace,
            &suggestion,
            true,
            &mut io::stdin().lock(),
            &mut io::stdout().lock(),
        )?;
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct GitHubEnrollmentReport {
    status: &'static str,
    path: &'static str,
    enrollment: Option<GitHubEnrollment>,
    credential_stored: bool,
    remote_changes: bool,
}

fn configure_github_enrollment(
    workspace: &Workspace,
    dry_run: bool,
    input: GitHubEnrollmentInput<'_>,
) -> Result<GitHubEnrollmentReport> {
    let requested = match (input.repository, input.repository_id, input.app_id) {
        (None, None, None) if input.check_name.is_none() => None,
        (Some(repository), Some(repository_id), Some(app_id)) => Some(GitHubEnrollment::new(
            repository,
            repository_id,
            app_id,
            input.check_name,
        )?),
        _ => bail!("GitHub enrollment requires repository, repository ID and App ID together"),
    };
    let existing = GitHubEnrollment::load_workspace(workspace)?;
    let (status, enrollment) = match (existing, requested) {
        (Some(existing), None) => ("existing", Some(existing)),
        (Some(existing), Some(requested)) if existing == requested => ("existing", Some(existing)),
        (Some(_), Some(_)) => {
            bail!(
                "GitHub enrollment already exists with a different identity; review and edit it explicitly"
            )
        }
        (None, None) => ("missing", None),
        (None, Some(requested)) if dry_run => ("planned", Some(requested)),
        (None, Some(requested)) if !workspace.write_enabled() => ("blocked", Some(requested)),
        (None, Some(requested)) => {
            requested.create_in_workspace(workspace)?;
            ("enrolled", Some(requested))
        }
    };
    Ok(GitHubEnrollmentReport {
        status,
        path: GITHUB_ENROLLMENT_FILE,
        enrollment,
        credential_stored: false,
        remote_changes: false,
    })
}

fn print_github_enrollment(report: &GitHubEnrollmentReport, output: &mut impl Write) -> Result<()> {
    writeln!(
        output,
        "\nGitHub publisher enrollment: {} · {}",
        report.status, report.path
    )?;
    if let Some(enrollment) = &report.enrollment {
        writeln!(
            output,
            "  {} · repository ID {} · App ID {} · Check {}",
            enrollment.repository,
            enrollment.repository_id,
            enrollment.app_id,
            enrollment.check_name
        )?;
    }
    writeln!(
        output,
        "  No publisher credential is stored and setup does not change GitHub branch rules."
    )?;
    Ok(())
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
