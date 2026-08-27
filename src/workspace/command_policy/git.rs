use super::*;

pub(super) fn validate_git_command(args: &[String], allow_risky_exec: bool) -> Result<()> {
    let subcommand_index = args
        .iter()
        .position(|arg| !arg.starts_with('-'))
        .ok_or_else(|| anyhow!("git subcommand is required"))?;
    for option in &args[..subcommand_index] {
        if !matches!(option.as_str(), "--no-pager" | "--literal-pathspecs") {
            bail!("git global option is blocked by the workspace policy: {option}");
        }
    }
    let subcommand = args[subcommand_index].as_str();
    let tail = &args[subcommand_index + 1..];
    if matches!(
        subcommand,
        "status" | "diff" | "log" | "show" | "rev-parse" | "ls-files"
    ) {
        for arg in tail {
            if arg == "--ext-diff"
                || arg == "--textconv"
                || arg == "--open-files-in-pager"
                || arg == "--show-signature"
                || arg == "--output"
                || arg.starts_with("--output=")
                || arg.starts_with("--git-dir")
                || arg.starts_with("--work-tree")
                || arg.contains("%G")
            {
                bail!("git option can execute helpers or write outside the result stream: {arg}");
            }
        }
        return Ok(());
    }

    if subcommand == "lfs" {
        return validate_git_lfs(tail, allow_risky_exec);
    }
    match subcommand {
        "add" => validate_git_add(tail)?,
        "commit" => validate_git_commit(tail)?,
        "push" => validate_git_push(tail)?,
        _ => bail!("git mutation subcommand is permanently blocked: {subcommand}"),
    }
    require_risky_exec("git repository mutation", allow_risky_exec)
}

fn validate_git_add(args: &[String]) -> Result<()> {
    let mut path_count = 0usize;
    for arg in args {
        if arg == "--" {
            continue;
        }
        if arg.starts_with('-') {
            bail!("git add option is blocked; authorize explicit pathspecs only: {arg}");
        }
        if arg.starts_with(':') {
            bail!("git add magic pathspecs are blocked: {arg}");
        }
        if matches!(arg.as_str(), "." | "./") {
            bail!("git add requires explicit files/directories; broad dot pathspecs are blocked");
        }
        path_count += 1;
    }
    if path_count == 0 {
        bail!("git add requires at least one explicit pathspec");
    }
    Ok(())
}

fn validate_git_commit(args: &[String]) -> Result<()> {
    if args.is_empty() {
        bail!("git commit requires an explicit -m/--message to avoid opening an editor");
    }
    let mut index = 0usize;
    let mut messages = 0usize;
    while index < args.len() {
        let arg = &args[index];
        if matches!(arg.as_str(), "-m" | "--message") {
            let Some(message) = args.get(index + 1) else {
                bail!("git commit message value is required");
            };
            if message.is_empty() {
                bail!("git commit message must not be empty");
            }
            messages += 1;
            index += 2;
            continue;
        }
        if let Some(message) = arg.strip_prefix("--message=") {
            if message.is_empty() {
                bail!("git commit message must not be empty");
            }
            messages += 1;
            index += 1;
            continue;
        }
        bail!("git commit option is blocked; only explicit -m/--message is supported: {arg}");
    }
    if messages == 0 {
        bail!("git commit requires an explicit -m/--message");
    }
    Ok(())
}

fn validate_git_push(args: &[String]) -> Result<()> {
    let mut positional = 0usize;
    for arg in args {
        if matches!(arg.as_str(), "-u" | "--set-upstream") {
            continue;
        }
        if arg.starts_with('-') {
            bail!("git push option is blocked; force/delete/mirror/all/tag pushes are not authorizable: {arg}");
        }
        if arg.starts_with('+') || arg.ends_with(':') {
            bail!("git push force/delete refspecs are permanently blocked: {arg}");
        }
        positional += 1;
        if positional > 2 {
            bail!("git push accepts at most an explicit remote and one refspec");
        }
    }
    if positional != 2 {
        bail!("git push requires an explicit remote and one explicit refspec for auditable authorization");
    }
    Ok(())
}

fn validate_git_lfs(args: &[String], allow_risky_exec: bool) -> Result<()> {
    let action = args
        .first()
        .map(String::as_str)
        .ok_or_else(|| anyhow!("git lfs action is required"))?;
    match action {
        "status" | "ls-files" | "version" => Ok(()),
        "env" => bail!("git lfs env is blocked because remote/config output can contain sensitive endpoint information"),
        "fetch" | "pull" => require_risky_exec(&format!("git lfs {action}"), allow_risky_exec),
        "push" => {
            if args.iter().any(|arg| matches!(arg.as_str(), "--all" | "--object-id" | "--stdin")) {
                bail!("git lfs push broad/object-id/stdin modes are blocked; push one explicit remote/ref");
            }
            let positional = args[1..].iter().filter(|arg| !arg.starts_with('-')).count();
            if positional != 2 {
                bail!("git lfs push requires one explicit remote and one explicit ref");
            }
            require_risky_exec("git lfs push", allow_risky_exec)
        }
        "track" | "untrack" | "checkout" => {
            require_risky_exec(&format!("git lfs {action}"), allow_risky_exec)
        }
        "prune" | "migrate" | "uninstall" | "install" => {
            bail!("git lfs {action} is blocked by the bounded repository policy")
        }
        _ => bail!("git lfs action is blocked by the bounded repository policy: {action}"),
    }
}
