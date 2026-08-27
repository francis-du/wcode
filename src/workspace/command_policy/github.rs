use super::*;

pub(super) fn validate_gh_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    if args.iter().any(|arg| {
        matches!(
            arg.as_str(),
            "-R" | "--repo" | "--hostname" | "--web" | "--editor"
        ) || arg.starts_with("--repo=")
            || arg.starts_with("--hostname=")
    }) {
        bail!("gh repository/host redirection and interactive browser/editor modes are blocked; operate on the selected workspace repository");
    }
    let Some(group) = args.first().map(String::as_str) else {
        bail!("gh command group is required");
    };
    if group == "status" {
        return Ok(());
    }
    if matches!(
        group,
        "auth"
            | "api"
            | "alias"
            | "config"
            | "extension"
            | "secret"
            | "variable"
            | "ssh-key"
            | "gpg-key"
            | "codespace"
            | "copilot"
            | "agent-task"
    ) {
        bail!("gh {group} is blocked because it can expose credentials, bypass command inspection, or alter host-wide configuration");
    }
    let action = args
        .get(1)
        .map(String::as_str)
        .ok_or_else(|| anyhow!("gh {group} action is required"))?;
    let read_only = matches!(
        (group, action),
        ("pr", "list" | "view" | "status" | "checks" | "diff")
            | ("issue", "list" | "view" | "status")
            | ("run", "list" | "view" | "watch")
            | ("workflow", "list" | "view")
            | ("release", "list" | "view")
            | ("repo", "view")
            | ("search", "prs" | "issues" | "repos" | "code" | "commits")
    );
    if read_only {
        return Ok(());
    }
    match (group, action) {
        ("pr", "create") => validate_gh_pr_create(&args[2..])?,
        ("issue", "create") => validate_gh_issue_create(&args[2..])?,
        ("pr" | "issue", "comment") => validate_gh_comment(&args[2..])?,
        ("workflow", "run") => validate_gh_workflow_run(&args[2..])?,
        ("release", "create") => validate_gh_release_create(&args[2..])?,
        ("pr", "merge") => validate_gh_pr_merge(&args[2..])?,
        ("run", action @ ("rerun" | "cancel")) => validate_gh_run_mutation(action, &args[2..])?,
        _ => bail!("gh {group} {action} is blocked by the bounded GitHub policy"),
    }
    require_risky_exec("GitHub remote mutation", allow_risky_exec)
}

fn validate_gh_pr_create(args: &[String]) -> Result<()> {
    for required in ["--title", "--body", "--head", "--base"] {
        if !has_option_value(args, required) {
            bail!("gh pr create requires explicit {required} for non-interactive, auditable execution");
        }
    }
    validate_exact_options(
        args,
        0,
        &["--draft", "--no-maintainer-edit"],
        &["--title", "--body", "--head", "--base"],
        &[],
        "gh pr create",
    )
}

fn validate_gh_issue_create(args: &[String]) -> Result<()> {
    for required in ["--title", "--body"] {
        if !has_option_value(args, required) {
            bail!("gh issue create requires explicit {required} for non-interactive execution");
        }
    }
    validate_exact_options(args, 0, &[], &["--title", "--body"], &[], "gh issue create")
}

fn validate_gh_comment(args: &[String]) -> Result<()> {
    let target = args
        .first()
        .ok_or_else(|| anyhow!("gh comment requires an explicit PR/issue target"))?;
    if target.starts_with('-') {
        bail!("gh comment requires an explicit PR/issue target before options");
    }
    if !has_option_value(args, "--body") {
        bail!("gh comment requires an explicit --body");
    }
    validate_exact_options(args, 1, &[], &["--body"], &[], "gh comment")
}

