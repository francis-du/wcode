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
        "status" | "diff" | "log" | "show" | "rev-parse" | "ls-files" | "grep"
    ) {
        for arg in tail {
            if arg == "--ext-diff"
                || arg == "--textconv"
                || arg == "--open-files-in-pager"
                || arg.starts_with("--open-files-in-pager=")
                || arg == "--show-signature"
                || arg == "--output"
                || arg.starts_with("--output=")
                || arg.starts_with("--git-dir")
                || arg.starts_with("--work-tree")
                || arg.contains("%G")
            {
                bail!("git option can execute helpers or write outside the result stream: {arg}");
            }
            if subcommand == "grep"
                && matches!(
                    arg.as_str(),
                    "--no-index" | "--untracked" | "--no-exclude-standard" | "--recurse-submodules"
                )
            {
                bail!(
                    "git grep option widens inspection beyond the bounded repository view: {arg}"
                );
            }
        }
        return Ok(());
    }

    if subcommand == "lfs" {
        return validate_git_lfs(tail, allow_risky_exec);
    }
    if git_inspection_only(subcommand, tail) {
        return Ok(());
    }
    match subcommand {
        "ls-remote" => {
            validate_git_remote_read(tail)?;
            require_risky_exec("git ls-remote", allow_risky_exec)?;
        }
        "fetch" => {
            validate_git_fetch(tail)?;
            require_risky_exec("git fetch", allow_risky_exec)?;
        }
        "add" => validate_git_add(tail)?,
        "commit" => validate_git_commit(tail)?,
        "push" => validate_git_push(tail)?,
        "branch" | "switch" | "tag" => validate_git_named_operation(subcommand, tail)?,
        "restore" if tail.first().is_some_and(|arg| arg == "--staged") => {
            validate_git_add(&tail[1..])?;
        }
        _ => bail!("git mutation subcommand is permanently blocked: {subcommand}"),
    }
    // Bounded Git lifecycle operations are the repository's publication path,
    // not an escape hatch. Their dangerous variants are rejected above, so
    // normal add/commit/branch/switch/tag/stage-restore/push can participate in
    // autonomous verified iteration without per-operation approval.
    Ok(())
}

// Only message values in a fully validated Git form are text, not paths.
// Argument control characters and the exact operation approval still apply.
pub(super) fn literal_message_indices(args: &[String]) -> Vec<usize> {
    if validate_git_command(args, true).is_err() {
        return Vec::new();
    }
    let Some(command) = args.iter().position(|arg| !arg.starts_with('-')) else {
        return Vec::new();
    };
    match args[command].as_str() {
        "commit" => {
            let mut indices = Vec::new();
            let mut index = command + 1;
            while index < args.len() {
                if matches!(args[index].as_str(), "-m" | "--message") {
                    indices.push(index + 1);
                    index += 2;
                } else {
                    indices.push(index);
                    index += 1;
                }
            }
            indices
        }
        "tag" if args.get(command + 1).is_some_and(|arg| arg == "-a") => {
            vec![command + 4]
        }
        _ => Vec::new(),
    }
}

fn git_inspection_only(command: &str, args: &[String]) -> bool {
    let args = args.iter().map(String::as_str).collect::<Vec<_>>();
    match (command, args.as_slice()) {
        ("branch" | "tag", [])
        | ("branch", ["--show-current"])
        | ("branch" | "tag", ["--list" | "-l"])
        | ("remote", [] | ["-v" | "--verbose"]) => true,
        ("branch" | "tag", ["--list" | "-l", pattern]) => !pattern.starts_with('-'),
        ("remote", ["get-url", remote]) => !remote.is_empty() && !remote.starts_with('-'),
        _ => false,
    }
}

fn validate_git_remote_read(args: &[String]) -> Result<()> {
    if args.len() != 2 || !bounded_remote_name(&args[0]) || !bounded_remote_ref(&args[1], false) {
        bail!("git ls-remote requires one configured remote name and one explicit heads/tags ref");
    }
    Ok(())
}

fn validate_git_fetch(args: &[String]) -> Result<()> {
    if args.len() != 2 || !bounded_remote_name(&args[0]) || !bounded_remote_ref(&args[1], true) {
        bail!("git fetch requires one configured remote name and one explicit branch ref");
    }
    Ok(())
}

fn bounded_remote_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value != "."
        && value != ".."
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
}

fn bounded_remote_ref(value: &str, allow_short_branch: bool) -> bool {
    if value == "HEAD" {
        return !allow_short_branch;
    }
    let body = if let Some(body) = value.strip_prefix("refs/heads/") {
        body
    } else if !allow_short_branch {
        let Some(body) = value.strip_prefix("refs/tags/") else {
            return false;
        };
        body
    } else {
        value
    };
    !body.is_empty()
        && body.len() <= 256
        && !body.starts_with('-')
        && !body.contains("..")
        && !body.contains("//")
        && !body.contains("@{")
        && !body.ends_with('/')
        && body.split('/').all(|component| {
            !component.is_empty()
                && !component.starts_with('.')
                && !component.ends_with('.')
                && !component.ends_with(".lock")
                && component
                    .chars()
                    .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-'))
        })
}

fn validate_git_named_operation(command: &str, args: &[String]) -> Result<()> {
    let positional = |values: &[String]| {
        !values.is_empty()
            && values.len() <= 2
            && values
                .iter()
                .all(|arg| !arg.is_empty() && !arg.starts_with('-'))
    };
    let valid = match command {
        "branch" | "tag" => positional(args),
        "switch" => {
            (args.len() == 1 && positional(args))
                || (args
                    .first()
                    .is_some_and(|arg| arg == "-c" || arg == "--create")
                    && positional(&args[1..]))
        }
        _ => false,
    };
    let annotated_tag = command == "tag"
        && args.len() >= 4
        && args.len() <= 5
        && args[0] == "-a"
        && !args[1].is_empty()
        && !args[1].starts_with('-')
        && args[2] == "-m"
        && !args[3].is_empty()
        && args
            .get(4)
            .is_none_or(|arg| !arg.is_empty() && !arg.starts_with('-'));
    if valid || annotated_tag {
        Ok(())
    } else {
        bail!("git {command} requires explicit names; force/delete/editor/helper modes remain unavailable")
    }
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
    if args.is_empty() {
        return Ok(());
    }
    let mut positional = 0usize;
    let mut set_upstream = false;
    for arg in args {
        if matches!(arg.as_str(), "-u" | "--set-upstream") {
            set_upstream = true;
            continue;
        }
        if arg.starts_with('-') {
            bail!("git push option is blocked; force/delete/mirror/all/tag pushes are permanently unavailable: {arg}");
        }
        if arg.starts_with('+') || arg.ends_with(':') {
            bail!("git push force/delete refspecs are permanently blocked: {arg}");
        }
        positional += 1;
        if positional > 2 {
            bail!("git push accepts either the current upstream or an explicit remote and one refspec");
        }
    }
    if positional != 2 {
        bail!(
            "git push accepts either no arguments or an explicit remote and one explicit refspec"
        );
    }
    if set_upstream && positional != 2 {
        bail!("git push --set-upstream requires an explicit remote and refspec");
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