fn validate_gh_release_create(args: &[String]) -> Result<()> {
    let tag = args
        .first()
        .ok_or_else(|| anyhow!("gh release create requires an explicit existing tag"))?;
    if tag.starts_with('-') {
        bail!("gh release create requires the tag before options");
    }
    if !args.iter().any(|arg| arg == "--verify-tag") {
        bail!(
            "gh release create requires --verify-tag so it cannot create a missing tag implicitly"
        );
    }
    if !args
        .iter()
        .any(|arg| arg == "--generate-notes" || has_option_value(args, "--notes"))
    {
        bail!("gh release create requires explicit --notes or --generate-notes");
    }
    validate_exact_options(
        args,
        1,
        &[
            "--verify-tag",
            "--generate-notes",
            "--draft",
            "--prerelease",
            "--fail-on-no-commits",
            "--latest",
        ],
        &["--title", "--notes", "--target", "--notes-start-tag"],
        &["--latest"],
        "gh release create",
    )
}

fn validate_gh_pr_merge(args: &[String]) -> Result<()> {
    let target = args
        .first()
        .ok_or_else(|| anyhow!("gh pr merge requires an explicit PR number or URL"))?;
    if target.starts_with('-') {
        bail!("gh pr merge requires an explicit PR target before options");
    }
    let method_count = ["--merge", "--squash", "--rebase"]
        .into_iter()
        .filter(|flag| args.iter().any(|arg| arg == *flag))
        .count();
    if method_count != 1 {
        bail!("gh pr merge requires exactly one explicit --merge, --squash, or --rebase method");
    }
    validate_exact_options(
        args,
        1,
        &["--merge", "--squash", "--rebase"],
        &["--match-head-commit"],
        &[],
        "gh pr merge",
    )
}

fn validate_gh_run_mutation(action: &str, args: &[String]) -> Result<()> {
    let run = args
        .first()
        .ok_or_else(|| anyhow!("gh run mutation requires an explicit run ID"))?;
    if run.starts_with('-') {
        bail!("gh run mutation requires an explicit run ID before options");
    }
    match action {
        "rerun" => validate_exact_options(
            args,
            1,
            &["--debug", "--failed"],
            &["--job"],
            &[],
            "gh run rerun",
        ),
        "cancel" => validate_exact_options(args, 1, &["--force"], &[], &[], "gh run cancel"),
        _ => bail!("unsupported gh run mutation: {action}"),
    }
}

fn validate_gh_workflow_run(args: &[String]) -> Result<()> {
    let workflow = args
        .first()
        .ok_or_else(|| anyhow!("gh workflow run requires an explicit workflow name or ID"))?;
    if workflow.starts_with('-') {
        bail!("gh workflow run requires an explicit workflow name or ID before options");
    }
    validate_exact_options(
        args,
        1,
        &[],
        &["--ref", "--raw-field", "-f"],
        &[],
        "gh workflow run",
    )
}

fn validate_exact_options(
    args: &[String],
    start: usize,
    boolean_options: &[&str],
    value_options: &[&str],
    inline_value_options: &[&str],
    label: &str,
) -> Result<()> {
    let mut index = start;
    while index < args.len() {
        let arg = &args[index];
        if boolean_options.contains(&arg.as_str()) {
            index += 1;
            continue;
        }
        if let Some((option, value)) = arg.split_once('=') {
            if (value_options.contains(&option) || inline_value_options.contains(&option))
                && !value.is_empty()
            {
                index += 1;
                continue;
            }
            bail!("{label} option is not allowed by the bounded policy: {arg}");
        }
        if value_options.contains(&arg.as_str()) {
            let Some(value) = args.get(index + 1) else {
                bail!("{label} option requires a value: {arg}");
            };
            if value.is_empty() || value.starts_with('-') {
                bail!("{label} option requires a non-option value: {arg}");
            }
            index += 2;
            continue;
        }
        bail!("{label} option or positional argument is not allowed by the bounded policy: {arg}");
    }
    Ok(())
}

fn has_option_value(args: &[String], option: &str) -> bool {
    args.windows(2)
        .any(|pair| pair[0] == option && !pair[1].is_empty() && !pair[1].starts_with('-'))
        || args.iter().any(|arg| {
            arg.strip_prefix(&format!("{option}="))
                .is_some_and(|value| !value.is_empty())
        })
}
